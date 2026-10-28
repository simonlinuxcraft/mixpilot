use serde::Serialize;
use serde_json::Value;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

mod hotkey;

fn env_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback))
}

fn config_path() -> PathBuf {
    env_dir("XDG_CONFIG_HOME", ".config").join("mixpilot").join("config.json")
}

/// The core holds an exclusive flock on this file while it runs.
fn core_running() -> bool {
    let Ok(f) = std::fs::OpenOptions::new().read(true).open(runtime_dir().join("mixpilot.lock")) else { return false };
    let free = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0;
    !free
}

/// Development tree: app/target/release/mixpilot -> core/target/release/mixpilot-core. Only from
/// there; inside an AppImage mount ../../.. is /tmp, where anyone can put a binary.
fn dev_core(exe_dir: &std::path::Path) -> Option<PathBuf> {
    exe_dir.ends_with("app/target/release").then(|| exe_dir.join("../../../core/target/release/mixpilot-core"))
}

fn core_binary() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    [
        std::env::var_os("MIXPILOT_CORE").map(PathBuf::from),
        Some(exe_dir.join("mixpilot-core")),
        // before /usr/lib: an installed core of another version would be replaced on every window open
        dev_core(&exe_dir),
        Some(PathBuf::from("/usr/lib/mixpilot/mixpilot-core")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}

/// Must match STATE_PROTO in the core.
const STATE_PROTO: u64 = 6;

fn age(path: &std::path::Path) -> Option<std::time::Duration> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()?.elapsed().ok()
}

/// A running core rewrites state.json at least once a second. The protocol it speaks, if it wrote
/// within 2 s.
fn fresh_proto() -> Option<u64> {
    let path = runtime_dir().join("mixpilot").join("state.json");
    if age(&path)? > std::time::Duration::from_secs(2) {
        return None;
    }
    let v: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    Some(v.get("proto").and_then(Value::as_u64).unwrap_or(0))
}

/// A core still starting or a busy machine can leave the state a few seconds old, that is no reason
/// to cut the audio; the lock is rewritten when a core starts.
fn core_hung() -> bool {
    let quiet = |name: &str| age(&runtime_dir().join(name)).is_none_or(|a| a > std::time::Duration::from_secs(10));
    core_running() && quiet("mixpilot.lock") && quiet("mixpilot/state.json")
}

