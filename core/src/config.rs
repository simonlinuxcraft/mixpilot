use serde::{Deserialize, Serialize};
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

pub const CHANNELS: [ChannelDef; 4] = [
    ChannelDef { id: "game", label: "Game" },
    ChannelDef { id: "chat", label: "Chat" },
    ChannelDef { id: "media", label: "Media" },
    ChannelDef { id: "aux", label: "Aux" },
];

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Channel {
    pub volume: f32,
    pub mute: bool,
    /// sound preset, see dsp::CHANNEL_PRESETS
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
}

impl Default for Mic {
    fn default() -> Self {
        Self { gain: 100.0, mute: false, noise: "normal".into(), agc: true, gate: true, voice: "natural".into(), monitor: false }
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
    /// 10 bands in dB, -12..=12, see dsp::EQ_FREQS
    pub eq: [f32; 10],
    /// node.name of the hardware sink, empty = system default
    pub output: String,
    /// node.name of the microphone, empty = system default source
    pub input: String,
    pub mic: Mic,
    /// lower Media and Game while someone talks in Chat
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
            aux: Channel::default(),
            master: 100.0,
            chatmix: 0.0,
            bass: 0.0,
            limiter: true,
            clarity: false,
            auto_volume: "off".into(),
            eq: [0.0; 10],
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
                r("spotify", "media"),
                r("firefox", "media"),
                r("chromium", "media"),
                r("chrome", "media"),
                r("vlc", "media"),
                r("mpv", "media"),
                r("rhythmbox", "media"),
                r("youtube", "media"),
            ],
        }
    }
}

pub fn index_of(list: &[&str], name: &str) -> u32 {
    list.iter().position(|n| *n == name).unwrap_or(0) as u32
}

impl Config {
    pub fn channels(&self) -> [&Channel; 4] {
        [&self.game, &self.chat, &self.media, &self.aux]
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
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if !v.is_object() {
        return Err("not a JSON object".into());
    }
    serde_json::from_value(v).map_err(|e| e.to_string())
}

/// A broken file is reported and left untouched, never overwritten with defaults.
pub fn load_or_create(path: &Path) -> Config {
    if path.exists() {
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
        assert_eq!(c.channel_for(&[Some("wine64-preloader"), None, None]), Some(0));
        assert_eq!(c.channel_for(&[Some("gnome-shell"), None, None]), None);
    }

    #[test]
    fn user_rules_win_and_survive_autopilot_off() {
        let mut c = Config::default();
        c.rules.insert(0, Rule { pattern: "firefox".into(), channel: "aux".into(), user: true });
        assert_eq!(c.channel_for(&[Some("firefox"), None, None]), Some(3));
        c.auto = false;
        assert_eq!(c.channel_for(&[Some("firefox"), None, None]), Some(3));
        assert_eq!(c.channel_for(&[Some("discord"), None, None]), None);
        c.auto = true;
        c.auto_route = false;
        assert_eq!(c.channel_for(&[Some("spotify"), None, None]), None);
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
