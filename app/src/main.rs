use serde::Serialize;
use serde_json::Value;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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

fn core_binary() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    [
        std::env::var_os("MIXPILOT_CORE").map(PathBuf::from),
        Some(exe_dir.join("mixpilot-core")),
        // development tree first: app/target/release/mixpilot -> core/target/release/mixpilot-core.
        // An installed core of another version would be replaced on every window open.
        Some(exe_dir.join("../../../core/target/release/mixpilot-core")),
        Some(PathBuf::from("/usr/lib/mixpilot/mixpilot-core")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}

/// Must match STATE_PROTO in the core.
const STATE_PROTO: u64 = 4;

/// A current core rewrites state.json every 50 ms and reports the protocol this app speaks.
fn state_fresh() -> bool {
    let path = runtime_dir().join("mixpilot").join("state.json");
    let recent = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|d| d.as_millis() < 2000));
    recent
        && std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("proto").and_then(Value::as_u64))
            == Some(STATE_PROTO)
}

/// A core that holds the lock but writes no current state is an older version (after an update) or hung.
/// It is stopped by PID so a current one can take over.
fn replace_stale_core() {
    // Once per app run at most: if the only core binary around speaks another protocol, replacing it
    // again on every window open would cut the audio each time.
    static REPLACED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !core_running() || REPLACED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    for _ in 0..30 {
        if state_fresh() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    REPLACED.store(true, std::sync::atomic::Ordering::Relaxed);
    eprintln!("mixpilot: replacing a core without current state");
    stop_core();
}

/// True if the process was started with the given XDG_RUNTIME_DIR, i.e. belongs to that session.
fn same_runtime(pid: i32, dir: &std::path::Path) -> bool {
    let Ok(env) = std::fs::read(format!("/proc/{pid}/environ")) else { return false };
    let want = format!("XDG_RUNTIME_DIR={}", dir.display());
    env.split(|b| *b == 0).any(|kv| kv == want.as_bytes())
}

/// Cores of this user that belong to this session. A core from another session (a test instance,
/// a second login) is never touched.
fn core_pids() -> Vec<i32> {
    let dir = runtime_dir();
    let is_core = |pid: &i32| std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "mixpilot-core");
    // A current core writes its PID into this session's lock. The pgrep route stays for older cores,
    // it fails inside the snap, which may not read /proc/<pid>/environ.
    let locked = std::fs::read_to_string(dir.join("mixpilot.lock")).ok().and_then(|s| s.trim().parse().ok()).filter(|p| core_running() && is_core(p));
    let uid = unsafe { libc::getuid() }.to_string();
    let out = Command::new("pgrep").args(["-u", &uid, "-x", "mixpilot-core"]).output().map(|o| o.stdout).unwrap_or_default();
    let mut pids: Vec<i32> = String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .filter(|&pid| same_runtime(pid, &dir))
        .chain(locked)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// Starts the audio core detached, so it keeps running after the window is closed.
fn start_core() -> Result<(), String> {
    // ensure_core runs off the main thread now; two starts at once would race for the log and the lock
    static STARTING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _start = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    replace_stale_core();
    if core_running() {
        return Ok(());
    }
    let bin = core_binary().ok_or("Audio-Kern nicht gefunden")?;
    let log_dir = env_dir("XDG_STATE_HOME", ".local/state").join("mixpilot");
    std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    // keep the previous run, it holds the last words of a crashed core
    let _ = std::fs::rename(log_dir.join("core.log"), log_dir.join("core.log.1"));
    let log = std::fs::File::create(log_dir.join("core.log")).map_err(|e| e.to_string())?;
    let mut child = Command::new(bin)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    // reap it if it exits while the window is open, so no zombie is left
    std::thread::spawn(move || child.wait());
    Ok(())
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
    if let Some(mute) = get_config().ok().and_then(|c| c.pointer("/mic/mute").cloned()) {
        if cfg["mic"].is_object() {
            cfg["mic"]["mute"] = mute;
        }
    }
    write_config(&cfg)
}

/// Write to a temp file and rename, so the core never reads a half-written file.
fn write_config(cfg: &Value) -> Result<(), String> {
    let path = config_path();
    let tmp = path.with_extension("json.tmp");
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
    let ok = Command::new("pw-metadata")
        .args(["-n", "default", "0", "default.configured.audio.source", &value, "Spa:String:JSON"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok { Ok(()) } else { Err("Standard-Mikrofon konnte nicht gesetzt werden".into()) }
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// Live data from the core: meters, apps, events. `mic_test` keeps the microphone open while the
/// microphone page is visible (the core closes it 3 s after the last touch).
#[tauri::command]
fn get_state(mic_test: bool) -> Result<Value, String> {
    // a crashed core leaves its last state behind; never show frozen meters
    if !core_running() {
        return Err("core not running".into());
    }
    let dir = runtime_dir().join("mixpilot");
    if mic_test {
        let _ = std::fs::write(dir.join("mic-test"), b"");
    }
    let text = std::fs::read(dir.join("state.json")).map_err(|e| e.to_string())?;
    serde_json::from_slice(&text).map_err(|e| e.to_string())
}

fn mic_test_file() -> PathBuf {
    runtime_dir().join("mixpilot").join("mic-test.wav")
}

/// Five seconds of the processed voice, the same signal Discord and OBS get.
#[tauri::command(async)]
fn mic_test_record() -> Result<(), String> {
    let file = mic_test_file();
    let mut rec = Command::new("pw-record")
        .args(["--target", "mixpilot_mic", "--rate", "48000", "--channels", "1"])
        .arg(&file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("pw-record: {e}"))?;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Ok(Some(_)) = rec.try_wait() {
            return Err("Aufnahme fehlgeschlagen".into());
        }
    }
    // SIGINT lets pw-record finish the file header
    unsafe { libc::kill(rec.id() as i32, libc::SIGINT) };
    let _ = rec.wait();
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > 44) { Ok(()) } else { Err("Aufnahme fehlgeschlagen".into()) }
}

#[tauri::command(async)]
fn mic_test_play() -> Result<(), String> {
    let file = mic_test_file();
    let played = Command::new("pw-play").arg(&file).stdout(Stdio::null()).stderr(Stdio::null()).status();
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
    // earlier builds started only the core from here
    let _ = std::fs::remove_file(autostart_dir().join("mixpilot-core.desktop"));
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

/// Volume commands run one after another, never in parallel.
static PACTL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn pactl(args: &[&str]) -> Result<String, String> {
    let _serial = PACTL.lock().unwrap_or_else(|e| e.into_inner());
    let out = Command::new("pactl").args(args).env("LC_ALL", "C").output().map_err(|e| format!("pactl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[derive(Serialize)]
struct Master {
    volume: u32,
    mute: bool,
}

/// Master is the volume of the output device itself, the same one GNOME and the volume keys change.
#[tauri::command(async)]
fn get_master() -> Result<Master, String> {
    let sink = output_sink();
    let vol = pactl(&["get-sink-volume", &sink])?;
    let volume = vol
        .split('/')
        .nth(1)
        .and_then(|p| p.trim().trim_end_matches('%').trim().parse().ok())
        .ok_or("unknown volume format")?;
    let mute = pactl(&["get-sink-mute", &sink])?.contains("yes");
    Ok(Master { volume, mute })
}

#[tauri::command(async)]
fn set_master(volume: u32) -> Result<(), String> {
    pactl(&["set-sink-volume", &output_sink(), &format!("{}%", volume.min(150))])
        .map(|_| ())
        .map_err(|e| { eprintln!("mixpilot: pactl: {e}"); "Lautstärke des Ausgabegeräts konnte nicht geändert werden".to_string() })
}

#[tauri::command(async)]
fn set_master_mute(mute: bool) -> Result<(), String> {
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
    let pipewire = Command::new("pw-cli").arg("info").arg("0").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
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
/// Set at build time only, never in the source: a proxy URL (key stays on the server) or a Web3Forms key.
const BUG_PROXY: Option<&str> = option_env!("MIXPILOT_BUG_URL");
const BUG_KEY: Option<&str> = option_env!("MIXPILOT_WEB3FORMS_KEY");

fn first_line_with(cmd: &str, args: &[&str], needle: &str) -> Option<String> {
    let out = Command::new(cmd).args(args).env("LC_ALL", "C").output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find(|l| l.contains(needle)).map(|l| l.trim().to_string())
}

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
    let server = first_line_with("pactl", &["info"], "Server Name").unwrap_or_else(|| "pactl info: n/a".into());
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

#[tauri::command]
fn bug_info() -> BugInfo {
    BugInfo { can_send: BUG_PROXY.is_some() || BUG_KEY.is_some(), email: BUG_EMAIL.into(), info: system_info(), log: core_log_tail(60) }
}

#[derive(serde::Deserialize)]
struct BugReport {
    description: String,
    errors: String,
    email: String,
    include_info: bool,
    include_log: bool,
}

fn report_text(r: &BugReport) -> String {
    let mut t = r.description.trim().to_string();
    if !r.errors.trim().is_empty() {
        t += &format!("\n\n--- Error messages ---\n{}", r.errors.trim());
    }
    if r.include_info {
        t += &format!("\n\n--- System ---\n{}", system_info());
    }
    if r.include_log {
        t += &format!("\n\n--- Core log (last 60 lines) ---\n{}", core_log_tail(60));
    }
    t += &format!("\n\nUser-Email: {}", if r.email.trim().is_empty() { "(not provided)" } else { r.email.trim() });
    t
}

/// Sends through the proxy or Web3Forms with curl (present on every desktop, staged in the snap).
#[tauri::command]
async fn send_bug_report(report: BugReport) -> Result<(), String> {
    if report.description.trim().len() < 5 {
        return Err("Bitte beschreib kurz, was passiert ist.".into());
    }
    let url = BUG_PROXY.unwrap_or("https://api.web3forms.com/submit");
    if BUG_PROXY.is_none() && BUG_KEY.is_none() {
        return Err("no-endpoint".into());
    }
    let mut payload = serde_json::json!({
        "subject": format!("Mixpilot Bug Report v{}", env!("CARGO_PKG_VERSION")),
        "from_name": "Mixpilot App",
        "message": report_text(&report),
        "botcheck": "",
    });
    if let (None, Some(key)) = (BUG_PROXY, BUG_KEY) {
        payload["access_key"] = Value::String(key.into());
    }
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
#[tauri::command]
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
    if let Some(cfg) = app.config().app.windows.first() {
        if let Err(e) = WebviewWindowBuilder::from_config(app, cfg).and_then(|b| b.initialization_script(lang_script()).build()) {
            eprintln!("mixpilot: window: {e}");
        }
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
    let pids = core_pids();
    if pids.is_empty() {
        return;
    }
    for sig in [libc::SIGTERM, libc::SIGKILL] {
        for &pid in &pids {
            unsafe { libc::kill(pid, sig) };
        }
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
    stop_core();
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

#[tauri::command(async)]
fn set_hotkey(on: bool) -> Result<hotkey::Status, String> {
    hotkey::send(if on { hotkey::Cmd::On } else { hotkey::Cmd::Off })
}

#[tauri::command(async)]
fn change_hotkey() -> Result<hotkey::Status, String> {
    hotkey::send(hotkey::Cmd::Change)
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
    if hand_over(background) {
        return;
    }
    if let Err(e) = start_core() {
        eprintln!("mixpilot: {e}");
    }
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
            if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                if w.label() == "main" && !tray_available() {
                    api.prevent_close();
                    let _ = w.minimize();
                }
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
            // an older autostart entry that only started the core is replaced by the tray app
            if autostart_dir().join("mixpilot-core.desktop").is_file() {
                let _ = set_autostart(true);
            }
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
    fn only_processes_of_the_same_session_match() {
        let mut child = Command::new("sleep").arg("5").env("XDG_RUNTIME_DIR", "/tmp/mixpilot-session-a").spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let pid = child.id() as i32;
        let a = same_runtime(pid, std::path::Path::new("/tmp/mixpilot-session-a"));
        let b = same_runtime(pid, std::path::Path::new("/run/user/1000"));
        let _ = child.kill();
        let _ = child.wait();
        assert!(a && !b);
    }

    #[test]
    fn config_path_respects_xdg() {
        // SAFETY: single-threaded test
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/x") };
        assert_eq!(config_path(), std::path::Path::new("/x/mixpilot/config.json"));
    }
}