/// A core that speaks another protocol is an older version (after an update), one that has written
/// nothing for 10 s is hung. Either is stopped by PID so a current one can take over.
fn replace_stale_core() {
    // Once per app run at most: if the only core binary around speaks another protocol, replacing it
    // again on every window open would cut the audio each time.
    static REPLACED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !core_running() {
        return;
    }
    let mut other = false;
    for _ in 0..30 {
        match fresh_proto() {
            Some(p) if p == STATE_PROTO => return,
            Some(_) => {
                other = true;
                break;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    let keep = if other { REPLACED.swap(true, std::sync::atomic::Ordering::Relaxed) } else { !core_hung() };
    if keep {
        return;
    }
    eprintln!("mixpilot: replacing an older or hung core");
    stop_core();
}

/// The core of this session: every published core writes its PID into this session's lock.
fn core_pid() -> Option<i32> {
    let is_core = |pid: &i32| std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "mixpilot-core");
    std::fs::read_to_string(runtime_dir().join("mixpilot.lock")).ok().and_then(|s| s.trim().parse().ok()).filter(|p| core_running() && is_core(p))
}

// ensure_core runs off the main thread and the watchdog has its own; two starts at once would race
// for the log and the lock. Quitting holds it too, so no start slips in after the core was stopped.
static STARTING: std::sync::Mutex<()> = std::sync::Mutex::new(());
static QUITTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Starts the audio core detached, so it keeps running after the window is closed.
fn start_core() -> Result<(), String> {
    let _start = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if QUITTING.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(());
    }
    replace_stale_core();
    if core_running() {
        return Ok(());
    }
    let bin = core_binary().ok_or("Audio-Kern nicht gefunden")?;
    let log_dir = env_dir("XDG_STATE_HOME", ".local/state").join("mixpilot");
    std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    // appended, so the last words of a crashed core survive the watchdog's next attempts
    let log_path = log_dir.join("core.log");
    if std::fs::metadata(&log_path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::rename(&log_path, log_dir.join("core.log.1"));
    }
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| e.to_string())?;
    let mut child = Command::new(bin)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    // keep the start lock until the core holds its own, or the next caller would start a second one
    for _ in 0..50 {
        if core_running() || !matches!(child.try_wait(), Ok(None)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // reap it if it exits while the window is open, so no zombie is left
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The core ends itself when PipeWire goes away (a restart, an update, a crash) or a channel fails,
/// and nothing else would bring it back; a hung one is replaced, also with the window closed. Waits
/// longer each time it dies young, so a system without PipeWire or a snap without its permissions is
/// not hammered.
fn watch_core() {
    std::thread::spawn(|| {
        let mut delay = Duration::from_secs(2);
        let mut last = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let hung = core_hung();
            if core_running() && !hung {
                if last.elapsed() > Duration::from_secs(60) {
                    delay = Duration::from_secs(2);
                }
            } else if last.elapsed() >= delay {
                eprintln!("mixpilot: audio core {}, starting it again", if hung { "hung" } else { "not running" });
                if let Err(e) = start_core() {
                    eprintln!("mixpilot: {e}");
                }
                last = Instant::now();
                delay = (delay * 2).min(Duration::from_secs(60));
            }
        }
    });
}

/// Even `async` commands run on a Tokio worker; anything that waits on a process goes to the blocking pool.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn core_status() -> bool {
    core_running()
}

#[tauri::command(async)]
fn ensure_core() -> Result<(), String> {
    start_core()
}

#[tauri::command]
fn get_config() -> Result<Value, String> {
    let text = std::fs::read_to_string(config_path()).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if v.is_object() { Ok(v) } else { Err("config is not a JSON object".into()) }
}

// The window, the tray and the push-to-mute thread all write the same file.
static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Read-modify-write for the tray menu toggles and the push-to-mute key.
fn update_config(f: impl FnOnce(&mut Value)) -> Result<(), String> {
    let _lock = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cfg = get_config()?;
    f(&mut cfg);
    write_config(&cfg)
}

/// The window sends its whole copy. It shows the microphone mute but never sets it, the tray and
/// the push-to-mute key own it, so the value on disk wins over a copy that may be a second old.
#[tauri::command]
fn set_config(mut cfg: Value) -> Result<(), String> {
    if !cfg.is_object() {
        return Err("config must be a JSON object".into());
    }
    let _lock = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    keep_from_disk(&mut cfg, get_config().ok().as_ref());
    write_config(&cfg)
}

/// The core's note about a replaced broken file stays until the window has shown it and sends null.
fn keep_from_disk(cfg: &mut Value, disk: Option<&Value>) {
    if let Some(mute) = disk.and_then(|c| c.pointer("/mic/mute")).cloned()
        && cfg["mic"].is_object()
    {
        cfg["mic"]["mute"] = mute;
    }
    match cfg.get("config_reset") {
        None => {
            if let Some(note) = disk.and_then(|c| c.get("config_reset")).cloned() {
                cfg["config_reset"] = note;
            }
        }
        Some(Value::Null) => {
            cfg.as_object_mut().map(|o| o.remove("config_reset"));
        }
        Some(_) => {}
    }
}

/// Write to a temp file and rename, so the core never reads a half-written file. No fsync, it costs
/// 5 to 15 ms per fader step on the main thread; the core repairs a file a power cut breaks.
fn write_config(cfg: &Value) -> Result<(), String> {
    let path = config_path();
    // own name per process, two writers never share a temp file
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&cfg).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Hardware outputs and microphones for the pickers, plus the current system defaults, as the core
/// last saw them in the PipeWire registry. Empty until a core has run.
#[tauri::command(async)]
fn list_devices() -> Value {
    std::fs::read(runtime_dir().join("mixpilot").join("state.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|mut s| s.get_mut("devices").map(Value::take))
        .unwrap_or_else(|| serde_json::json!({ "sinks": [], "sources": [], "default_sink": "", "default_source": "", "duplicate": false }))
}

/// Makes a microphone the system default, the same way the GNOME sound settings do.
#[tauri::command(async)]
fn set_default_source(name: String) -> Result<(), String> {
    let value = serde_json::json!({ "name": name }).to_string();
    let ok = output_within(Command::new("pw-metadata").args(["-n", "default", "0", "default.configured.audio.source", &value, "Spa:String:JSON"]), CMD_LIMIT)
        .is_ok_and(|o| o.status.success());
    if ok { Ok(()) } else { Err("Standard-Mikrofon konnte nicht gesetzt werden".into()) }
}

/// Without XDG_RUNTIME_DIR (a cron job, some containers): the session's /run/user/<uid>, else a
/// private directory in /tmp. One that another user created or linked there first is never used.
fn runtime_dir() -> PathBuf {
    use std::os::unix::fs::DirBuilderExt;
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let uid = unsafe { libc::getuid() };
    let run = PathBuf::from(format!("/run/user/{uid}"));
    if private_dir(&run, uid) {
        return run;
    }
    let tmp = std::env::temp_dir().join(format!("mixpilot-{uid}"));
    let _ = std::fs::DirBuilder::new().mode(0o700).create(&tmp);
    if private_dir(&tmp, uid) {
        return tmp;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache").join(format!("mixpilot-run-{uid}"));
    let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&home);
    home
}

/// A real directory (no symlink) of this user that nobody else can enter.
fn private_dir(path: &std::path::Path, uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && m.uid() == uid && m.mode() & 0o077 == 0)
}

/// Touches a file the core watches, at most once a second; it counts a touch for 3 s.
fn touch(path: PathBuf, last: &std::sync::Mutex<Option<Instant>>) {
    let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
        *last = Some(Instant::now());
        let _ = std::fs::write(path, b"");
    }
}

/// Live data from the core: meters, apps, events. Asking for it makes the core write it at 20 Hz
/// instead of once a second. `mic_test` keeps the microphone open while the microphone page is visible.
#[tauri::command]
fn get_state(mic_test: bool) -> Result<Value, String> {
    static VIEWER: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    static MIC: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    // a crashed core leaves its last state behind; never show frozen meters
    if !core_running() {
        return Err("core not running".into());
    }
    let dir = runtime_dir().join("mixpilot");
    touch(dir.join("viewer"), &VIEWER);
    if mic_test {
        touch(dir.join("mic-test"), &MIC);
    }
    let text = std::fs::read(dir.join("state.json")).map_err(|e| e.to_string())?;
    serde_json::from_slice(&text).map_err(|e| e.to_string())
}

fn mic_test_file() -> PathBuf {
    runtime_dir().join("mixpilot").join("mic-test.wav")
}

// a window closed and reopened during the five seconds must not start a second recording
static MIC_TEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Five seconds of the processed voice, the same signal Discord and OBS get.
#[tauri::command]
async fn mic_test_record() -> Result<(), String> {
    blocking(record_mic_test).await?
}

fn record_mic_test() -> Result<(), String> {
    // without the core there is no Mixpilot microphone, WirePlumber would hand pw-record the raw one
    if !core_running() {
        return Err("Audio-Kern gestoppt".into());
    }
    if MIC_TEST.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("Der Mikrofon-Test läuft schon".into());
    }
    let result = record_five_seconds(&mic_test_file());
    MIC_TEST.store(false, std::sync::atomic::Ordering::SeqCst);
    result
}

