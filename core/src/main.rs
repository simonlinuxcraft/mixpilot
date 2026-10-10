/// eprintln! panics when stderr is a closed pipe, and a panic inside a PipeWire callback aborts.
#[macro_export]
macro_rules! log {
    ($($t:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), "mixpilot: {}", format_args!($($t)*));
    }};
}

mod config;
mod dsp;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Cursor, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use nnnoiseless::DenoiseState;
use pipewire as pw;
use pw::metadata::{Metadata, MetadataListener};
use pw::node::{Node, NodeListener};
use pw::properties::properties;
use pw::spa;
use pw::stream::{StreamBox, StreamFlags, StreamRc, StreamState};
use pw::types::ObjectType;
use spa::pod::{serialize::PodSerializer, Object, Pod, Value};

use config::{index_of, Config, CHANNELS};
use dsp::{run5, sanitize, Agc, Biquad, Chain5, DeEsser, Envelope, Eq10, Gate, Leveler, Limiter, Smooth};

/// ~100 ms of stereo audio. Anything older is dropped so latency cannot creep up.
const RING_MAX: usize = 48_000 / 10 * 2;
/// ~100 ms of mono microphone audio.
const MIC_RING_MAX: usize = 48_000 / 10;
const BASS_FREQ: f32 = 100.0;
const FRAME: usize = DenoiseState::FRAME_SIZE;
/// main loop tick; config polling, level snapshots and reconnects all run on it
const TICK_MS: u64 = 50;
const RETRY_TICKS: u32 = 40;
/// Bumped whenever the app needs something new from the core; must match STATE_PROTO in the app.
const STATE_PROTO: u32 = 5;
/// mix channels, see config::CHANNELS
const N: usize = CHANNELS.len();

/// Values the control side writes and the realtime threads read. f32 stored as bits.
#[derive(Default)]
struct Params {
    gain: [AtomicU32; N],
    /// per channel the 10 band gains of its preset
    ch_eq: [[AtomicU32; 10]; N],
    master: AtomicU32,
    bass_db: AtomicU32,
    limiter: AtomicBool,
    clarity: AtomicBool,
    auto_volume: AtomicU32,
    ducking: AtomicBool,
    mic_gain: AtomicU32,
    mic_noise: AtomicU32,
    mic_agc: AtomicBool,
    mic_gate: AtomicBool,
    mic_voice: AtomicU32,
    mic_deess: AtomicU32,
    monitor: AtomicBool,
}

fn put(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Relaxed);
}

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Relaxed))
}

impl Params {
    fn apply(&self, cfg: &Config, night: bool) {
        let (game, chat) = dsp::chatmix(cfg.chatmix);
        for (i, ch) in cfg.channels().iter().enumerate() {
            let mix = match i {
                0 => game,
                1 => chat,
                _ => 1.0,
            };
            put(&self.gain[i], if ch.mute { 0.0 } else { dsp::fader_to_gain(ch.volume) * mix });
            for (a, g) in self.ch_eq[i].iter().zip(cfg.curve(&ch.eq)) {
                put(a, if g.is_finite() { g.clamp(-12.0, 12.0) } else { 0.0 });
            }
        }
        put(&self.master, dsp::fader_to_gain(cfg.master));
        put(&self.bass_db, if night { 0.0 } else { cfg.bass.clamp(0.0, 100.0) * 0.12 });
        self.limiter.store(cfg.limiter, Relaxed);
        self.clarity.store(cfg.clarity, Relaxed);
        let av = if night { 2 } else { index_of(&["off", "soft", "night"], &cfg.auto_volume) };
        self.auto_volume.store(av, Relaxed);
        self.ducking.store(cfg.ducking && cfg.auto, Relaxed);
        put(&self.mic_gain, if cfg.mic.mute { 0.0 } else { dsp::mic_gain(cfg.mic.gain) });
        self.mic_noise.store(index_of(&["off", "normal", "strong"], &cfg.mic.noise), Relaxed);
        self.mic_agc.store(cfg.mic.agc, Relaxed);
        self.mic_gate.store(cfg.mic.gate, Relaxed);
        self.mic_voice.store(index_of(&dsp::VOICE_PRESETS, &cfg.mic.voice), Relaxed);
        self.mic_deess.store(index_of(&["off", "normal", "strong"], &cfg.mic.deess), Relaxed);
        self.monitor.store(cfg.mic.monitor, Relaxed);
    }
}

/// Peak meters: channels L/R, then master L/R, microphone processed, microphone input.
/// The realtime side keeps the maximum, the main loop takes and resets it on every tick.
const LV_MASTER: usize = 2 * N;
const LV_MIC: usize = 2 * N + 2;
const LV_MIC_IN: usize = 2 * N + 3;
#[derive(Default)]
struct Levels([AtomicU32; 2 * N + 4]);

