//! Global "mute microphone" shortcut through the XDG GlobalShortcuts portal (GNOME 48+, KDE).
//! The desktop asks for the key in its own dialog, Mixpilot only hears "pressed".

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::blocking::Connection;
use dbus::message::MatchRule;
use dbus::Path;
use serde::Serialize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEST: &str = "org.freedesktop.portal.Desktop";
const OBJ: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
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
pub fn start(restore: bool, on_press: fn()) {
    let (tx, rx) = channel::<(Cmd, Reply)>();
    let _ = CMDS.set(Mutex::new(tx));
    std::thread::spawn(move || {
        let Ok(c) = Connection::new_session() else { return };
        let p = c.with_proxy(DEST, OBJ, Duration::from_secs(5));
        if std::env::var_os("SNAP").is_none() && std::env::var_os("FLATPAK_ID").is_none() {
            // unsandboxed apps have to name themselves first, otherwise the desktop cannot store the key
            let _: Result<(), _> = p.method_call("org.freedesktop.host.portal.Registry", "Register", ("mixpilot", PropMap::new()));
        }
        if p.get::<u32>(IFACE, "version").is_err() {
            return;
        }
        STATUS.lock().unwrap().available = true;
        let pressed = MatchRule::new_signal(IFACE, "Activated");
        let _ = c.add_match(pressed, move |(_, id): (Path, String), _, _| {
            if id == ID {
                on_press();
            }
            true
        });
        let mut session = None;
        if restore {
            if let Err(e) = bind(&c, &mut session) {
                eprintln!("mixpilot: hotkey: {e}");
            }
        }
        loop {
            if let Ok((cmd, reply)) = rx.try_recv() {
                let r = match cmd {
                    Cmd::On => bind(&c, &mut session),
                    Cmd::Off => Ok(close(&c, &mut session)),
                    Cmd::Change => change(&c, &mut session),
                };
                let _ = reply.send(r.map(|_| status()));
            }
            if c.process(Duration::from_millis(200)).is_err() {
                return;
            }
        }
    });
}

/// Without a key the shortcut stays off. Cancelling the dialog keeps the previous state.
fn bind(c: &Connection, session: &mut Option<String>) -> Result<(), String> {
    let p = c.with_proxy(DEST, OBJ, Duration::from_secs(5));
    if session.is_none() {
        let r = request(c, |mut opts| {
            opts.insert("session_handle_token".into(), Variant(Box::new("mixpilot".to_string())));
            p.method_call(IFACE, "CreateSession", (opts,)).map(|_: (Path,)| ())
        })?;
        *session = Some(r.session.ok_or("Keine Sitzung für Tastenkürzel bekommen")?);
    }
    let handle = Path::from(session.clone().unwrap());
    let mut props = PropMap::new();
    props.insert("description".into(), Variant(Box::new(crate::tr("Mikrofon stumm schalten", "Mute microphone").to_string())));
    let shortcuts = vec![(ID.to_string(), props)];
    let r = request(c, |opts| p.method_call(IFACE, "BindShortcuts", (handle, shortcuts, "", opts)).map(|_: (Path,)| ()))?;
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
fn change(c: &Connection, session: &mut Option<String>) -> Result<(), String> {
    if let Some(s) = session.as_ref() {
        let p = c.with_proxy(DEST, OBJ, Duration::from_secs(5));
        let r: Result<(), _> = p.method_call(IFACE, "ConfigureShortcuts", (Path::from(s.clone()), "", PropMap::new()));
        if r.is_ok() {
            return Ok(());
        }
    }
    bind(c, session)
}

struct Answer {
    code: u32,
    session: Option<String>,
    trigger: Option<String>,
}

/// Portal calls answer later through a Request object; subscribe before calling so no answer is lost.
fn request(c: &Connection, call: impl FnOnce(PropMap) -> Result<(), dbus::Error>) -> Result<Answer, String> {
    static N: AtomicU32 = AtomicU32::new(0);
    let token = format!("mixpilot{}", N.fetch_add(1, Ordering::Relaxed));
    let sender = c.unique_name().trim_start_matches(':').replace('.', "_");
    let path = format!("{OBJ}/request/{sender}/{token}");
    let slot = Arc::new(Mutex::new(None));
    let s2 = slot.clone();
    let rule = MatchRule::new_signal("org.freedesktop.portal.Request", "Response").with_path(path);
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