// not via timeout(1): the snap's AppArmor profile refuses to run /usr/bin/timeout
fn record_five_seconds(file: &std::path::Path) -> Result<(), String> {
    let mut cmd = Command::new("pw-record");
    cmd.args(["--target", "mixpilot_mic", "--rate", "48000", "--channels", "1"]).arg(file).stdout(Stdio::null()).stderr(Stdio::null());
    // the kernel sends SIGINT when Mixpilot ends, so a quit during the five seconds stops the recording
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGINT);
            Ok(())
        });
    }
    let mut rec = cmd.spawn().map_err(|e| format!("pw-record: {e}"))?;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Ok(Some(_)) = rec.try_wait() {
            return Err("Aufnahme fehlgeschlagen".into());
        }
    }
    // SIGINT lets pw-record finish the file header
    unsafe { libc::kill(rec.id() as i32, libc::SIGINT) };
    let _ = rec.wait();
    if std::fs::metadata(file).is_ok_and(|m| m.len() > 44) { Ok(()) } else { Err("Aufnahme fehlgeschlagen".into()) }
}

#[tauri::command]
async fn mic_test_play() -> Result<(), String> {
    blocking(play_mic_test).await?
}

fn play_mic_test() -> Result<(), String> {
    let file = mic_test_file();
    // on the device Mixpilot plays to, which may differ from the system default
    let mut play = Command::new("pw-play");
    let out = output_sink();
    if out != "@DEFAULT_SINK@" {
        play.args(["--target", &out]);
    }
    let played = play.arg(&file).stdout(Stdio::null()).stderr(Stdio::null()).status();
    let _ = std::fs::remove_file(&file);
    match played {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err("Wiedergabe fehlgeschlagen".into()),
        Err(e) => Err(format!("pw-play: {e}")),
    }
}

fn autostart_dir() -> PathBuf {
    env_dir("XDG_CONFIG_HOME", ".config").join("autostart")
}

fn autostart_file() -> PathBuf {
    autostart_dir().join("mixpilot.desktop")
}

#[tauri::command]
fn get_autostart() -> bool {
    autostart_file().is_file()
}

/// Starts Mixpilot at login in the tray, without a window; the app then starts the audio core.
#[tauri::command]
fn set_autostart(on: bool) -> Result<(), String> {
    let file = autostart_file();
    if !on {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    // In the snap, snapd starts the app named in snapcraft.yaml "autostart" from this file in
    // $SNAP_USER_DATA/.config/autostart, so Exec names the snap app instead of a path.
    let exec = if std::env::var_os("SNAP").is_some() {
        "mixpilot --background".to_string()
    } else {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        format!("\"{}\" --background", exe.canonicalize().unwrap_or(exe).display())
    };
    std::fs::create_dir_all(autostart_dir()).map_err(|e| e.to_string())?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Mixpilot\nComment=Mixer, sound and microphone control\nExec={exec}\nIcon=mixpilot\nX-GNOME-Autostart-enabled=true\n"
    );
    std::fs::write(&file, entry).map_err(|e| e.to_string())
}

/// The device Mixpilot plays on: the chosen one, or the system default.
fn output_sink() -> String {
    get_config()
        .ok()
        .and_then(|c| c.get("output").and_then(Value::as_str).map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "@DEFAULT_SINK@".into())
}