impl Levels {
    fn raise(&self, i: usize, v: f32) {
        if v.is_finite() {
            // non-negative f32 bit patterns order like the values, so fetch_max on bits works
            self.0[i].fetch_max(v.abs().to_bits(), Relaxed);
        }
    }
    fn take(&self) -> [f32; 2 * N + 4] {
        std::array::from_fn(|i| f32::from_bits(self.0[i].swap(0, Relaxed)))
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn format_pod(channels: u32) -> Vec<u8> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(dsp::RATE as u32);
    info.set_channels(channels);
    let mut pos = [0; spa::param::audio::MAX_CHANNELS];
    if channels == 1 {
        pos[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
    } else {
        pos[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
        pos[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    }
    info.set_position(pos);
    PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &Value::Object(Object {
            type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: spa::param::ParamType::EnumFormat.as_raw(),
            properties: info.into(),
        }),
    )
    .expect("format pod")
    .0
    .into_inner()
}

/// Output mix state, owned by the output stream's realtime callback.
struct Mix {
    gains: [Smooth; N],
    master: Smooth,
    duck: Smooth,
    duck_env: Envelope,
    ch_eq: [[Eq10; 2]; N],
    bass: [Biquad; 2],
    bass_db: f32,
    clarity: [Biquad; 2],
    leveler: Option<Leveler>,
    lev_mode: u32,
    limiter: Limiter,
}

impl Mix {
    fn new() -> Self {
        Self {
            gains: [Smooth::new(0.0, 20.0); N],
            master: Smooth::new(0.0, 20.0),
            duck: Smooth::new(1.0, 200.0),
            duck_env: Envelope::new(400.0),
            ch_eq: Default::default(),
            bass: [Biquad::low_shelf(BASS_FREQ, 0.0); 2],
            bass_db: 0.0,
            clarity: [Biquad::peaking(3200.0, 3.0, 0.9); 2],
            leveler: None,
            lev_mode: 0,
            limiter: Limiter::new(-1.0, 120.0),
        }
    }

    /// Pick up parameter changes once per buffer; coefficients only change when a value did.
    fn update(&mut self, p: &Params) {
        for c in 0..N {
            let gains: [f32; 10] = std::array::from_fn(|i| load(&p.ch_eq[c][i]));
            for e in &mut self.ch_eq[c] {
                e.set(&gains);
            }
        }
        let db = load(&p.bass_db);
        if db != self.bass_db {
            self.bass_db = db;
            for b in &mut self.bass {
                b.retune(Biquad::low_shelf(BASS_FREQ, db));
            }
        }
        let mode = p.auto_volume.load(Relaxed);
        if mode != self.lev_mode {
            self.lev_mode = mode;
            self.leveler = Leveler::output_mode(mode);
        }
    }
}

/// Microphone chain, owned by the capture stream's realtime callback:
/// gain, RNNoise in 10 ms frames, voice preset, gate, auto level, broadcast compression, limiter.
struct MicDsp {
    denoise: Box<DenoiseState<'static>>,
    inbuf: [f32; FRAME],
    filled: usize,
    frame_out: [f32; FRAME],
    voice: Chain5,
    voice_idx: u32,
    gate: Gate,
    agc: Agc,
    comp: Leveler,
    deess: DeEsser,
    deess_idx: u32,
    limiter: Limiter,
    gain: Smooth,
    vad: Smooth,
}

impl MicDsp {
    fn new() -> Self {
        Self {
            denoise: DenoiseState::new(),
            inbuf: [0.0; FRAME],
            filled: 0,
            frame_out: [0.0; FRAME],
            voice: dsp::voice_preset(0),
            voice_idx: 0,
            gate: Gate::new(),
            agc: Agc::voice(),
            comp: Leveler::broadcast(),
            deess: DeEsser::new(1),
            deess_idx: 1,
            // -3 dBFS leaves headroom for the codecs of voice chat apps
            limiter: Limiter::new(-3.0, 80.0),
            gain: Smooth::new(1.0, 20.0),
            vad: Smooth::new(1.0, 60.0),
        }
    }

    /// Returns (processed peak, input peak after gain).
    fn process(&mut self, input: &[f32], p: &Params, out: &mut VecDeque<f32>, mon: Option<&mut VecDeque<f32>>) -> (f32, f32) {
        let v = p.mic_voice.load(Relaxed);
        if v != self.voice_idx {
            self.voice_idx = v;
            self.voice = dsp::voice_preset(v);
        }
        let d = p.mic_deess.load(Relaxed);
        if d != self.deess_idx {
            self.deess_idx = d;
            self.deess = DeEsser::new(d);
        }
        let (target, noise) = (load(&p.mic_gain), p.mic_noise.load(Relaxed));
        let (gate_on, agc_on) = (p.mic_gate.load(Relaxed), p.mic_agc.load(Relaxed));
        let mut mon = mon;
        let (mut peak, mut in_peak) = (0f32, 0f32);
        for &x in input {
            let x = sanitize(x) * self.gain.next(target);
            in_peak = in_peak.max(x.abs());
            self.inbuf[self.filled] = x;
            self.filled += 1;
            if self.filled < FRAME {
                continue;
            }
            self.filled = 0;
            // RNNoise always runs while the microphone is open, so switching modes is instant and
            // never starts from a stale internal state. It works on 16-bit scale.
            let mut scaled = self.inbuf;
            for s in &mut scaled {
                *s *= 32768.0;
            }
            let mut vad = self.denoise.process_frame(&mut self.frame_out, &scaled);
            if noise > 0 {
                for s in &mut self.frame_out {
                    *s /= 32768.0;
                }
            } else {
                self.frame_out = self.inbuf;
                vad = 1.0;
            }
            // "strong" also ducks everything RNNoise does not classify as voice
            let vad_target = if noise == 2 && vad < 0.6 { 0.1 } else { 1.0 };
            let broadcast = self.voice_idx == 3;
            for i in 0..FRAME {
                let mut y = run5(&mut self.voice, sanitize(self.frame_out[i]));
                if gate_on {
                    // with RNNoise running its voice detection is the better judge than loudness
                    y = if noise > 0 { self.gate.run_voice(y, vad >= 0.5) } else { self.gate.run(y) };
                }
                y *= self.vad.next(vad_target);
                if agc_on {
                    y *= self.agc.gain(y);
                }
                if broadcast {
                    y *= self.comp.gain(y);
                }
                // last before the limiter: compression and presence boosts push S sounds forward
                if d > 0 {
                    y = self.deess.run(y);
                }
                let (y, _) = self.limiter.run(y, y);
                peak = peak.max(y.abs());
                if out.len() >= MIC_RING_MAX {
                    out.pop_front();
                }
                out.push_back(y);
                if let Some(m) = mon.as_deref_mut() {
                    if m.len() >= MIC_RING_MAX {
                        m.pop_front();
                    }
                    m.push_back(y);
                }
            }
        }
        (peak, in_peak)
    }
}

/// Holds a ring at a small, steady latency: waits until `need` samples are buffered before playing
/// (so uneven 480-sample RNNoise blocks never run dry) and drops the oldest if it grows past `max`.
struct Prime(bool);

impl Prime {
    fn ready(&mut self, ring: &mut VecDeque<f32>, frames: usize, need: usize, max: usize) -> bool {
        if ring.len() > max {
            let n = ring.len() - need;
            ring.drain(..n);
        }
        if !self.0 && ring.len() >= need {
            self.0 = true;
        }
        if self.0 && ring.len() < frames {
            self.0 = false;
        }
        self.0
    }
}

/// target.object entries we own in WirePlumber's "default" metadata. Kept here so they can be
/// written again when WirePlumber restarts and its metadata comes back empty.
#[derive(Default)]
struct Router {
    // listener first so it is dropped before its proxy
    md: Option<(u32, MetadataListener, Metadata)>,
    targets: HashMap<u32, String>,
}

impl Router {
    fn set(&mut self, id: u32, target: &str) {
        if let Some((_, _, m)) = &self.md {
            m.set_property(id, "target.object", None, Some(target));
        }
        self.targets.insert(id, target.to_string());
    }
    fn clear(&mut self, id: u32) {
        if let Some((_, _, m)) = &self.md {
            m.set_property(id, "target.object", None, None);
        }
        self.targets.remove(&id);
    }
}

/// Audio sinks and sources as the registry reports them: the app's device pickers, the microphone
/// target, and a second running Mixpilot (its channels show up next to ours).
#[derive(serde::Serialize)]
struct Device {
    name: String,
    description: String,
    #[serde(skip)]
    sink: bool,
}

struct App {
    name: String,
    binary: String,
    node: String,
    channel: Option<usize>,
}

impl App {
    fn keys(&self) -> [Option<&str>; 3] {
        [Some(&self.binary), Some(&self.name), Some(&self.node)]
    }
    fn label(&self) -> &str {
        if !self.name.is_empty() { &self.name } else if !self.binary.is_empty() { &self.binary } else { &self.node }
    }
}

/// (time, key, args); the app turns key and args into a sentence in the user's language
type Events = Rc<RefCell<VecDeque<(String, &'static str, Vec<String>)>>>;

fn local_time() -> (u32, u32) {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        (tm.tm_hour as u32, tm.tm_min as u32)
    }
}

/// Device names other apps show (Discord's microphone list) follow the system language.
fn german() -> bool {
    ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|v| std::env::var(v).ok()).find(|v| !v.is_empty()).is_some_and(|v| v.starts_with("de"))
}

fn event(events: &Events, key: &'static str, args: &[&str]) {
    log!("{key} {args:?}");
    let (h, m) = local_time();
    let mut e = events.borrow_mut();
    if e.len() >= 8 {
        e.pop_front();
    }
    e.push_back((format!("{h:02}:{m:02}"), key, args.iter().map(|a| a.to_string()).collect()));
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// A second instance would create duplicate mixpilot_* nodes. The lock dies with the process.
/// Holds our PID for the app, which may not read /proc/<pid>/environ inside the snap.
fn single_instance(path: &Path) -> Option<std::fs::File> {
    // no truncate before the lock, a refused second instance must not wipe the running PID
    let mut file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
    // the app probes the lock with a short shared lock many times a second; only a lock still held
    // after 200 ms means another core
    let mut tries = 0;
    while unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        tries += 1;
        if tries == 20 {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = file.set_len(0).and_then(|_| write!(file, "{}", std::process::id()));
    Some(file)
}

fn fresh(path: &Path, secs: u64) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).is_ok_and(|d| d.as_secs() < secs))
}

/// Atomic replace so the app never reads half a file.
fn levels_json(l: &[f32; 2 * N + 4], mic_on: bool) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (i, ch) in CHANNELS.iter().enumerate() {
        m.insert(ch.id.into(), serde_json::json!([l[2 * i], l[2 * i + 1]]));
    }
    m.insert("master".into(), serde_json::json!([l[LV_MASTER], l[LV_MASTER + 1]]));
    m.insert("mic".into(), serde_json::json!(if mic_on { l[LV_MIC] } else { -1.0 }));
    m.insert("mic_in".into(), serde_json::json!(if mic_on { l[LV_MIC_IN] } else { -1.0 }));
    serde_json::Value::Object(m)
}

