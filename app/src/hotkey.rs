//! Global "mute microphone" shortcut through the XDG GlobalShortcuts portal (GNOME 48+, KDE).
//! The desktop asks for the key in its own dialog, Mixpilot only hears "pressed".

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::blocking::Connection;
use dbus::message::MatchRule;
use dbus::Path;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEST: &str = "org.freedesktop.portal.Desktop";
const OBJ: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const DBUS: &str = "org.freedesktop.DBus";
const ID: &str = "mic-mute";
// the desktop's dialog waits for the user
const ANSWER_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Serialize)]
pub struct Status {
    available: bool,
    on: bool,
    trigger: String,
}

pub enum Cmd {
    On,
    Off,
    Change,
}

type Reply = Sender<Result<Status, String>>;

static STATUS: Mutex<Status> = Mutex::new(Status { available: false, on: false, trigger: String::new() });
static CMDS: OnceLock<Mutex<Sender<(Cmd, Reply)>>> = OnceLock::new();

pub fn status() -> Status {
    STATUS.lock().unwrap().clone()
}

/// May open the desktop's shortcut dialog and waits until the user is done with it.
pub fn send(cmd: Cmd) -> Result<Status, String> {
    let (tx, rx) = channel();
    CMDS.get().ok_or("Globale Tastenkürzel sind nicht verfügbar")?.lock().unwrap().send((cmd, tx)).map_err(|e| e.to_string())?;
    rx.recv_timeout(ANSWER_TIMEOUT + Duration::from_secs(5)).map_err(|e| e.to_string())?
}

/// Runs the portal session on its own thread. `restore` re-binds the key chosen in an earlier run.
/// At login the bus or the portal may come up after Mixpilot, and the portal may restart later.
pub fn start(restore: bool, on_press: fn()) {
    let (tx, rx) = channel::<(Cmd, Reply)>();
    let _ = CMDS.set(Mutex::new(tx));
    std::thread::spawn(move || {
        let (mut restore, mut wait, mut last) = (restore, Duration::from_secs(2), String::new());
        loop {
            let up = Instant::now();
            let why = run(&rx, restore, on_press);
            {
                let mut st = STATUS.lock().unwrap();
                if st.available {
                    restore = st.on;
                }
                *st = Status { available: false, on: false, trigger: String::new() };
            }
            // a desktop without the portal would otherwise log the same line every minute
            if why != last {
                eprintln!("mixpilot: hotkey: {why}");
                last = why;
            }
            if up.elapsed() > Duration::from_secs(60) {
                wait = Duration::from_secs(2);
            }
            refuse_until(&rx, Instant::now() + wait);
            wait = (wait * 2).min(Duration::from_secs(60));
        }
    });
}