/// PipeWire tools answer in milliseconds; a hung pipewire-pulse keeps pactl waiting for 30 s.
const CMD_LIMIT: Duration = Duration::from_secs(2);

/// Command::output with a time limit. The pipes are read after the exit, so only for short outputs.
fn output_within(cmd: &mut Command, limit: Duration) -> std::io::Result<std::process::Output> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let end = Instant::now() + limit;
    while child.try_wait()?.is_none() {
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("no answer within {} s", limit.as_secs())));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait_with_output()
}

/// Volume commands run one after another, never in parallel.
static PACTL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn pactl(args: &[&str]) -> Result<String, String> {
    let _serial = PACTL.lock().unwrap_or_else(|e| e.into_inner());
    let out = output_within(Command::new("pactl").args(args).env("LC_ALL", "C"), CMD_LIMIT).map_err(|e| format!("pactl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[derive(Serialize, Clone)]
struct Master {
    volume: u32,
    mute: bool,
}

/// The last read: device, values, change count at the time of reading, when.
static MASTER: std::sync::Mutex<Option<(String, Master, u64, Instant)>> = std::sync::Mutex::new(None);
/// Sink and server changes `pactl subscribe` reported, and whether it runs at all.
static CHANGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SUBSCRIBED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn master_changed() {
    CHANGES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// Device and volume changes from `pactl subscribe`, so the window's poll asks pactl only after one.
/// Restarts with a growing pause when it ends (pipewire-pulse restarted or not there yet).
fn watch_master() {
    std::thread::spawn(|| {
        let mut delay = Duration::from_secs(1);
        loop {
            let started = Instant::now();
            let mut cmd = Command::new("pactl");
            cmd.arg("subscribe").env("LC_ALL", "C").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
            // the kernel ends it with this thread, which lives as long as the app
            unsafe {
                cmd.pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    Ok(())
                });
            }
            if let Ok(mut child) = cmd.spawn() {
                if let Some(out) = child.stdout.take() {
                    master_changed();
                    SUBSCRIBED.store(true, std::sync::atomic::Ordering::SeqCst);
                    for line in std::io::BufRead::lines(std::io::BufReader::new(out)).map_while(Result::ok) {
                        if line.contains(" on sink #") || line.contains(" on server") {
                            master_changed();
                        }
                    }
                }
                SUBSCRIBED.store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = child.wait();
            }
            if started.elapsed() > Duration::from_secs(60) {
                delay = Duration::from_secs(1);
            }
            std::thread::sleep(delay);
            delay = (delay * 2).min(Duration::from_secs(60));
        }
    });
}

/// Master is the volume of the output device itself, the same one GNOME and the volume keys change.
#[tauri::command(async)]
fn get_master() -> Result<Master, String> {
    static WATCH: std::sync::Once = std::sync::Once::new();
    WATCH.call_once(watch_master);
    let sink = output_sink();
    let seen = CHANGES.load(std::sync::atomic::Ordering::SeqCst);
    // read again after 5 s anyway: an event can get lost while subscribe is still connecting
    if SUBSCRIBED.load(std::sync::atomic::Ordering::SeqCst)
        && let Some((s, m, at_change, at)) = &*MASTER.lock().unwrap_or_else(|e| e.into_inner())
        && *s == sink
        && *at_change == seen
        && at.elapsed() < Duration::from_secs(5)
    {
        return Ok(m.clone());
    }
    let vol = pactl(&["get-sink-volume", &sink])?;
    let volume = vol
        .split('/')
        .nth(1)
        .and_then(|p| p.trim().trim_end_matches('%').trim().parse().ok())
        .ok_or("unknown volume format")?;
    let mute = pactl(&["get-sink-mute", &sink])?.contains("yes");
    *MASTER.lock().unwrap_or_else(|e| e.into_inner()) = Some((sink, Master { volume, mute }, seen, Instant::now()));
    Ok(Master { volume, mute })
}

#[tauri::command(async)]
fn set_master(volume: u32) -> Result<(), String> {
    master_changed();
    pactl(&["set-sink-volume", &output_sink(), &format!("{}%", volume.min(150))])
        .map(|_| ())
        .map_err(|e| { eprintln!("mixpilot: pactl: {e}"); "Lautstärke des Ausgabegeräts konnte nicht geändert werden".to_string() })
}

#[tauri::command(async)]
fn set_master_mute(mute: bool) -> Result<(), String> {
    master_changed();
    pactl(&["set-sink-mute", &output_sink(), if mute { "1" } else { "0" }])
        .map(|_| ())
        .map_err(|e| { eprintln!("mixpilot: pactl: {e}"); "Lautstärke des Ausgabegeräts konnte nicht geändert werden".to_string() })
}

#[derive(Serialize)]
struct Setup {
    pipewire: bool,
    conflicts: Vec<String>,
    snap: bool,
    snap_missing: Vec<String>,
}

/// Everything the first-run window checks.
#[tauri::command(async)]
fn setup_check() -> Setup {
    let pipewire = output_within(Command::new("pw-cli").args(["info", "0"]), CMD_LIMIT).is_ok_and(|o| o.status.success());
    let running = |name: &str| Command::new("pgrep").arg("-x").arg(name).stdout(Stdio::null()).status().is_ok_and(|s| s.success());
    let conflicts = [("easyeffects", "Easy Effects"), ("jamesdsp", "JamesDSP"), ("jdsp-gui", "JamesDSP")]
        .iter()
        .filter(|(p, _)| running(p))
        .map(|(_, n)| n.to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let snap = std::env::var_os("SNAP").is_some();
    let snap_missing = if snap {
        ["pipewire", "audio-record"]
            .iter()
            .filter(|plug| !Command::new("snapctl").args(["is-connected", plug]).status().is_ok_and(|s| s.success()))
            .map(|p| p.to_string())
            .collect()
    } else {
        Vec::new()
    };
    Setup { pipewire, conflicts, snap, snap_missing }
}

/// Ko-fi opens in the user's browser, payment never touches Mixpilot.
#[tauri::command]
fn open_support() {
    let _ = Command::new("xdg-open").arg("https://ko-fi.com/simonlinuxcraft").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

const BUG_EMAIL: &str = "simonlinuxcraft@pm.me";
/// The proxy URL, set at build time only (the Web3Forms key stays on the server); without it, mail.
const BUG_PROXY: Option<&str> = option_env!("MIXPILOT_BUG_URL");

fn package() -> &'static str {
    if std::env::var_os("SNAP").is_some() {
        "snap"
    } else if std::env::var_os("FLATPAK_ID").is_some() {
        "flatpak"
    } else if std::env::var_os("APPIMAGE").is_some() {
        "appimage"
    } else {
        "deb/native"
    }
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    package: &'static str,
    email: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo { version: env!("CARGO_PKG_VERSION"), package: package(), email: BUG_EMAIL }
}

/// What a report carries when "Systeminfos anhängen" is on. Nothing personal beyond the device names.
fn system_info() -> String {
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| t.lines().find(|l| l.starts_with("PRETTY_NAME=")).map(|l| l[12..].trim_matches('"').to_string()))
        .unwrap_or_else(|| "unknown".into());
    let kernel = unsafe {
        let mut u: libc::utsname = std::mem::zeroed();
        libc::uname(&mut u);
        std::ffi::CStr::from_ptr(u.release.as_ptr()).to_string_lossy().into_owned()
    };
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| "-".into());
    let package = package();
    let server = output_within(Command::new("pactl").arg("info").env("LC_ALL", "C"), CMD_LIMIT)
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().find(|l| l.contains("Server Name")).map(|l| l.trim().to_string()))
        .unwrap_or_else(|| "pactl info: n/a".into());
    let cfg = get_config().unwrap_or(Value::Null);
    let pick = |p: &str| cfg.pointer(p).map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    format!(
        "Mixpilot: {}\nOS: {os}\nKernel: {kernel}\nDesktop: {} | Session: {} | GDK_BACKEND: {}\nPackage: {package}\nAudio: {server}\nCore: {}\nOutput: {} | Input: {}\nMic: {}\nAutopilot: {} | Auto volume: {}",
        env!("CARGO_PKG_VERSION"),
        env("XDG_CURRENT_DESKTOP"),
        env("XDG_SESSION_TYPE"),
        env("GDK_BACKEND"),
        if core_running() { "running" } else { "not running" },
        pick("/output"),
        pick("/input"),
        pick("/mic"),
        pick("/auto"),
        pick("/auto_volume"),
    )
}

