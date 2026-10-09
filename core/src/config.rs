use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct ChannelDef {
    pub id: &'static str,
    pub label: &'static str,
}

impl ChannelDef {
    pub fn node_name(&self) -> String {
        format!("mixpilot_{}", self.id)
    }
}

pub const CHANNELS: [ChannelDef; 5] = [
    ChannelDef { id: "game", label: "Game" },
    ChannelDef { id: "chat", label: "Chat" },
    ChannelDef { id: "media", label: "Media" },
    ChannelDef { id: "music", label: "Music" },
    ChannelDef { id: "aux", label: "Aux" },
];

/// Built-in EQ presets, 10 bands in dB at dsp::EQ_FREQS. app.js shows the same table as EQ_BUILTIN.
pub const EQ_BUILTIN: [(&str, [f32; 10]); 5] = [
    ("flat", [0.0; 10]),
    ("bass", [6.0, 5.0, 3.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("voice", [-3.0, -2.0, -1.0, 0.0, 2.0, 3.0, 3.0, 2.0, 0.0, -1.0]),
    ("gaming", [3.0, 4.0, 2.0, 0.0, -1.0, 0.0, 1.0, 3.0, 2.0, 1.0]),
    ("clear", [0.0, 0.0, -1.0, -1.0, 0.0, 1.0, 2.0, 3.0, 3.0, 2.0]),
];

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Channel {
    pub volume: f32,
    pub mute: bool,
    /// EQ preset: a key of EQ_BUILTIN or the name of one of the user's own presets
    pub eq: String,
}

impl Default for Channel {
    fn default() -> Self {
        Self { volume: 100.0, mute: false, eq: "flat".into() }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Rule {
    /// Case-insensitive substring of the app's binary, application.name or node.name.
    #[serde(rename = "match")]
    pub pattern: String,
    /// Channel id; anything else (e.g. "none") keeps the app out of Mixpilot.
    pub channel: String,
    /// Set by the user in the app. Only these apply while automatic sorting is off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub user: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Mic {
    /// 0..=200 %, see dsp::mic_gain
    pub gain: f32,
    pub mute: bool,
    /// "off" | "normal" | "strong"
    pub noise: String,
    pub agc: bool,
    pub gate: bool,
    /// see dsp::VOICE_PRESETS
    pub voice: String,
    /// hear yourself on the output
    pub monitor: bool,
    /// "off" | "normal" | "strong": turns sharp S sounds down
    pub deess: String,
}

impl Default for Mic {
    fn default() -> Self {
        Self { gain: 100.0, mute: false, noise: "normal".into(), agc: true, gate: true, voice: "natural".into(), monitor: false, deess: "normal".into() }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Night {
    pub enabled: bool,
    pub from: u32,
    pub to: u32,
}

impl Default for Night {
    fn default() -> Self {
        Self { enabled: false, from: 22, to: 7 }
    }
}

impl Night {
    pub fn active_at(&self, hour: u32) -> bool {
        let (f, t) = (self.from % 24, self.to % 24);
        self.enabled && if f <= t { hour >= f && hour < t } else { hour >= f || hour < t }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    /// Autopilot master switch: automatic sorting, ducking and night mode.
    pub auto: bool,
    pub game: Channel,
    pub chat: Channel,
    pub media: Channel,
    pub music: Channel,
    pub aux: Channel,
    pub master: f32,
    /// -100 (Chat down) ..= 100 (Game down)
    pub chatmix: f32,
    /// 0..=100, maps to 0..=12 dB low shelf
    pub bass: f32,
    pub limiter: bool,
    /// presence lift around 3 kHz
    pub clarity: bool,
    /// "off" | "soft" | "night"
    pub auto_volume: String,
    /// the user's own EQ presets, name -> 10 bands in dB
    pub eq_presets: HashMap<String, Vec<f32>>,
    /// built-in presets the user tuned, key -> 10 bands in dB; missing means as shipped
    pub eq_edits: HashMap<String, Vec<f32>>,
    /// node.name of the hardware sink, empty = system default
    pub output: String,
    /// node.name of the microphone, empty = system default source
    pub input: String,
    pub mic: Mic,
    /// lower Game, Media and Music while someone talks in Chat
    pub ducking: bool,
    pub night: Night,
    /// sort new apps into channels with the built-in rules
    pub auto_route: bool,
    pub rules: Vec<Rule>,
}

impl Default for Config {
    fn default() -> Self {
        let r = |p: &str, c: &str| Rule { pattern: p.into(), channel: c.into(), user: false };
        Self {
            auto: true,
            game: Channel { eq: "gaming".into(), ..Channel::default() },
            chat: Channel { eq: "voice".into(), ..Channel::default() },
            media: Channel::default(),
            music: Channel::default(),
            aux: Channel::default(),
            master: 100.0,
            chatmix: 0.0,
            bass: 0.0,
            limiter: true,
            clarity: false,
            auto_volume: "off".into(),
            eq_presets: HashMap::new(),
            eq_edits: HashMap::new(),
            output: String::new(),
            input: String::new(),
            mic: Mic::default(),
            ducking: true,
            night: Night::default(),
            auto_route: true,
            // Order matters: first match wins, so chat apps built on Chromium/Electron come before browsers.
            rules: vec![
                r("discord", "chat"),
                r("vesktop", "chat"),
                r("teamspeak", "chat"),
                r("mumble", "chat"),
                r("zoom", "chat"),
                r("signal", "chat"),
                r("element", "chat"),
                r("webrtc", "chat"),
                r("wine", "game"),
                r("proton", "game"),
                r("steam", "game"),
                r("gamescope", "game"),
                r("obs", "aux"),
                r("spotify", "music"),
                r("rhythmbox", "music"),
                r("amberol", "music"),
                r("lollypop", "music"),
                r("elisa", "music"),
                r("strawberry", "music"),
                r("clementine", "music"),
                r("audacious", "music"),
                r("tidal", "music"),
                r("deezer", "music"),
                r("firefox", "media"),
                r("chromium", "media"),
                r("chrome", "media"),
                r("vlc", "media"),
                r("mpv", "media"),
                r("youtube", "media"),
            ],
        }
    }
}

pub fn index_of(list: &[&str], name: &str) -> u32 {
    list.iter().position(|n| *n == name).unwrap_or(0) as u32
}

impl Config {
    pub fn channels(&self) -> [&Channel; CHANNELS.len()] {
        [&self.game, &self.chat, &self.media, &self.music, &self.aux]
    }

    /// The bands a channel preset stands for; an unknown name plays flat.
    pub fn curve(&self, name: &str) -> [f32; 10] {
        let ten = |v: &Vec<f32>| <[f32; 10]>::try_from(v.as_slice()).ok();
        match EQ_BUILTIN.iter().find(|(k, _)| *k == name) {
            Some((_, shipped)) => self.eq_edits.get(name).and_then(ten).unwrap_or(*shipped),
            None => self.eq_presets.get(name).and_then(ten).unwrap_or([0.0; 10]),
        }
    }

    /// The first matching rule decides. Built-in rules are skipped while automatic sorting is off.
    pub fn channel_for(&self, keys: &[Option<&str>]) -> Option<usize> {
        let keys: Vec<String> = keys.iter().flatten().map(|k| k.to_lowercase()).collect();
        let auto = self.auto && self.auto_route;
        let rule = self.rules.iter().filter(|r| r.user || auto).find(|rule| {
            let pat = rule.pattern.to_lowercase();
            !pat.is_empty() && keys.iter().any(|k| k.contains(&pat))
        })?;
        CHANNELS.iter().position(|c| c.id == rule.channel)
    }
}

pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("mixpilot").join("config.json")
}

/// Change detection key. Size is included so a half-written file (same mtime granule) is retried.
pub fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    std::fs::metadata(path).and_then(|m| Ok((m.modified()?, m.len()))).ok()
}

pub fn load(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse(&text)
}

/// Only a JSON object counts; serde would also accept `[]` and silently reset every fader.
pub fn parse(text: &str) -> Result<Config, String> {
    let mut v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if !v.is_object() {
        return Err("not a JSON object".into());
    }
    migrate(&mut v);
    serde_json::from_value(v).map_err(|e| e.to_string())
}

/// Configs from before the Music channel carry their own copy of the built-in rules, which sends
/// Spotify and Rhythmbox to Media. Those two entries go, the built-in music rules come in before the
/// first built-in Media rule; every other rule stays. Works on the raw JSON so keys only the app
/// knows (own presets, seen_version) survive a write back.
fn migrate(v: &mut serde_json::Value) -> bool {
    if v.get("music").is_some() {
        return false;
    }
    v["music"] = serde_json::to_value(Channel::default()).unwrap_or_default();
    // get_mut, not v["rules"]: indexing would insert a null and break the config
    let Some(rules) = v.get_mut("rules").and_then(|r| r.as_array_mut()) else { return true };
    let builtin = |r: &serde_json::Value| r["user"] != true;
    let music: Vec<Rule> = Config::default().rules.into_iter().filter(|r| r.channel == "music").collect();
    rules.retain(|r| !(builtin(r) && r["channel"] == "media" && music.iter().any(|m| r["match"] == m.pattern.as_str())));
    let new: Vec<serde_json::Value> = music
        .iter()
        .filter(|m| !rules.iter().any(|r| r["match"] == m.pattern.as_str()))
        .filter_map(|m| serde_json::to_value(m).ok())
        .collect();
    let at = rules.iter().position(|r| builtin(r) && r["channel"] == "media").unwrap_or(rules.len());
    rules.splice(at..at, new);
    true
}

/// Writes the migrated config back once, so the app reads and keeps the new rules too.
fn migrate_file(path: &Path) {
    let Some(mut v) = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else {
        return;
    };
    if v.is_object() && migrate(&mut v) {
        crate::write_atomic(path, &serde_json::to_vec_pretty(&v).unwrap_or_default());
        crate::log!("config: added the Music channel and its rules");
    }
}

/// A broken file is reported and left untouched, never overwritten with defaults.
pub fn load_or_create(path: &Path) -> Config {
    if path.exists() {
        migrate_file(path);
        return load(path).unwrap_or_else(|e| {
            crate::log!("{} unreadable ({e}), using defaults", path.display());
            Config::default()
        });
    }
    let cfg = Config::default();
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(path, serde_json::to_string_pretty(&cfg).unwrap_or_default())
    };
    if let Err(e) = write() {
        crate::log!("cannot write {}: {e}", path.display());
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_first_match_wins() {
        let c = Config::default();
        // Discord's voice stream reports Chromium-ish names but the Discord binary
        assert_eq!(c.channel_for(&[Some("Discord"), Some("Chromium"), None]), Some(1));
        assert_eq!(c.channel_for(&[Some("firefox"), None, None]), Some(2));
        assert_eq!(c.channel_for(&[Some("spotify"), None, None]), Some(3));
        assert_eq!(c.channel_for(&[Some("wine64-preloader"), None, None]), Some(0));
        assert_eq!(c.channel_for(&[Some("gnome-shell"), None, None]), None);
    }

    #[test]
    fn user_rules_win_and_survive_autopilot_off() {
        let mut c = Config::default();
        c.rules.insert(0, Rule { pattern: "firefox".into(), channel: "aux".into(), user: true });
        assert_eq!(c.channel_for(&[Some("firefox"), None, None]), Some(4));
        c.auto = false;
        assert_eq!(c.channel_for(&[Some("firefox"), None, None]), Some(4));
        assert_eq!(c.channel_for(&[Some("discord"), None, None]), None);
        c.auto = true;
        c.auto_route = false;
        assert_eq!(c.channel_for(&[Some("spotify"), None, None]), None);
    }

    #[test]
    fn old_config_gets_the_music_rules_and_keeps_user_rules_and_unknown_keys() {
        let mut v: serde_json::Value = serde_json::from_str(
            r#"{"eq_presets":{"Mine":[1,0,0,0,0,0,0,0,0,0]},
                "rules":[{"match":"spotify","channel":"aux","user":true},{"match":"mytool","channel":"chat"},
                         {"match":"rhythmbox","channel":"media"},{"match":"chromium","channel":"media"}]}"#,
        )
        .unwrap();
        assert!(migrate(&mut v));
        assert!(!migrate(&mut v), "runs once");
        assert_eq!(v["eq_presets"]["Mine"][0], 1);
        let c: Config = serde_json::from_value(v).unwrap();
        assert_eq!(c.channel_for(&[Some("spotify"), None, None]), Some(4), "user rule still wins");
        assert_eq!(c.channel_for(&[Some("mytool"), None, None]), Some(1), "hand-written rule stays");
        assert_eq!(c.channel_for(&[Some("rhythmbox"), None, None]), Some(3));
        // music rules sit before the browsers, so an Electron player reporting Chromium still goes to Music
        assert_eq!(c.channel_for(&[Some("tidal-hifi"), Some("Chromium"), None]), Some(3));
        assert_eq!(c.channel_for(&[Some("chromium"), None, None]), Some(2));
        assert_eq!(parse(r#"{"rules":[{"match":"spotify","channel":"media"}]}"#).unwrap().channel_for(&[Some("spotify"), None, None]), Some(3));
    }

    #[test]
    fn channel_curves_from_built_ins_edits_and_own_presets() {
        let c = parse(r#"{"eq_presets":{"Mine":[1,2,3,4,5,6,7,8,9,10],"Short":[1]},"eq_edits":{"bass":[2,0,0,0,0,0,0,0,0,0]}}"#).unwrap();
        assert_eq!(c.curve("gaming"), EQ_BUILTIN[3].1);
        assert_eq!(c.curve("bass")[0], 2.0);
        assert_eq!(c.curve("Mine")[9], 10.0);
        assert_eq!(c.curve("Short"), [0.0; 10]);
        assert_eq!(c.curve("gone"), [0.0; 10]);
    }

    #[test]
    fn night_window_wraps_midnight() {
        let n = Night { enabled: true, from: 22, to: 7 };
        assert!(n.active_at(23) && n.active_at(0) && n.active_at(6));
        assert!(!n.active_at(7) && !n.active_at(12) && !n.active_at(21));
        let d = Night { enabled: true, from: 9, to: 17 };
        assert!(d.active_at(9) && !d.active_at(17));
        assert!(!Night { enabled: false, ..n }.active_at(23));
    }

    #[test]
    fn partial_and_bad_config() {
        let c: Config = serde_json::from_str(r#"{"chat":{"volume":40}}"#).unwrap();
        assert_eq!(c.chat.volume, 40.0);
        assert!(!c.chat.mute);
        assert_eq!(c.master, 100.0);
        let c: Config = serde_json::from_str(r#"{"rules":[{"match":"","channel":"game"},{"match":"x","channel":"nope"},{"match":"x","channel":"game"}]}"#).unwrap();
        // first match decides, "nope" keeps it out even though a later rule would match
        assert_eq!(c.channel_for(&[Some("anything x"), None, None]), None);
        assert!(parse("[]").is_err());
        assert!(parse("null").is_err());
        assert!(parse("{").is_err());
        assert!(parse(r#"{"master":1e309}"#).is_err());
        assert_eq!(parse("{}").unwrap().master, 100.0);
    }
}