fn write_atomic(path: &Path, data: &[u8]) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, data).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(_lock) = single_instance(&runtime_dir().join("mixpilot.lock")) else {
        log!("already running");
        std::process::exit(3);
    };
    // Block INT/TERM before PipeWire spawns its threads. Otherwise the kernel may deliver the
    // signal to a data thread, whose default action kills the process without cleanup.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let path = config::path();
    let state_dir = runtime_dir().join("mixpilot");
    let _ = std::fs::create_dir_all(&state_dir);
    let cfg = Rc::new(RefCell::new(config::load_or_create(&path)));
    let night = Rc::new(Cell::new(cfg.borrow().auto && cfg.borrow().night.active_at(local_time().0)));
    let params = Arc::new(Params::default());
    params.apply(&cfg.borrow(), night.get());
    let levels = Arc::new(Levels::default());
    let events: Events = Rc::new(RefCell::new(VecDeque::new()));
    log!("config {}", path.display());

    // Set when the server connection or a channel fails: exit non-zero so the supervisor
    // (snap/systemd user service) starts a fresh instance instead of hanging silently.
    let failed = Rc::new(Cell::new(false));
    let fail = {
        let (failed, ml) = (failed.clone(), mainloop.downgrade());
        Rc::new(move |why: String| {
            log!("{why}");
            failed.set(true);
            if let Some(ml) = ml.upgrade() {
                ml.quit();
            }
        })
    };

    // All mix nodes share one driver (node.group) so the rings neither starve nor overflow,
    // and every node shares one link group so WirePlumber never links Mixpilot into itself.
    let rings: Arc<[Mutex<VecDeque<f32>>; N]> =
        Arc::new(std::array::from_fn(|_| Mutex::new(VecDeque::with_capacity(RING_MAX + 2))));
    let mic_out: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::with_capacity(MIC_RING_MAX + 1)));
    let mic_mon: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::with_capacity(MIC_RING_MAX + 1)));
    let stereo = format_pod(2);
    let mono = format_pod(1);
    let mut streams = Vec::new();
    let mut listeners = Vec::new();

    for (i, ch) in CHANNELS.iter().enumerate() {
        let stream = StreamBox::new(
            &core,
            &format!("mixpilot-{}", ch.id),
            properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CLASS => "Audio/Sink",
                *pw::keys::NODE_NAME => ch.node_name(),
                // the only channel name that differs in German, shown in the desktop's sound settings
                *pw::keys::NODE_DESCRIPTION => format!("Mixpilot {}", if ch.id == "music" && german() { "Musik" } else { ch.label }),
                *pw::keys::NODE_GROUP => "mixpilot",
                *pw::keys::NODE_LINK_GROUP => "mixpilot",
                "priority.session" => "0",
            },
        )?;
        let rings = rings.clone();
        let fail = fail.clone();
        let l = stream
            .add_local_listener_with_user_data(())
            .state_changed(move |_, _, _, new| {
                if let StreamState::Error(e) = new {
                    fail(format!("channel {} failed: {e}", CHANNELS[i].id));
                }
            })
            .process(move |s, _| {
                let Some(mut buf) = s.dequeue_buffer() else { return };
                let Some(d) = buf.datas_mut().first_mut() else { return };
                let (off, size) = (d.chunk().offset() as usize, d.chunk().size() as usize);
                let Some(bytes) = d.data() else { return };
                let start = off.min(bytes.len());
                let end = start.saturating_add(size).min(bytes.len());
                // whole L/R frames only, and only the newest that fit, so pairs never shift
                let frames = (end - start) / 8;
                let keep = frames.min(RING_MAX / 2);
                let bytes = &bytes[start + (frames - keep) * 8..start + frames * 8];
                let mut ring = lock(&rings[i]);
                // drop oldest first so the push never grows past the preallocated capacity
                let over = (ring.len() + keep * 2).saturating_sub(RING_MAX);
                let n = over.min(ring.len());
                ring.drain(..n);
                ring.extend(bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
            })
            .register()?;
        stream.connect(
            spa::utils::Direction::Input,
            None,
            StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
            &mut [Pod::from_bytes(&stereo).unwrap()],
        )?;
        streams.push(stream);
        listeners.push(l);
    }

    // The output device is chosen through the "default" metadata like any app stream (see the
    // timer), never through the system default.
    let output = StreamRc::new(
        core.clone(),
        "mixpilot-output",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::NODE_NAME => "mixpilot_output",
            *pw::keys::NODE_DESCRIPTION => "Mixpilot Output",
            *pw::keys::NODE_GROUP => "mixpilot",
            *pw::keys::NODE_LINK_GROUP => "mixpilot",
            *pw::keys::APP_NAME => "Mixpilot",
        },
    )?;
    // No output device at all (headset unplugged, nothing else present) puts the stream into an
    // error state it never leaves by itself. Not fatal: the timer reconnects until one appears.
    let out_retry = Rc::new(Cell::new(false));
    let out_reported = Rc::new(Cell::new(false));
    let mut mix = Mix::new();
    let mut mon_prime = Prime(false);
    let (p, lv, out_rings, mon_ring) = (params.clone(), levels.clone(), rings.clone(), mic_mon.clone());
    let (retry, reported) = (out_retry.clone(), out_reported.clone());
    let out_listener = output
        .add_local_listener_with_user_data(())
        .state_changed(move |_, _, _, new| match new {
            StreamState::Error(e) => {
                if !reported.replace(true) {
                    log!("output: {e}, retrying until a device is available");
                }
                retry.set(true);
            }
            // Paused also shows up briefly before WirePlumber reports "no target", so only Streaming counts
            StreamState::Streaming => {
                if reported.replace(false) {
                    log!("output connected again");
                }
            }
            _ => {}
        })
        .process(move |s, _| {
            let Some(mut buf) = s.dequeue_buffer() else { return };
            let req = buf.requested() as usize;
            let Some(d) = buf.datas_mut().first_mut() else { return };
            let Some(bytes) = d.data() else { return };
            let mut frames = bytes.len() / 8;
            if req > 0 {
                frames = frames.min(req);
            }
            mix.update(&p);
            let limit = p.limiter.load(Relaxed);
            let clarity = p.clarity.load(Relaxed);
            let ducking = p.ducking.load(Relaxed);
            let monitor = p.monitor.load(Relaxed);
            let targets: [f32; N] = std::array::from_fn(|i| load(&p.gain[i]));
            let target_master = load(&p.master);
            let mut locked: [_; N] = std::array::from_fn(|i| lock(&out_rings[i]));
            // keep app audio at most ~3 cycles behind; anything older is a backlog, not a buffer
            for ring in locked.iter_mut() {
                let cap = frames * 2 * 3;
                if ring.len() > cap {
                    let n = ring.len() - cap;
                    ring.drain(..n);
                }
            }
            let mut mon = lock(&mon_ring);
            let mon_play = if monitor {
                mon_prime.ready(&mut mon, frames, frames + FRAME, frames + 3 * FRAME)
            } else {
                mon.clear();
                mon_prime.0 = false;
                false
            };
            let mut peaks = [0f32; LV_MASTER + 2];

            for f in 0..frames {
                let mut ch = [(0f32, 0f32); N];
                for c in 0..N {
                    let g = mix.gains[c].next(targets[c]);
                    // an underrun plays silence for that channel; drift correction (resampling) only if dropouts show up
                    if locked[c].len() >= 2 {
                        let a = sanitize(locked[c].pop_front().unwrap()) * g;
                        let b = sanitize(locked[c].pop_front().unwrap()) * g;
                        // flat bands are skipped, a flat preset costs nothing
                        let [el, er] = &mut mix.ch_eq[c];
                        ch[c] = (el.run(a), er.run(b));
                    }
                }
                // someone talks in Chat: Game, Media and Music step back by 12 dB, Aux stays
                let talk = mix.duck_env.run(ch[1].0.abs().max(ch[1].1.abs()));
                let duck = mix.duck.next(if ducking && talk > 0.01 { 0.25 } else { 1.0 });
                let (mut l, mut r) = (0.0f32, 0.0f32);
                for c in 0..N {
                    let k = if matches!(c, 0 | 2 | 3) { duck } else { 1.0 };
                    let (a, b) = (ch[c].0 * k, ch[c].1 * k);
                    peaks[c * 2] = peaks[c * 2].max(a.abs());
                    peaks[c * 2 + 1] = peaks[c * 2 + 1].max(b.abs());
                    l += a;
                    r += b;
                }
                l = mix.bass[0].run(sanitize(l));
                r = mix.bass[1].run(sanitize(r));
                if clarity {
                    l = mix.clarity[0].run(l);
                    r = mix.clarity[1].run(r);
                }
                if let Some(lev) = &mut mix.leveler {
                    let g = lev.gain(l.abs().max(r.abs()));
                    l *= g;
                    r *= g;
                }
                let m = mix.master.next(target_master);
                l *= m;
                r *= m;
                if mon_play && let Some(v) = mon.pop_front() {
                    l += v * 0.8;
                    r += v * 0.8;
                }
                if limit {
                    (l, r) = mix.limiter.run(l, r);
                } else {
                    (l, r) = (sanitize(l).clamp(-1.0, 1.0), sanitize(r).clamp(-1.0, 1.0));
                }
                peaks[LV_MASTER] = peaks[LV_MASTER].max(l.abs());
                peaks[LV_MASTER + 1] = peaks[LV_MASTER + 1].max(r.abs());
                bytes[f * 8..f * 8 + 4].copy_from_slice(&l.to_le_bytes());
                bytes[f * 8 + 4..f * 8 + 8].copy_from_slice(&r.to_le_bytes());
            }
            drop(mon);
            drop(locked);
            for (i, v) in peaks.iter().enumerate() {
                lv.raise(i, *v);
            }
            let chunk = d.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = 8;
            *chunk.size_mut() = (frames * 8) as u32;
        })
        .register()?;
    output.connect(
        spa::utils::Direction::Output,
        None,
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut [Pod::from_bytes(&stereo).unwrap()],
    )?;

    // Virtual microphone "Mixpilot Mikrofon": apps record the processed voice from here.
    // Its own node.group and link group keep the microphone's clock apart from the output's:
    // a shared link group put both under one driver, so the headset had to be resampled to the
    // microphone's clock (crackling) whenever the microphone was open.
    let mic_src = StreamRc::new(
        core.clone(),
        "mixpilot-mic",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CLASS => "Audio/Source",
            *pw::keys::NODE_NAME => "mixpilot_mic",
            *pw::keys::NODE_DESCRIPTION => if german() { "Mixpilot Mikrofon" } else { "Mixpilot Microphone" },
            *pw::keys::NODE_GROUP => "mixpilot-mic",
            *pw::keys::NODE_LINK_GROUP => "mixpilot-mic",
            "priority.session" => "0",
        },
    )?;
    // A stream node runs even without links (it always gets a driver), so its state says nothing
    // about listeners. Links whose output is this node are counted instead (registry below).
    let src_links: Rc<RefCell<HashSet<u32>>> = Rc::new(RefCell::new(HashSet::new()));
    let src_out = mic_out.clone();
    let mut src_prime = Prime(false);
    let src_listener = mic_src
        .add_local_listener_with_user_data(())
        .process(move |s, _| {
            let Some(mut buf) = s.dequeue_buffer() else { return };
            let req = buf.requested() as usize;
            let Some(d) = buf.datas_mut().first_mut() else { return };
            let Some(bytes) = d.data() else { return };
            let mut frames = bytes.len() / 4;
            if req > 0 {
                frames = frames.min(req);
            }
            let mut ring = lock(&src_out);
            let play = src_prime.ready(&mut ring, frames, frames + FRAME, frames + 3 * FRAME);
            for f in 0..frames {
                let v = if play { ring.pop_front().unwrap_or(0.0) } else { 0.0 };
                bytes[f * 4..f * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            drop(ring);
            let chunk = d.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = 4;
            *chunk.size_mut() = (frames * 4) as u32;
        })
        .register()?;
    mic_src.connect(
        spa::utils::Direction::Output,
        None,
        StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut [Pod::from_bytes(&mono).unwrap()],
    )?;

    // Microphone capture. Only connected while something needs it (an app records from
    // "Mixpilot Mikrofon", monitoring is on, or the app's microphone page is open), so the
    // desktop's microphone indicator is not lit all the time.
    let mic_in = StreamRc::new(
        core.clone(),
        "mixpilot-mic-in",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Communication",
            *pw::keys::NODE_NAME => "mixpilot_mic_in",
            *pw::keys::NODE_DESCRIPTION => if german() { "Mixpilot Mikrofon-Eingang" } else { "Mixpilot Microphone Input" },
            *pw::keys::NODE_GROUP => "mixpilot-mic",
            *pw::keys::NODE_LINK_GROUP => "mixpilot-mic",
            *pw::keys::APP_NAME => "Mixpilot",
        },
    )?;
    let mic_err = Rc::new(Cell::new(false));
    let mut mic_dsp = MicDsp::new();
    let (me, p, lv, mo, mm) = (mic_err.clone(), params.clone(), levels.clone(), mic_out.clone(), mic_mon.clone());
    let mic_listener = mic_in
        .add_local_listener_with_user_data(())
        .state_changed(move |_, _, _, new| {
            if let StreamState::Error(e) = new {
                log!("microphone: {e}");
                me.set(true);
            }
        })
        .process(move |s, _| {
            let Some(mut buf) = s.dequeue_buffer() else { return };
            let Some(d) = buf.datas_mut().first_mut() else { return };
            let (off, size) = (d.chunk().offset() as usize, d.chunk().size() as usize);
            let Some(bytes) = d.data() else { return };
            let start = off.min(bytes.len());
            let end = start.saturating_add(size).min(bytes.len());
            let mut input = [0f32; 2048];
            let n = ((end - start) / 4).min(input.len());
            for (i, b) in bytes[start..start + n * 4].chunks_exact(4).enumerate() {
                input[i] = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
            let mut out = lock(&mo);
            let monitor = p.monitor.load(Relaxed);
            let mut mon = lock(&mm);
            let (peak, in_peak) = mic_dsp.process(&input[..n], &p, &mut out, monitor.then_some(&mut *mon));
            drop(mon);
            drop(out);
            lv.raise(LV_MIC, peak);
            lv.raise(LV_MIC_IN, in_peak);
        })
        .register()?;

    // Auto-routing: move matching app streams to their channel via the "default" metadata.
    // The registry global only carries a few props, so each new stream is bound once to read
    // application.process.binary from its full info. The metadata belongs to WirePlumber; when it
    // restarts, the metadata comes back empty, so every target we own is written again.
    let registry = core.get_registry_rc()?;
    let reg_weak = registry.downgrade();
    let router: Rc<RefCell<Router>> = Rc::new(RefCell::new(Router::default()));
    let apps: Rc<RefCell<HashMap<u32, App>>> = Rc::new(RefCell::new(HashMap::new()));
    // listener first so it is dropped before its proxy
    let bound: Rc<RefCell<HashMap<u32, (NodeListener, Node)>>> = Rc::new(RefCell::new(HashMap::new()));
    let handled: Rc<RefCell<HashSet<u32>>> = Rc::new(RefCell::new(HashSet::new()));
    let (rt, ap, rcfg, b, h, ev) = (router.clone(), apps.clone(), cfg.clone(), bound.clone(), handled.clone(), events.clone());
    let (rt2, ap2, b2, h2) = (router.clone(), apps.clone(), bound.clone(), handled.clone());
    let (sl, sl2, src_node) = (src_links.clone(), src_links.clone(), mic_src.clone());
    // read here instead of from pw-dump, whose output changes between PipeWire versions
    let devices: Rc<RefCell<HashMap<u32, Device>>> = Rc::new(RefCell::new(HashMap::new()));
    let defaults: Rc<RefCell<[String; 2]>> = Rc::new(RefCell::new(Default::default()));
    let (dv, dv2, df) = (devices.clone(), devices.clone(), defaults.clone());
    let _reg_listener = registry
        .add_listener_local()
        .global(move |obj| {
            let Some(props) = obj.props else { return };
            let Some(reg) = reg_weak.upgrade() else { return };
            match obj.type_ {
                ObjectType::Metadata if props.get("metadata.name") == Some("default") => match reg.bind::<Metadata, _>(obj) {
                    Ok(m) => {
                        let df = df.clone();
                        let listener = m
                            .add_listener_local()
                            .property(move |subject, key, _, value| {
                                if subject != pw::core::PW_ID_CORE {
                                    return 0;
                                }
                                // values look like {"name": "alsa_output..."}
                                let name = || {
                                    value
                                        .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
                                        .and_then(|v| v["name"].as_str().map(str::to_string))
                                        .unwrap_or_default()
                                };
                                match key {
                                    Some("default.audio.sink") => df.borrow_mut()[0] = name(),
                                    Some("default.audio.source") => df.borrow_mut()[1] = name(),
                                    None => *df.borrow_mut() = Default::default(),
                                    _ => {}
                                }
                                0
                            })
                            .register();
                        let mut r = rt.borrow_mut();
                        for (id, target) in r.targets.iter() {
                            m.set_property(*id, "target.object", None, Some(target));
                        }
                        r.md = Some((obj.id, listener, m));
                    }
                    Err(e) => log!("cannot bind metadata: {e}"),
                },
                ObjectType::Link => {
                    let out_node = props.get("link.output.node").and_then(|v| v.parse::<u32>().ok());
                    if out_node.is_some_and(|n| n == src_node.node_id()) {
                        sl.borrow_mut().insert(obj.id);
                    }
                }
                ObjectType::Node => {
                    let name = props.get("node.name").unwrap_or("");
                    let class = props.get("media.class");
                    if matches!(class, Some("Audio/Sink" | "Audio/Source")) {
                        let description = props.get("node.description").unwrap_or(name).to_string();
                        dv.borrow_mut().insert(obj.id, Device { name: name.to_string(), description, sink: class == Some("Audio/Sink") });
                    }
                    if name == "easyeffects_sink" {
                        event(&ev, "easyeffects", &[]);
                    }
                    if props.get("media.class") != Some("Stream/Output/Audio") || name.starts_with("mixpilot") {
                        return;
                    }
                    let node: Node = match reg.bind(obj) {
                        Ok(n) => n,
                        Err(e) => return log!("cannot bind node {}: {e}", obj.id),
                    };
                    let (id, rt, ap, rcfg, h, ev) = (obj.id, rt.clone(), ap.clone(), rcfg.clone(), h.clone(), ev.clone());
                    let listener = node
                        .add_listener_local()
                        .info(move |info| {
                            let Some(p) = info.props() else { return };
                            if !h.borrow_mut().insert(id) {
                                return;
                            }
                            let s = |k: &str| p.get(k).unwrap_or("").to_string();
                            let mut app = App { name: s("application.name"), binary: s("application.process.binary"), node: s("node.name"), channel: None };
                            app.channel = rcfg.borrow().channel_for(&app.keys());
                            if let Some(ch) = app.channel {
                                rt.borrow_mut().set(id, &CHANNELS[ch].node_name());
                                event(&ev, "sorted", &[app.label(), CHANNELS[ch].label]);
                            }
                            ap.borrow_mut().insert(id, app);
                        })
                        .register();
                    b.borrow_mut().insert(obj.id, (listener, node));
                }
                _ => {}
            }
        })
        .global_remove(move |id| {
            // runs outside the proxies' own callbacks, so dropping them here is safe
            b2.borrow_mut().remove(&id);
            h2.borrow_mut().remove(&id);
            sl2.borrow_mut().remove(&id);
            dv2.borrow_mut().remove(&id);
            ap2.borrow_mut().remove(&id);
            let mut r = rt2.borrow_mut();
            r.targets.remove(&id);
            if r.md.as_ref().is_some_and(|(mid, ..)| *mid == id) {
                r.md = None;
            }
        })
        .register();

    // One timer does all the control work: config reload, night mode, device targets,
    // microphone on demand, reconnects and the state file the app reads for meters and lists.
    // stat() + state file every 50 ms is cheap enough; a socket would only pay off with more traffic
    let last = Cell::new(config::stamp(&path));
    let ticks = Cell::new(0u64);
    let out_ticks = Cell::new(0u32);
    let (out_seen, out_want) = (Cell::new(u32::MAX), RefCell::new(None::<String>));
    let (mic_seen, mic_want) = (Cell::new(u32::MAX), RefCell::new(None::<String>));
    let (mic_on, mic_idle, mic_wait) = (Cell::new(false), Cell::new(0u32), Cell::new(0u32));
    let mic_test = state_dir.join("mic-test");
    let state_file = state_dir.join("state.json");
    let (p, rcfg, rt, ap, ev, lv) = (params.clone(), cfg.clone(), router.clone(), apps.clone(), events.clone(), levels.clone());
    let (out, src_on, mic, merr, devs, defs) = (output.clone(), src_links.clone(), mic_in.clone(), mic_err.clone(), devices.clone(), defaults.clone());
    let (nightc, stereo2, mono2, state_out) = (night.clone(), stereo.clone(), mono.clone(), state_file.clone());
    let timer = mainloop.loop_().add_timer(move |_| {
        let tick = ticks.get() + 1;
        ticks.set(tick);

        // config
        let now = config::stamp(&path);
        if now != last.get() {
            last.set(now);
            match config::load(&path) {
                Ok(c) => {
                    let n = c.auto && c.night.active_at(local_time().0);
                    p.apply(&c, n);
                    nightc.set(n);
                    // rules may have changed: move running apps where they belong now
                    let mut r = rt.borrow_mut();
                    for (id, app) in ap.borrow_mut().iter_mut() {
                        let ch = c.channel_for(&app.keys());
                        if ch != app.channel {
                            match ch {
                                Some(i) => {
                                    r.set(*id, &CHANNELS[i].node_name());
                                    event(&ev, "moved", &[app.label(), CHANNELS[i].label]);
                                }
                                None => {
                                    r.clear(*id);
                                    event(&ev, "direct", &[app.label()]);
                                }
                            }
                            app.channel = ch;
                        }
                    }
                    drop(r);
                    *rcfg.borrow_mut() = c;
                }
                Err(e) => log!("config not applied, keeping previous: {e}"),
            }
        }

        // night mode, checked every 10 s
        if tick % 200 == 0 {
            let n = rcfg.borrow().auto && rcfg.borrow().night.active_at(local_time().0);
            if n != nightc.get() {
                nightc.set(n);
                p.apply(&rcfg.borrow(), n);
                event(&ev, if n { "night_on" } else { "night_off" }, &[]);
            }
        }

        // device targets: "" follows the system default, a node.name pins it. Applied to every new
        // node id (reconnects) and whenever the setting changes; deleting the key also clears a
        // target WirePlumber may have restored from an earlier session.
        let sync = |id: u32, want: String, seen: &Cell<u32>, last_want: &RefCell<Option<String>>, what: &'static str| {
            let mut r = rt.borrow_mut();
            if id == u32::MAX || r.md.is_none() {
                return;
            }
            let have = r.targets.get(&id).cloned().unwrap_or_default();
            if seen.get() == id && have == want {
                return;
            }
            seen.set(id);
            if want.is_empty() { r.clear(id) } else { r.set(id, &want) }
            drop(r);
            if last_want.borrow().as_deref() != Some(&want) {
                // an empty device name means the system default
                event(&ev, what, &[&want]);
                *last_want.borrow_mut() = Some(want);
            }
        };
        sync(out.node_id(), rcfg.borrow().output.clone(), &out_seen, &out_want, "output");
        if mic_on.get() {
            sync(mic.node_id(), rcfg.borrow().input.clone(), &mic_seen, &mic_want, "input");
        }

        // output reconnect, every 2 s while there is no device
        if out_retry.get() {
            out_ticks.set(out_ticks.get() + 1);
            if out_ticks.get() >= RETRY_TICKS {
                out_ticks.set(0);
                out_retry.set(false);
                let _ = out.disconnect();
                let flags = StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS;
                if let Err(e) = out.connect(spa::utils::Direction::Output, None, flags, &mut [Pod::from_bytes(&stereo2).unwrap()]) {
                    log!("output reconnect failed: {e}");
                    out_retry.set(true);
                }
            }
        }

        // microphone on demand
        if merr.replace(false) {
            let _ = mic.disconnect();
            mic_on.set(false);
            mic_wait.set(RETRY_TICKS);
        }
        mic_wait.set(mic_wait.get().saturating_sub(1));
        let wanted = !src_on.borrow().is_empty() || rcfg.borrow().mic.monitor || fresh(&mic_test, 3);
        if wanted {
            mic_idle.set(0);
            if !mic_on.get() && mic_wait.get() == 0 {
                let flags = StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS;
                let input = rcfg.borrow().input.clone();
                let target = devs.borrow().iter().find(|(_, d)| !d.sink && d.name == input && !input.starts_with("mixpilot")).map(|(id, _)| *id);
                match mic.connect(spa::utils::Direction::Input, target, flags, &mut [Pod::from_bytes(&mono2).unwrap()]) {
                    Ok(()) => {
                        mic_on.set(true);
                        mic_seen.set(u32::MAX);
                        event(&ev, "mic_active", &[]);
                    }
                    Err(e) => {
                        log!("microphone connect failed: {e}");
                        mic_wait.set(RETRY_TICKS);
                    }
                }
            }
        } else if mic_on.get() {
            mic_idle.set(mic_idle.get() + 1);
            if mic_idle.get() >= RETRY_TICKS {
                let _ = mic.disconnect();
                mic_on.set(false);
                event(&ev, "mic_paused", &[]);
            }
        }

        // state for the app: meters, apps, events
        let l = lv.take();
        let mut list: Vec<_> = ap
            .borrow()
            .values()
            .map(|a| {
                serde_json::json!({
                    "name": a.label(),
                    "key": if a.binary.is_empty() { a.label().to_lowercase() } else { a.binary.to_lowercase() },
                    "channel": a.channel.map(|c| CHANNELS[c].id),
                })
            })
            .collect();
        // one row per key (two Wine games share wine64-preloader), then by name for the list
        list.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()).then(a["name"].as_str().cmp(&b["name"].as_str())));
        list.dedup_by(|a, b| a["key"] == b["key"]);
        list.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        let dl = devs.borrow();
        let hw = |sink: bool| {
            let mut v: Vec<&Device> = dl.values().filter(|d| d.sink == sink && !d.name.starts_with("mixpilot")).collect();
            v.sort_by(|a, b| a.description.cmp(&b.description));
            v
        };
        let def = defs.borrow();
        let state = serde_json::json!({
            // the app replaces a running core whose protocol differs (old version after an update)
            "proto": STATE_PROTO,
            "levels": levels_json(&l, mic_on.get()),
            "mic_active": mic_on.get(),
            "night_active": nightc.get(),
            "apps": list,
            "devices": {
                "sinks": hw(true),
                "sources": hw(false),
                "default_sink": def[0],
                "default_source": def[1],
                "duplicate": dl.values().filter(|d| d.name == "mixpilot_game").count() > 1,
            },
            "events": ev.borrow().iter().map(|(t, k, a)| serde_json::json!({"time": t, "key": k, "args": a})).collect::<Vec<_>>(),
        });
        if let Ok(bytes) = serde_json::to_vec(&state) {
            write_atomic(&state_out, &bytes);
        }
    });
    timer.update_timer(Some(Duration::from_millis(TICK_MS)), Some(Duration::from_millis(TICK_MS)));

    let quit = |ml: pw::main_loop::MainLoopWeak| {
        move || {
            if let Some(ml) = ml.upgrade() {
                ml.quit();
            }
        }
    };
    let _sigint = mainloop.loop_().add_signal_local(pw::loop_::Signal::INT, quit(mainloop.downgrade()));
    let _sigterm = mainloop.loop_().add_signal_local(pw::loop_::Signal::TERM, quit(mainloop.downgrade()));

    let core_fail = fail.clone();
    let _core_listener = core
        .add_listener_local()
        .error(move |id, _seq, res, msg| {
            if id == pw::core::PW_ID_CORE {
                core_fail(format!("PipeWire connection lost ({res}: {msg})"));
            }
        })
        .register();

    log!("running");
    mainloop.run();
    // Disconnect first so the data threads are out of our process callbacks before the listeners go.
    for s in &streams {
        let _ = s.disconnect();
    }
    let _ = output.disconnect();
    let _ = mic_in.disconnect();
    let _ = mic_src.disconnect();
    drop((out_listener, listeners, mic_listener, src_listener));
    drop(timer);
    drop((output, streams, mic_in, mic_src));
    let _ = std::fs::remove_file(&state_file);
    if failed.get() {
        log!("stopped after failure");
        std::process::exit(2);
    }
    log!("stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn refused_second_instance_keeps_pid() {
        let path = std::env::temp_dir().join(format!("mixpilot-lock-test-{}", std::process::id()));
        let held = super::single_instance(&path).expect("first lock");
        assert!(super::single_instance(&path).is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), std::process::id().to_string());
        drop(held);
        let _ = std::fs::remove_file(&path);
    }
}