/// The end of the core log, reaching into the previous run when this one has only just started.
fn core_log_tail(lines: usize) -> String {
    let dir = env_dir("XDG_STATE_HOME", ".local/state").join("mixpilot");
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
    let text = read("core.log.1") + &read("core.log");
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

#[derive(Serialize)]
struct BugInfo {
    can_send: bool,
    email: String,
    info: String,
    log: String,
}

// system_info asks pactl, which must never block the main thread (window and tray)
#[tauri::command]
async fn bug_info() -> Result<BugInfo, String> {
    blocking(|| BugInfo { can_send: BUG_PROXY.is_some(), email: BUG_EMAIL.into(), info: system_info(), log: core_log_tail(60) }).await
}

#[derive(serde::Deserialize)]
struct BugReport {
    description: String,
    errors: String,
    email: String,
    include_info: bool,
    include_log: bool,
    // the texts the preview showed, not a fresh read at send time
    info: String,
    log: String,
}

fn report_text(r: &BugReport) -> String {
    let mut t = r.description.trim().to_string();
    if !r.errors.trim().is_empty() {
        t += &format!("\n\n--- Error messages ---\n{}", r.errors.trim());
    }
    if r.include_info {
        t += &format!("\n\n--- System ---\n{}", r.info.trim());
    }
    if r.include_log {
        t += &format!("\n\n--- Core log (last 60 lines) ---\n{}", r.log);
    }
    t += &format!("\n\nUser-Email: {}", if r.email.trim().is_empty() { "(not provided)" } else { r.email.trim() });
    t
}

/// Sends through the proxy with curl (present on every desktop, staged in the snap).
#[tauri::command]
async fn send_bug_report(report: BugReport) -> Result<(), String> {
    blocking(move || post_bug_report(&report)).await?
}

fn post_bug_report(report: &BugReport) -> Result<(), String> {
    if report.description.trim().len() < 5 {
        return Err("Bitte beschreib kurz, was passiert ist.".into());
    }
    let Some(url) = BUG_PROXY else { return Err("no-endpoint".into()) };
    let mut payload = serde_json::json!({
        "subject": format!("Mixpilot Bug Report v{}", env!("CARGO_PKG_VERSION")),
        "from_name": "Mixpilot App",
        "message": report_text(report),
        "botcheck": "",
    });
    if !report.email.trim().is_empty() {
        payload["email"] = Value::String(report.email.trim().into());
    }
    let mut child = Command::new("curl")
        .args(["-sS", "-m", "20", "-X", "POST", "-H", "Content-Type: application/json", "-H", "Accept: application/json", "--data-binary", "@-", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.to_string().as_bytes()).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    if out.status.success() && answer.get("success").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(answer.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| String::from_utf8_lossy(&out.stderr).trim().to_string()))
    }
}