/// Without a portal, commands get their answer right away instead of after the timeout.
fn refuse_until(rx: &Receiver<(Cmd, Reply)>, end: Instant) {
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok((_, reply)) => {
                let _ = reply.send(Err("Globale Tastenkürzel sind nicht verfügbar".into()));
            }
            Err(RecvTimeoutError::Disconnected) => std::thread::sleep(left),
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// One portal connection. Returns why it ended: no bus, no portal, or the portal went away.
fn run(rx: &Receiver<(Cmd, Reply)>, restore: bool, on_press: fn()) -> String {
    let c = match Connection::new_session() {
        Ok(c) => c,
        Err(e) => return e.to_string(),
    };
    let p = c.with_proxy(DEST, OBJ, Duration::from_secs(5));
    if std::env::var_os("SNAP").is_none() && std::env::var_os("FLATPAK_ID").is_none() {
        // unsandboxed apps have to name themselves first, otherwise the desktop cannot store the key
        let _: Result<(), _> = p.method_call("org.freedesktop.host.portal.Registry", "Register", ("mixpilot", PropMap::new()));
    }
    if let Err(e) = p.get::<u32>(IFACE, "version") {
        return e.to_string();
    }
    // signals count only from the portal itself, any app on the bus could send one straight to us
    let bus = c.with_proxy(DBUS, "/org/freedesktop/DBus", Duration::from_secs(5));
    let owner: String = match bus.method_call(DBUS, "GetNameOwner", (DEST,)) {
        Ok((o,)) => o,
        Err(e) => return e.to_string(),
    };
    STATUS.lock().unwrap().available = true;
    let gone = Arc::new(AtomicBool::new(false));
    let g = gone.clone();
    let _ = c.add_match(MatchRule::new_signal(DBUS, "NameOwnerChanged").with_strict_sender(DBUS), move |(name,): (String,), _, _| {
        if name == DEST {
            g.store(true, Ordering::Relaxed);
        }
        true
    });
    let pressed = MatchRule::new_signal(IFACE, "Activated").with_strict_sender(owner.clone());
    let _ = c.add_match(pressed, move |(_, id): (Path, String), _, _| {
        if id == ID {
            on_press();
        }
        true
    });
    // the key changed or was removed in the desktop's own settings
    let changed = MatchRule::new_signal(IFACE, "ShortcutsChanged").with_strict_sender(owner.clone());
    let _ = c.add_match(changed, |(_, list): (Path, Vec<(String, PropMap)>), _, _| {
        if let Some((_, props)) = list.iter().find(|(id, _)| id == ID) {
            let trigger = props.get("trigger_description").and_then(|v| v.0.as_str()).unwrap_or_default().to_string();
            let mut st = STATUS.lock().unwrap();
            st.on = st.on && !trigger.is_empty();
            st.trigger = trigger;
        }
        true
    });
    let mut session = None;
    if restore && let Err(e) = bind(&c, &owner, &mut session) {
        eprintln!("mixpilot: hotkey: {e}");
    }
    loop {
        if let Ok((cmd, reply)) = rx.try_recv() {
            let r = match cmd {
                Cmd::On => bind(&c, &owner, &mut session),
                Cmd::Off => {
                    close(&c, &mut session);
                    Ok(())
                }
                Cmd::Change => change(&c, &owner, &mut session),
            };
            let _ = reply.send(r.map(|_| status()));
        }
        if let Err(e) = c.process(Duration::from_millis(200)) {
            return e.to_string();
        }
        if gone.load(Ordering::Relaxed) {
            return "the portal went away".into();
        }
    }
}

/// Without a key the shortcut stays off. Cancelling the dialog keeps the previous state.
fn bind(c: &Connection, owner: &str, session: &mut Option<String>) -> Result<(), String> {
    let p = c.with_proxy(DEST, OBJ, Duration::from_secs(5));
    if session.is_none() {
        let r = request(c, owner, |mut opts| {
            opts.insert("session_handle_token".into(), Variant(Box::new("mixpilot".to_string())));
            p.method_call(IFACE, "CreateSession", (opts,)).map(|_: (Path,)| ())
        })?;
        *session = Some(r.session.ok_or("Keine Sitzung für Tastenkürzel bekommen")?);
    }
    let handle = Path::from(session.clone().unwrap());
    let mut props = PropMap::new();
    props.insert("description".into(), Variant(Box::new(crate::tr("Mikrofon stumm schalten", "Mute microphone").to_string())));
    let shortcuts = vec![(ID.to_string(), props)];
    let r = request(c, owner, |opts| p.method_call(IFACE, "BindShortcuts", (handle, shortcuts, "", opts)).map(|_: (Path,)| ()))?;
    match r.code {
        0 => {
            let trigger = r.trigger.unwrap_or_default();
            let on = !trigger.is_empty();
            *STATUS.lock().unwrap() = Status { available: true, on, trigger };
            if !on {
                close(c, session);
            }
            Ok(())
        }
        1 => {
            if !STATUS.lock().unwrap().on {
                close(c, session);
            }
            Ok(())
        }
        _ => {
            close(c, session);
            Err("Der Desktop hat das Tastenkürzel abgelehnt".into())
        }
    }
}

/// Ending the session releases the key, so other apps get it again.
fn close(c: &Connection, session: &mut Option<String>) {
    if let Some(s) = session.take() {
        let _: Result<(), _> = c.with_proxy(DEST, s, Duration::from_secs(5)).method_call("org.freedesktop.portal.Session", "Close", ());
    }
    let mut st = STATUS.lock().unwrap();
    st.on = false;
}

/// Newer portals open the desktop's own settings for a bound key; older ones only know BindShortcuts.
fn change(c: &Connection, owner: &str, session: &mut Option<String>) -> Result<(), String> {
    if let Some(s) = session.as_ref() {
        let p = c.with_proxy(DEST, OBJ, Duration::from_secs(5));
        let r: Result<(), _> = p.method_call(IFACE, "ConfigureShortcuts", (Path::from(s.clone()), "", PropMap::new()));
        if r.is_ok() {
            return Ok(());
        }
    }
    bind(c, owner, session)
}

struct Answer {
    code: u32,
    session: Option<String>,
    trigger: Option<String>,
}

/// Portal calls answer later through a Request object; subscribe before calling so no answer is lost.
fn request(c: &Connection, owner: &str, call: impl FnOnce(PropMap) -> Result<(), dbus::Error>) -> Result<Answer, String> {
    static N: AtomicU32 = AtomicU32::new(0);
    let token = format!("mixpilot{}", N.fetch_add(1, Ordering::Relaxed));
    let sender = c.unique_name().trim_start_matches(':').replace('.', "_");
    let path = format!("{OBJ}/request/{sender}/{token}");
    let slot = Arc::new(Mutex::new(None));
    let s2 = slot.clone();
    let rule = MatchRule::new_signal("org.freedesktop.portal.Request", "Response").with_path(path).with_strict_sender(owner.to_string());
    let m = c
        .add_match(rule, move |(code, res): (u32, PropMap), _, _| {
            *s2.lock().unwrap() = Some(Answer {
                code,
                session: res.get("session_handle").and_then(|v| v.0.as_str()).map(str::to_string),
                trigger: res.get("shortcuts").and_then(|v| trigger_of(&*v.0)),
            });
            false
        })
        .map_err(|e| e.to_string())?;
    let mut opts = PropMap::new();
    opts.insert("handle_token".into(), Variant(Box::new(token)));
    let sent = call(opts).map_err(|e| e.to_string());
    let start = Instant::now();
    while sent.is_ok() && slot.lock().unwrap().is_none() && start.elapsed() < ANSWER_TIMEOUT {
        c.process(Duration::from_millis(200)).map_err(|e| e.to_string())?;
    }
    let _ = c.remove_match(m);
    sent?;
    slot.lock().unwrap().take().ok_or_else(|| "Keine Antwort vom Desktop".into())
}

/// a(sa{sv}) -> trigger_description of our shortcut
fn trigger_of(list: &dyn RefArg) -> Option<String> {
    for item in list.as_iter()? {
        let mut fields = item.as_iter()?;
        let (id, props) = (fields.next()?, fields.next()?);
        if id.as_str() != Some(ID) {
            continue;
        }
        let mut kv = props.as_iter()?;
        while let (Some(k), Some(v)) = (kv.next(), kv.next()) {
            if k.as_str() == Some("trigger_description") {
                return v.as_str().map(str::to_string);
            }
        }
        return Some(String::new());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbus::channel::{MatchingReceiver as _, Sender as _};
    use dbus::Message;
    use std::process::{Child, Command, Stdio};

    struct Bus(Child, String);
    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A private bus without service activation, so no real portal is ever started on it.
    fn private_bus() -> Option<Bus> {
        Command::new("dbus-daemon").arg("--version").stdout(Stdio::null()).status().ok()?;
        let dir = std::env::temp_dir().join(format!("mixpilot-hotkey-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (conf, sock) = (dir.join("bus.conf"), dir.join("bus"));
        let addr = format!("unix:path={}", sock.display());
        std::fs::write(&conf, format!("<busconfig><type>session</type><listen>{addr}</listen><auth>EXTERNAL</auth><policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>")).unwrap();
        let child = Command::new("dbus-daemon").arg(format!("--config-file={}", conf.display())).arg("--nofork").stdout(Stdio::null()).spawn().unwrap();
        let bus = Bus(child, addr);
        assert!(until(3000, || sock.exists()), "private dbus-daemon did not start");
        Some(bus)
    }

    fn until(ms: u64, f: impl Fn() -> bool) -> bool {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        f()
    }

    /// Owns the portal name, answers CreateSession and BindShortcuts with Ctrl+M, presses on request.
    struct Portal {
        stop: Arc<AtomicBool>,
        press: Arc<AtomicU32>,
        binds: Arc<AtomicU32>,
        app: Arc<Mutex<Option<String>>>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Portal {
        fn start() -> Portal {
            let (stop, press, binds, app) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)), Arc::new(Mutex::new(None)));
            let (s, p, b, a) = (stop.clone(), press.clone(), binds.clone(), app.clone());
            let (ready_tx, ready_rx) = channel();
            let thread = std::thread::spawn(move || {
                let c = Connection::new_session().unwrap();
                c.request_name(DEST, false, true, false).unwrap();
                let (b2, a2) = (b.clone(), a.clone());
                c.start_receive(
                    MatchRule::new_method_call(),
                    Box::new(move |m: Message, c: &Connection| {
                        let from = m.sender().unwrap().to_string();
                        *a2.lock().unwrap() = Some(from.clone());
                        let who = from.trim_start_matches(':').replace('.', "_");
                        let respond = |opts: &PropMap, results: PropMap| {
                            let token = opts.get("handle_token").and_then(|v| v.0.as_str()).unwrap().to_string();
                            let path = format!("{OBJ}/request/{who}/{token}");
                            let _ = c.send(m.method_return().append1(Path::from(path.clone())));
                            let mut sig = Message::new_signal(path, "org.freedesktop.portal.Request", "Response").unwrap().append2(0u32, results);
                            sig.set_destination(Some(from.clone().into()));
                            let _ = c.send(sig);
                        };
                        match m.member().as_deref() {
                            Some("Get") => drop(c.send(m.method_return().append1(Variant(1u32)))),
                            Some("CreateSession") => {
                                let opts: PropMap = m.read1().unwrap();
                                let mut r = PropMap::new();
                                r.insert("session_handle".into(), Variant(Box::new(format!("{OBJ}/session/{who}/mixpilot"))));
                                respond(&opts, r);
                            }
                            Some("BindShortcuts") => {
                                let (_, _, _, opts): (Path, Vec<(String, PropMap)>, String, PropMap) = m.read4().unwrap();
                                b2.fetch_add(1, Ordering::SeqCst);
                                let mut props = PropMap::new();
                                props.insert("trigger_description".into(), Variant(Box::new("Ctrl+M".to_string())));
                                let mut r = PropMap::new();
                                r.insert("shortcuts".into(), Variant(Box::new(vec![(ID.to_string(), props)])));
                                respond(&opts, r);
                            }
                            _ => drop(c.send(m.method_return())),
                        }
                        true
                    }),
                );
                ready_tx.send(()).unwrap();
                let mut pressed = 0;
                while !s.load(Ordering::SeqCst) {
                    c.process(Duration::from_millis(20)).unwrap();
                    if p.load(Ordering::SeqCst) > pressed {
                        pressed += 1;
                        let to = a.lock().unwrap().clone().unwrap();
                        let _ = c.send(activated(&to));
                    }
                }
            });
            ready_rx.recv().unwrap();
            Portal { stop, press, binds, app, thread: Some(thread) }
        }
    }
    impl Drop for Portal {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            let _ = self.thread.take().map(|t| t.join());
        }
    }

    fn activated(to: &str) -> Message {
        let session = Path::from(format!("{OBJ}/session/x/mixpilot"));
        let mut m = Message::new_signal(OBJ, IFACE, "Activated").unwrap().append3(session, ID, 0u64).append1(PropMap::new());
        m.set_destination(Some(to.to_string().into()));
        m
    }

    static PRESSES: AtomicU32 = AtomicU32::new(0);
    fn pressed() {
        PRESSES.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn portal_comes_late_spoofs_are_ignored_and_a_restart_restores_the_key() {
        let Some(bus) = private_bus() else { return eprintln!("no dbus-daemon, skipped") };
        // SAFETY: the only test that reads the session bus address
        unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &bus.1) };
        start(false, pressed);

        // no portal yet: an answer right away, not after five minutes
        let t = Instant::now();
        assert!(send(Cmd::On).is_err());
        assert!(t.elapsed() < Duration::from_secs(3));

        let portal = Portal::start();
        assert!(until(8000, || status().available), "portal not picked up");
        let s = send(Cmd::On).unwrap();
        assert!(s.on && s.trigger == "Ctrl+M");
        portal.press.fetch_add(1, Ordering::SeqCst);
        assert!(until(2000, || PRESSES.load(Ordering::SeqCst) == 1));

        // the same signal from another app on the bus
        let spoof = Connection::new_session().unwrap();
        let app = portal.app.lock().unwrap().clone().unwrap();
        spoof.send(activated(&app)).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(PRESSES.load(Ordering::SeqCst), 1, "a spoofed press counted");

        // the portal restarts: the key comes back by itself
        drop(portal);
        assert!(until(2000, || !status().available));
        let portal = Portal::start();
        assert!(until(8000, || status().on), "key not restored after a portal restart");
        assert_eq!(portal.binds.load(Ordering::SeqCst), 1);
        portal.press.fetch_add(1, Ordering::SeqCst);
        assert!(until(2000, || PRESSES.load(Ordering::SeqCst) == 2));
    }
}