/// Fallback without a configured endpoint: the user's mail program with the report filled in.
#[tauri::command(async)]
fn mail_bug_report(report: BugReport) {
    let enc = |t: &str| t.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    // mail programs cut very long mailto links, so the log is left out here
    let body: String = report_text(&BugReport { include_log: false, ..report }).chars().take(6000).collect();
    let url = format!("mailto:{BUG_EMAIL}?subject={}&body={}", enc(&format!("Mixpilot Bug Report v{}", env!("CARGO_PKG_VERSION"))), enc(&body));
    let _ = Command::new("xdg-open").arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

#[tauri::command]
async fn open_bug_report(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("bugreport") {
        bring_to_front(&w);
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, "bugreport", WebviewUrl::App("bugreport.html".into()))
        .title(tr("Fehler melden", "Report a bug"))
        .initialization_script(lang_script())
        .inner_size(580.0, 680.0)
        .min_inner_size(420.0, 480.0)
        .zoom_hotkeys_enabled(true)
        .decorations(false)
        .background_color(tauri::window::Color(0x11, 0x13, 0x16, 0xff))
        // shown only once the page is loaded: showing it earlier blocks the whole app up to 500 ms
        .visible(false)
        .on_page_load(|w, p| {
            if p.event() == tauri::webview::PageLoadEvent::Finished {
                let _ = w.show();
            }
        })
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Under native Wayland a client cannot raise or un-minimize itself; hiding and showing maps the
/// window again, which the compositor brings up. Same calls are harmless on X11.
fn bring_to_front(w: &tauri::WebviewWindow) {
    let _ = w.hide();
    let _ = w.show();
    let _ = w.set_focus();
}

/// Opens the window, or brings the existing one to the front. The window is created on demand and
/// destroyed on close, so the background app (tray + core) does not keep a web view in memory.
fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        bring_to_front(&w);
        return;
    }
    if let Some(cfg) = app.config().app.windows.first()
        && let Err(e) = WebviewWindowBuilder::from_config(app, cfg).and_then(|b| b.initialization_script(lang_script()).build())
    {
        eprintln!("mixpilot: window: {e}");
    }
}

fn ipc_socket() -> PathBuf {
    runtime_dir().join("mixpilot").join("app.sock")
}

/// A second start (menu entry while the tray app runs) only asks the running one to show its window.
fn hand_over(background: bool) -> bool {
    match UnixStream::connect(ipc_socket()) {
        Ok(mut s) => {
            if !background {
                let _ = s.write_all(b"show");
            }
            true
        }
        Err(_) => false,
    }
}

/// Held for the whole run by the one tray app of the session. None: another start holds it.
fn app_lock(path: &std::path::Path) -> std::io::Result<Option<std::fs::File>> {
    std::fs::create_dir_all(path.parent().unwrap_or(path))?;
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path)?;
    Ok((unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0).then_some(f))
}

fn listen(app: AppHandle) {
    let path = ipc_socket();
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else { return };
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut msg = String::new();
            let _ = stream.read_to_string(&mut msg);
            if msg == "show" {
                let a = app.clone();
                let _ = app.run_on_main_thread(move || show_window(&a));
            }
        }
    });
}

/// Stops the audio core: all apps fall back to their normal device.
/// SIGKILL after 2 s: a core hung in its main loop would otherwise keep its nodes and swallow the audio.
fn stop_core() {
    let Some(pid) = core_pid() else { return };
    for sig in [libc::SIGTERM, libc::SIGKILL] {
        unsafe { libc::kill(pid, sig) };
        for _ in 0..40 {
            if !core_running() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// German for German locales, English for everyone else. The UI gets the same choice.
fn german() -> bool {
    ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|v| std::env::var(v).ok()).find(|v| !v.is_empty()).is_some_and(|v| v.starts_with("de"))
}

fn tr(de: &'static str, en: &'static str) -> &'static str {
    if german() { de } else { en }
}

fn lang_script() -> &'static str {
    if german() { "window.MIXPILOT_LANG = 'de';" } else { "window.MIXPILOT_LANG = 'en';" }
}

/// GNOME without the AppIndicator extension (Fedora, Arch, Debian) has no tray host.
fn tray_available() -> bool {
    let Ok(c) = dbus::blocking::Connection::new_session() else { return true };
    let bus = c.with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", std::time::Duration::from_millis(500));
    bus.method_call("org.freedesktop.DBus", "NameHasOwner", ("org.kde.StatusNotifierWatcher",)).map(|(b,): (bool,)| b).unwrap_or(true)
}

/// Ends Mixpilot completely: the core stops and apps play on their normal device again.
#[tauri::command]
fn quit_app(app: AppHandle) {
    {
        let _start = STARTING.lock().unwrap_or_else(|e| e.into_inner());
        QUITTING.store(true, std::sync::atomic::Ordering::Relaxed);
        stop_core();
    }
    let _ = std::fs::remove_file(ipc_socket());
    app.exit(0);
}

fn set_mic_mute(f: impl FnOnce(bool) -> bool) {
    let _ = update_config(|c| {
        if !c["mic"].is_object() {
            c["mic"] = serde_json::json!({});
        }
        let now = c["mic"]["mute"].as_bool().unwrap_or(false);
        c["mic"]["mute"] = Value::Bool(f(now));
    });
}

fn toggle_mic_mute() {
    set_mic_mute(|on| !on);
}

/// The window's way out of a mute when there is neither a tray nor a push-to-mute key.
#[tauri::command]
fn unmute_mic() {
    set_mic_mute(|_| false);
}

#[tauri::command]
fn hotkey_status() -> hotkey::Status {
    hotkey::status()
}

#[tauri::command]
async fn set_hotkey(on: bool) -> Result<hotkey::Status, String> {
    blocking(move || hotkey::send(if on { hotkey::Cmd::On } else { hotkey::Cmd::Off })).await?
}

#[tauri::command]
async fn change_hotkey() -> Result<hotkey::Status, String> {
    blocking(|| hotkey::send(hotkey::Cmd::Change)).await?
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let cfg = get_config().unwrap_or(Value::Null);
    let flag = |ptr: &str, default: bool| cfg.pointer(ptr).and_then(Value::as_bool).unwrap_or(default);
    let open = MenuItem::with_id(app, "open", tr("Mixpilot öffnen", "Open Mixpilot"), true, None::<&str>)?;
    let bug = MenuItem::with_id(app, "bug", tr("Fehler melden", "Report a bug"), true, None::<&str>)?;
    let auto = CheckMenuItem::with_id(app, "auto", "Autopilot", true, flag("/auto", true), None::<&str>)?;
    let mute = CheckMenuItem::with_id(app, "mute", tr("Mikrofon stumm", "Mute microphone"), true, flag("/mic/mute", false), None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr("Beenden", "Quit"), true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &PredefinedMenuItem::separator(app)?, &auto, &mute, &PredefinedMenuItem::separator(app)?, &bug, &quit],
    )?;
    let (auto2, mute2) = (auto.clone(), mute.clone());
    // simplified logo: the full app icon blurs at 22 px
    let tray_icon = tauri::include_image!("icons/tray.png");
    let tray = TrayIconBuilder::with_id("mixpilot").tooltip("Mixpilot").icon(tray_icon.clone()).menu(&menu).show_menu_on_left_click(true);
    let tray = tray.on_menu_event(move |app, event| match event.id().as_ref() {
        "open" => show_window(app),
        "bug" => {
            let a = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = open_bug_report(a).await;
            });
        }
        "auto" => {
            let on = auto2.is_checked().unwrap_or(true);
            let _ = update_config(|c| c["auto"] = Value::Bool(on));
        }
        "mute" => set_mic_mute(|_| mute2.is_checked().unwrap_or(false)),
        "quit" => quit_app(app.clone()),
        _ => {}
    })
    .build(app)?;

    // keep the check marks and the muted icon in step with changes made in the window
    let normal = Some(tray_icon);
    let muted_icon = tauri::include_image!("icons/tray-muted.png");
    let mut shown = (true, false);
    std::thread::spawn(move || {
        loop {
            if let Ok(c) = get_config() {
                let now = (c.pointer("/auto").and_then(Value::as_bool).unwrap_or(true), c.pointer("/mic/mute").and_then(Value::as_bool).unwrap_or(false));
                if now != shown {
                    let _ = auto.set_checked(now.0);
                    let _ = mute.set_checked(now.1);
                    if now.1 != shown.1 {
                        let _ = tray.set_icon(if now.1 { Some(muted_icon.clone()) } else { normal.clone() });
                    }
                    shown = now;
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
    Ok(())
}

fn main() {
    let background = std::env::args().any(|a| a == "--background");
    // The socket answers only once the window exists. A start before that (autostart plus a click at
    // login) waits for it instead of becoming a second tray app with its own core watchdog.
    let mut waited = 0;
    let _lock = loop {
        if hand_over(background) {
            return;
        }
        match app_lock(&runtime_dir().join("mixpilot").join("app.lock")) {
            Ok(Some(f)) => break Some(f),
            Ok(None) if waited < 100 => {
                waited += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => return,
            Err(e) => {
                eprintln!("mixpilot: app lock: {e}");
                break None;
            }
        }
    };
    if let Err(e) = start_core() {
        eprintln!("mixpilot: {e}");
    }
    watch_core();
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            core_status,
            ensure_core,
            get_config,
            set_config,
            list_devices,
            set_default_source,
            get_state,
            get_autostart,
            set_autostart,
            get_master,
            set_master,
            set_master_mute,
            open_support,
            open_bug_report,
            bug_info,
            send_bug_report,
            mail_bug_report,
            setup_check,
            app_info,
            hotkey_status,
            unmute_mic,
            mic_test_record,
            mic_test_play,
            set_hotkey,
            change_hotkey,
            quit_app
        ])
        .on_window_event(|w, e| {
            // without a tray there is no way back to a hidden window: minimize instead
            if let tauri::WindowEvent::CloseRequested { api, .. } = e
                && w.label() == "main"
                && !tray_available()
            {
                api.prevent_close();
                let _ = w.minimize();
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            if let Err(e) = build_tray(&handle) {
                // no tray (desktop without AppIndicator support): the window stays the way back in
                eprintln!("mixpilot: tray: {e}");
            }
            listen(handle.clone());
            let restore = get_config().ok().and_then(|c| c["mic_hotkey"].as_bool()).unwrap_or(false);
            hotkey::start(restore, toggle_mic_mute);
            if !background {
                show_window(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("mixpilot: {e}");
            std::process::exit(1);
        });
    app.run(|_, event| {
        // closing the window keeps Mixpilot in the tray; only "Beenden" (exit with a code) ends it
        if let RunEvent::ExitRequested { code: None, api, .. } = event {
            api.prevent_exit();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_respects_xdg() {
        // SAFETY: single-threaded test
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/x") };
        assert_eq!(config_path(), std::path::Path::new("/x/mixpilot/config.json"));
    }

    #[test]
    fn one_app_holds_the_lock() {
        let path = std::env::temp_dir().join(format!("mixpilot-app-lock-test-{}", std::process::id())).join("app.lock");
        let first = app_lock(&path).unwrap();
        assert!(first.is_some());
        assert!(app_lock(&path).unwrap().is_none());
        drop(first);
        assert!(app_lock(&path).unwrap().is_some());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn dev_core_only_from_the_source_tree() {
        assert!(dev_core(std::path::Path::new("/home/u/mixpilot/app/target/release")).is_some());
        assert!(dev_core(std::path::Path::new("/tmp/.mount_MixpiXy/usr/bin")).is_none());
        assert!(dev_core(std::path::Path::new("/usr/bin")).is_none());
    }

    #[test]
    fn report_sends_what_the_preview_showed() {
        let r = BugReport {
            description: "it broke".into(),
            errors: String::new(),
            email: String::new(),
            include_info: true,
            include_log: false,
            info: "OS: as shown".into(),
            log: "core: as shown".into(),
        };
        let t = report_text(&r);
        assert!(t.contains("OS: as shown") && !t.contains("core: as shown"));
    }

    #[test]
    fn commands_end_within_the_limit() {
        let t = Instant::now();
        let e = output_within(Command::new("sleep").arg("5"), Duration::from_millis(200)).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
        assert!(t.elapsed() < Duration::from_secs(1));
        let out = output_within(Command::new("echo").arg("hi"), CMD_LIMIT).unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "hi\n");
    }

    #[test]
    fn mute_and_reset_note_come_from_disk() {
        let disk = serde_json::json!({ "mic": { "mute": true }, "config_reset": "/c/config.json.broken-1" });
        let mut cfg = serde_json::json!({ "mic": { "mute": false } });
        keep_from_disk(&mut cfg, Some(&disk));
        assert_eq!(cfg, disk);
        // shown: the window sends null and the note goes
        let mut cfg = serde_json::json!({ "mic": { "mute": true }, "config_reset": null });
        keep_from_disk(&mut cfg, Some(&disk));
        assert_eq!(cfg, serde_json::json!({ "mic": { "mute": true } }));
        // unreadable file on disk: the window's copy is written as it is
        let mut cfg = serde_json::json!({ "mic": { "mute": false } });
        keep_from_disk(&mut cfg, None);
        assert_eq!(cfg, serde_json::json!({ "mic": { "mute": false } }));
    }
}
