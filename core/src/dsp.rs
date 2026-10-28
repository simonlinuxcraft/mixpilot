//! Realtime-safe DSP building blocks. No allocation after construction.

pub const RATE: f32 = 48_000.0;
const TAU: f32 = 2.0 * std::f32::consts::PI;

/// RBJ cookbook biquad, transposed direct form II, one instance per audio channel.
#[derive(Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub const fn identity() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 }
    }

    fn norm(b: [f32; 3], a: [f32; 3]) -> Self {
        Self { b0: b[0] / a[0], b1: b[1] / a[0], b2: b[2] / a[0], a1: a[1] / a[0], a2: a[2] / a[0], z1: 0.0, z2: 0.0 }
    }

    pub fn low_shelf(freq: f32, gain_db: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let (sin, cos) = (TAU * freq / RATE).sin_cos();
        // shelf slope S = 1
        let sq = 2.0 * a.sqrt() * (sin / 2.0 * 2f32.sqrt());
        Self::norm(
            [a * ((a + 1.0) - (a - 1.0) * cos + sq), 2.0 * a * ((a - 1.0) - (a + 1.0) * cos), a * ((a + 1.0) - (a - 1.0) * cos - sq)],
            [(a + 1.0) + (a - 1.0) * cos + sq, -2.0 * ((a - 1.0) + (a + 1.0) * cos), (a + 1.0) + (a - 1.0) * cos - sq],
        )
    }

    pub fn high_shelf(freq: f32, gain_db: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let (sin, cos) = (TAU * freq / RATE).sin_cos();
        let sq = 2.0 * a.sqrt() * (sin / 2.0 * 2f32.sqrt());
        Self::norm(
            [a * ((a + 1.0) + (a - 1.0) * cos + sq), -2.0 * a * ((a - 1.0) + (a + 1.0) * cos), a * ((a + 1.0) + (a - 1.0) * cos - sq)],
            [(a + 1.0) - (a - 1.0) * cos + sq, 2.0 * ((a - 1.0) - (a + 1.0) * cos), (a + 1.0) - (a - 1.0) * cos - sq],
        )
    }

    pub fn peaking(freq: f32, gain_db: f32, q: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let (sin, cos) = (TAU * freq / RATE).sin_cos();
        let alpha = sin / (2.0 * q);
        Self::norm([1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a], [1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a])
    }

    pub fn high_pass(freq: f32) -> Self {
        let (sin, cos) = (TAU * freq / RATE).sin_cos();
        let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        Self::norm([(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0], [1.0 + alpha, -2.0 * cos, 1.0 - alpha])
    }

    pub fn low_pass(freq: f32) -> Self {
        let (sin, cos) = (TAU * freq / RATE).sin_cos();
        let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        Self::norm([(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0], [1.0 + alpha, -2.0 * cos, 1.0 - alpha])
    }

    /// Swap coefficients but keep the filter state, so parameter changes do not click.
    pub fn retune(&mut self, other: Biquad) {
        let (z1, z2) = (self.z1, self.z2);
        *self = other;
        self.z1 = z1;
        self.z2 = z2;
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = flush(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = flush(self.b2 * x - self.a2 * y);
        y
    }
}

/// Microphone voice presets: 0 natural, 1 warm, 2 radio, 3 broadcast, 4 podcast, 5 laptop microphone.
pub const VOICE_PRESETS: [&str; 6] = ["natural", "warm", "radio", "broadcast", "podcast", "laptop"];

/// Up to five filters, enough for presets that are clearly audible, not just a nudge.
pub type Chain5 = [Biquad; 5];

#[inline]
pub fn run5(c: &mut Chain5, x: f32) -> f32 {
    let mut y = x;
    for f in c.iter_mut() {
        y = f.run(y);
    }
    y
}

pub fn voice_preset(i: u32) -> Chain5 {
    let id = Biquad::identity();
    match i {
        // fuller and softer, like a ribbon microphone: body below 200 Hz up, the top rolled off gently
        // from 8 kHz instead of dulled from 6 kHz
        1 => [Biquad::high_pass(70.0), Biquad::low_shelf(200.0, 4.5), Biquad::peaking(3000.0, -2.0, 0.8), Biquad::high_shelf(8000.0, -4.0), id],
        // telephone band: steep cut below 300 Hz and above 3.5 kHz, honky mids
        2 => [Biquad::high_pass(300.0), Biquad::high_pass(300.0), Biquad::peaking(1600.0, 6.0, 0.9), Biquad::high_shelf(3500.0, -14.0), Biquad::high_shelf(3500.0, -10.0)],
        // radio presenter: proximity bass, mud cut, presence and air (plus heavy compression and a
        // de-esser, see Leveler::broadcast and DeEsser). +5 dB at 120 Hz: men's voices have their
        // fundamental there already, more made them boom
        3 => [Biquad::high_pass(80.0), Biquad::peaking(120.0, 5.0, 0.8), Biquad::peaking(380.0, -5.0, 1.4), Biquad::peaking(4000.0, 6.0, 0.8), Biquad::high_shelf(9000.0, 5.0)],
        // close-miked podcast voice: full body below 170 Hz, a slight dip in the boxy 450 Hz, wide
        // presence and a little air (plus compression, see Leveler::podcast). Clearer than warm,
        // less boom and edge than broadcast
        4 => [Biquad::high_pass(70.0), Biquad::low_shelf(170.0, 5.0), Biquad::peaking(450.0, -2.0, 1.0), Biquad::peaking(3800.0, 2.5, 0.7), Biquad::high_shelf(11_000.0, 2.0)],
        // built-in laptop and webcam microphones: fan rumble and desk knocks out, the boxy room
        // sound down, the speech band forward, the harsh resonance of the case opening tamed
        5 => [Biquad::high_pass(120.0), Biquad::peaking(350.0, -3.0, 1.0), Biquad::peaking(2200.0, 4.0, 0.8), Biquad::peaking(6500.0, -3.0, 1.5), id],
        _ => [Biquad::high_pass(80.0), id, id, id, id],
    }
}

/// The compressor that belongs to a voice preset, it runs after the AGC.
pub fn voice_comp(i: u32) -> Option<Leveler> {
    match i {
        3 => Some(Leveler::broadcast()),
        4 => Some(Leveler::podcast()),
        _ => None,
    }
}

pub const EQ_FREQS: [f32; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

/// 10-band graphic EQ for one audio channel. Bands at 0 dB are skipped entirely.
pub struct Eq10 {
    bands: [Biquad; 10],
    gains: [f32; 10],
}

impl Default for Eq10 {
    fn default() -> Self {
        Self { bands: [Biquad::identity(); 10], gains: [0.0; 10] }
    }
}

impl Eq10 {
    pub fn set(&mut self, gains: &[f32; 10]) {
        for i in 0..10 {
            let g = gains[i].clamp(-12.0, 12.0);
            if g != self.gains[i] {
                if self.gains[i] == 0.0 {
                    self.bands[i] = Biquad::peaking(EQ_FREQS[i], g, 1.4);
                } else {
                    self.bands[i].retune(Biquad::peaking(EQ_FREQS[i], g, 1.4));
                }
                self.gains[i] = g;
            }
        }
    }

    #[inline]
    pub fn run(&mut self, mut x: f32) -> f32 {
        for i in 0..10 {
            if self.gains[i] != 0.0 {
                x = self.bands[i].run(x);
            }
        }
        x
    }
}

/// One-pole smoother for gains, avoids zipper noise when a fader moves.
#[derive(Clone, Copy)]
pub struct Smooth {
    pub value: f32,
    coef: f32,
}

impl Smooth {
    pub fn new(value: f32, ms: f32) -> Self {
        Self { value, coef: coef(ms) }
    }
    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        self.value += (target - self.value) * self.coef;
        if (target - self.value).abs() < 1e-6 {
            self.value = target;
        }
        self.value
    }
}

fn coef(ms: f32) -> f32 {
    1.0 - (-1.0 / (ms * 0.001 * RATE)).exp()
}

/// RMS compressor with makeup gain: evens out loud and quiet passages ("Auto-Lautstärke", "Auto-Pegel").
/// Near-silence below `floor_db` is never boosted, so hiss and room noise stay down.
#[derive(Clone, Copy)]
pub struct Leveler {
    ms: f32,
    detect: f32,
    attack: f32,
    release: f32,
    gain_db: f32,
    threshold: f32,
    ratio: f32,
    makeup: f32,
    floor_db: f32,
    // in near-silence keep the last gain instead of falling back to 0 dB, so the next syllable
    // starts already compressed
    hold: bool,
}

impl Leveler {
    pub fn new(threshold: f32, ratio: f32, makeup: f32, attack_ms: f32, release_ms: f32) -> Self {
        Self { ms: 0.0, detect: coef(80.0), attack: coef(attack_ms), release: coef(release_ms), gain_db: 0.0, threshold, ratio, makeup, floor_db: -55.0, hold: false }
    }

    /// 0 off, 1 soft, 2 night
    pub fn output_mode(mode: u32) -> Option<Self> {
        match mode {
            1 => Some(Self::new(-24.0, 2.5, 5.0, 60.0, 900.0)),
            2 => Some(Self::new(-34.0, 5.0, 12.0, 25.0, 400.0)),
            _ => None,
        }
    }

    /// Dense presenter sound, runs after the AGC so the input level is known (about -23 dBFS):
    /// around 10 dB of gain reduction with a fast detector, the way radio voice processors work.
    pub fn broadcast() -> Self {
        Self { detect: coef(10.0), hold: true, ..Self::new(-36.0, 6.0, 10.0, 3.0, 120.0) }
    }

    /// Studio compression after the AGC: 2.5:1 from 7 dB under the AGC target, dense but slow enough to stay unnoticed.
    pub fn podcast() -> Self {
        Self { detect: coef(25.0), hold: true, ..Self::new(-30.0, 2.5, 4.0, 8.0, 180.0) }
    }

    /// Returns the linear gain to apply to this sample (stereo-linked: feed the louder side).
    #[inline]
    pub fn gain(&mut self, x: f32) -> f32 {
        self.ms += (x * x - self.ms) * self.detect;
        let level = 10.0 * (self.ms + 1e-12).log10();
        let target = if level < self.floor_db {
            if self.hold {
                return 10f32.powf(self.gain_db / 20.0);
            }
            0.0
        } else {
            let over = level - self.threshold;
            self.makeup - if over > 0.0 { over * (1.0 - 1.0 / self.ratio) } else { 0.0 }
        };
        let k = if target < self.gain_db { self.attack } else { self.release };
        self.gain_db += (target - self.gain_db) * k;
        10f32.powf(self.gain_db / 20.0)
    }
}

/// Split-band de-esser: when the sibilant band stands out against the whole voice, only that band
/// is turned down, so S and Sch stop hissing without dulling the rest.
pub struct DeEsser {
    lp: Biquad,
    threshold: f32,
    floor: f32,
    env_hf: f32,
    env_all: f32,
    fall: f32,
    gain: f32,
    attack: f32,
    release: f32,
}

impl DeEsser {
    /// 1 normal: S above 5 kHz, at most 12 dB down. 2 strong: from 3.5 kHz, so Sch too, at most 18 dB.
    pub fn new(strength: u32) -> Self {
        let (split, threshold, floor) = if strength >= 2 { (3500.0, 0.2, 0.125) } else { (5000.0, 0.4, 0.25) };
        Self { lp: Biquad::low_pass(split), threshold, floor, env_hf: 0.0, env_all: 0.0, fall: coef(30.0), gain: 1.0, attack: coef(1.0), release: coef(80.0) }
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        // the band is the input minus its low part, so turning it down never shifts the phase of the rest
        let s = x - self.lp.run(x);
        self.env_hf = s.abs().max(self.env_hf - self.env_hf * self.fall);
        self.env_all = x.abs().max(self.env_all - self.env_all * self.fall);
        // the sibilant band may reach `threshold` of the voice before it is pulled down
        let ratio = self.env_hf / (self.env_all + 1e-6);
        let target = if ratio > self.threshold { (self.threshold / ratio).max(self.floor) } else { 1.0 };
        let k = if target < self.gain { self.attack } else { self.release };
        self.gain += (target - self.gain) * k;
        x - s * (1.0 - self.gain)
    }
}

/// Level the AGC steers toward: RMS of the last 300 ms of sound, in dBFS.
const AGC_TARGET: f32 = -23.0;
/// How far a single frame may stand above the target before it is turned down at once.
const AGC_BURST: f32 = 8.0;
/// Frames of sound in a row (40 ms) before they count as a voice: syllables last longer, clicks not.
const AGC_RUN: u32 = 4;

fn per_frame(ms: f32) -> f32 {
    1.0 - (-10.0 / ms).exp()
}

/// Automatic gain control for speech, decided once per 10 ms frame: steers the RMS of the last
/// 300 ms toward AGC_TARGET, turns down within half a second and up within 0.6 s. It holds still
/// while only the microphone's own noise floor is there, so pauses never lift the gain and the next
/// word does not start too loud. A shout is turned down within its own frame, before the limiter
/// would have to flatten it, and counts only as loud as that, so a laugh barely lowers the voice after it.
pub struct Agc {
    ms: f32,
    floor_db: f32,
    /// frames since the voice was last heard, and their summed power
    quiet: u32,
    gap: f32,
    /// sound frames in a row; the first ones wait in `pend` until the run is long enough to be a voice
    run: u32,
    pend: f32,
    /// voice frames in a row above the burst level
    loud: u32,
    want: f32,
    gain_db: f32,
    guard_db: f32,
    target: f32,
    out: Smooth,
    detect: f32,
    up: f32,
    down: f32,
}

impl Agc {
    pub fn voice() -> Self {
        Self {
            ms: 10f32.powf(AGC_TARGET / 10.0),
            floor_db: 0.0,
            quiet: 0,
            gap: 0.0,
            run: 0,
            pend: 0.0,
            loud: 0,
            want: 0.0,
            gain_db: 0.0,
            guard_db: 0.0,
            target: 1.0,
            out: Smooth::new(1.0, 2.0),
            detect: per_frame(300.0),
            up: per_frame(600.0),
            down: per_frame(500.0),
        }
    }

    /// A new microphone session or other settings in front of the AGC: the noise floor is learned
    /// again from the next frame, the voice level is kept.
    pub fn restart(&mut self) {
        (self.floor_db, self.quiet, self.gap, self.run, self.pend) = (0.0, 30, 0.0, 0, 0.0);
    }

    /// Once per frame, before its samples: mean square of the frame as the gate let it through,
    /// and before the gate.
    pub fn frame(&mut self, power: f32, before_gate: f32) {
        let level = 10.0 * (power + 1e-12).log10();
        let raw = 10.0 * (before_gate + 1e-12).log10();
        // the microphone's own noise floor, seen before the gate (behind it pauses are digital
        // silence): follows quieter frames at once, creeps up 2 dB/s
        self.floor_db = raw.min(self.floor_db + 0.02);
        // a laugh counts only as loud as the guard lets it out; a whole second that loud is a new
        // level (system volume turned up, another microphone) and counts in full
        let burst = if self.loud < 100 { 10f32.powf((AGC_TARGET + AGC_BURST - self.gain_db) / 10.0) } else { f32::MAX };
        // sound: clearly above the microphone's noise before the gate (a gate's hold time and closing
        // tail only carry that noise, so they never count) and let through by the gate
        if raw > self.floor_db + 10.0 && level > -65.0 {
            self.run += 1;
            if self.run < AGC_RUN {
                self.pend += power.min(burst);
            } else {
                if self.run == AGC_RUN {
                    // a gap between words counts like in any loudness reading, once the voice goes on
                    // within 300 ms; a longer pause is left out
                    if self.quiet > 0 && self.quiet < 30 {
                        self.mix(self.gap / self.quiet as f32, self.quiet);
                    }
                    self.mix(self.pend / (AGC_RUN - 1) as f32, AGC_RUN - 1);
                    (self.quiet, self.gap, self.pend) = (0, 0.0, 0.0);
                }
                self.mix(power.min(burst), 1);
                self.want = (AGC_TARGET - 10.0 * self.ms.log10()).clamp(-20.0, 15.0);
                self.loud = if level > AGC_TARGET + AGC_BURST - self.gain_db { self.loud + 1 } else { 0 };
            }
        } else {
            // shorter than a syllable: a click or a knock, it counts as part of the gap without its power
            if self.run < AGC_RUN {
                self.quiet += self.run;
            }
            (self.run, self.pend) = (0, 0.0);
            self.quiet = self.quiet.saturating_add(1);
            if self.quiet < 30 {
                self.gap += power;
            }
        }
        // keeps gliding in pauses, but only toward what the voice asked for
        self.gain_db += (self.want - self.gain_db) * if self.want < self.gain_db { self.down } else { self.up };
        // given back at 30 dB/s
        self.guard_db = (AGC_TARGET + AGC_BURST - level - self.gain_db).min(self.guard_db + 0.3).min(0.0);
        self.target = 10f32.powf((self.gain_db + self.guard_db) / 20.0);
    }

    /// `n` frames of mean square `p` into the 300 ms loudness reading
    fn mix(&mut self, p: f32, n: u32) {
        let keep = (1.0 - self.detect).powi(n as i32);
        // a reading of pure silence would be 0; never let it reach the log
        self.ms = (self.ms * keep + p * (1.0 - keep)).max(1e-10);
    }

    #[inline]
    pub fn gain(&mut self) -> f32 {
        self.out.next(self.target)
    }
}

/// Noise gate with an automatic threshold: it tracks the noise floor as the minimum loudness of the
/// last ~2 s (speech always has pauses) and opens 15 dB above it, between -70 and -25 dBFS. A hold time
/// keeps word endings, the gate then closes to -60 dB, so pauses are really silent.
pub struct Gate {
    env: f32,
    gain: f32,
    open: f32,
    close: f32,
    env_rel: f32,
    hold: u32,
    held: u32,
    block_sum: f32,
    block_n: u32,
    mins: [f32; 40],
    min_pos: usize,
    pub threshold: f32,
}

const GATE_BLOCK: u32 = 2400; // 50 ms

impl Gate {
    pub fn new() -> Self {
        Self {
            env: 0.0,
            gain: 1.0,
            open: coef(2.0),
            close: coef(120.0),
            env_rel: coef(60.0),
            hold: (RATE * 0.15) as u32,
            held: 0,
            block_sum: 0.0,
            block_n: 0,
            mins: [1.0; 40],
            min_pos: 0,
            threshold: 10f32.powf(-50.0 / 20.0),
        }
    }

    /// Level-only gate (noise suppression off).
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        self.track(x);
        self.decide(self.env > self.threshold, x)
    }

    /// With RNNoise running, both must agree: its voice detection says speech (so typing and clicks
    /// stay out) and the level is clearly above the noise floor (so steady noise that fools the
    /// detection stays out too).
    #[inline]
    pub fn run_voice(&mut self, x: f32, voice: bool) -> f32 {
        self.track(x);
        self.decide(voice && self.env > self.threshold, x)
    }

    #[inline]
    fn track(&mut self, x: f32) {
        let a = x.abs();
        self.env = if a > self.env { a } else { self.env + (a - self.env) * self.env_rel };
        // noise floor: RMS of 50 ms blocks, minimum over the last 40 blocks
        self.block_sum += x * x;
        self.block_n += 1;
        if self.block_n == GATE_BLOCK {
            let rms = (self.block_sum / GATE_BLOCK as f32).sqrt();
            self.mins[self.min_pos] = rms;
            self.min_pos = (self.min_pos + 1) % self.mins.len();
            self.block_sum = 0.0;
            self.block_n = 0;
            let floor = self.mins.iter().copied().fold(f32::MAX, f32::min);
            // peak envelope sits ~3 dB above RMS for noise, so +12 dB over the RMS floor
            self.threshold = (floor * 4.0 * 1.41).clamp(10f32.powf(-70.0 / 20.0), 10f32.powf(-25.0 / 20.0));
        }
    }

    #[inline]
    fn decide(&mut self, open: bool, x: f32) -> f32 {
        if open {
            self.held = self.hold;
        } else {
            self.held = self.held.saturating_sub(1);
        }
        let target = if self.held > 0 { 1.0 } else { 0.001 };
        let k = if target > self.gain { self.open } else { self.close };
        self.gain = flush(self.gain + (target - self.gain) * k);
        x * self.gain
    }
}

/// Peak envelope follower, used to detect speech in the chat channel for ducking.
pub struct Envelope {
    pub value: f32,
    rel: f32,
}

impl Envelope {
    pub fn new(release_ms: f32) -> Self {
        Self { value: 0.0, rel: coef(release_ms) }
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let a = x.abs();
        self.value = if a > self.value { a } else { flush(self.value + (a - self.value) * self.rel) };
        self.value
    }
}

const LOOKAHEAD: usize = 48; // 1 ms

/// Stereo peak limiter with 1 ms lookahead: the gain is already down when a peak leaves the delay,
/// so peaks are shaped smoothly instead of being clipped. A final clamp is only a safety net.
pub struct Limiter {
    ceiling: f32,
    gain: f32,
    attack: f32,
    release: f32,
    target: f32,
    hold: usize,
    delay: [(f32, f32); LOOKAHEAD],
    pos: usize,
}

impl Limiter {
    pub fn new(ceiling_db: f32, release_ms: f32) -> Self {
        Self {
            ceiling: 10f32.powf(ceiling_db / 20.0),
            gain: 1.0,
            // reaches the target well within the 48-sample lookahead, but over ~10 samples instead of one
            attack: coef(0.1),
            release: coef(release_ms),
            target: 1.0,
            hold: 0,
            delay: [(0.0, 0.0); LOOKAHEAD],
            pos: 0,
        }
    }

    #[inline]
    pub fn run(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (l, r) = (sanitize(l), sanitize(r));
        let peak = l.abs().max(r.abs());
        let need = if peak > self.ceiling { self.ceiling / peak } else { 1.0 };
        // the lowest gain needed by anything still inside the delay line
        if need <= self.target {
            self.target = need;
            self.hold = LOOKAHEAD;
        } else if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.target = need;
        }
        let k = if self.target < self.gain { self.attack } else { self.release };
        self.gain += (self.target - self.gain) * k;
        if (self.gain - self.target).abs() < 1e-4 {
            self.gain = self.target;
        }
        let (dl, dr) = std::mem::replace(&mut self.delay[self.pos], (l, r));
        self.pos = (self.pos + 1) % LOOKAHEAD;
        let c = self.ceiling;
        ((dl * self.gain).clamp(-c, c), (dr * self.gain).clamp(-c, c))
    }
}

/// Denormals cost up to 100x per operation on some CPUs; silence decays into them.
#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// One NaN/inf from a client would latch a filter state forever. Huge finite values would overflow it.
#[inline]
pub fn sanitize(x: f32) -> f32 {
    if x.is_finite() { x.clamp(-16.0, 16.0) } else { 0.0 }
}

/// Fader position 0..=100 to linear gain, cubic taper (0 = silence, 100 = unity).
pub fn fader_to_gain(pos: f32) -> f32 {
    let p = (pos / 100.0).clamp(0.0, 1.0);
    p * p * p
}

/// Microphone gain 0..=200 %: below 100 like a fader, above 100 up to +18 dB boost for quiet mics.
pub fn mic_gain(pct: f32) -> f32 {
    let p = pct.clamp(0.0, 200.0);
    if p <= 100.0 { fader_to_gain(p) } else { 10f32.powf((p - 100.0) / 100.0 * 18.0 / 20.0) }
}

/// ChatMix -100..=100: positive turns Game down, negative turns Chat down.
pub fn chatmix(mix: f32) -> (f32, f32) {
    let m = (mix / 100.0).clamp(-1.0, 1.0);
    (if m > 0.0 { 1.0 - m } else { 1.0 }, if m < 0.0 { 1.0 + m } else { 1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db_to_gain(db: f32) -> f32 {
        10f32.powf(db / 20.0)
    }

    fn sine_gain(mut f: impl FnMut(f32) -> f32, freq: f32) -> f32 {
        let n = (RATE as usize) / 2;
        let mut peak = 0f32;
        for i in 0..n {
            let x = (2.0 * std::f32::consts::PI * freq * i as f32 / RATE).sin();
            let y = f(x);
            if i > n / 2 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn low_shelf_boosts_bass_only() {
        let mut bq = Biquad::low_shelf(100.0, 12.0);
        let low = sine_gain(|x| bq.run(x), 30.0);
        let mut bq = Biquad::low_shelf(100.0, 12.0);
        let high = sine_gain(|x| bq.run(x), 5_000.0);
        assert!((low - db_to_gain(12.0)).abs() < 0.3, "low {low}");
        assert!((high - 1.0).abs() < 0.05, "high {high}");
    }

    #[test]
    fn high_shelf_peaking_and_high_pass() {
        let mut bq = Biquad::high_shelf(5000.0, 6.0);
        assert!((sine_gain(|x| bq.run(x), 15_000.0) - db_to_gain(6.0)).abs() < 0.15);
        let mut bq = Biquad::high_shelf(5000.0, 6.0);
        assert!((sine_gain(|x| bq.run(x), 100.0) - 1.0).abs() < 0.05);
        let mut bq = Biquad::peaking(1000.0, -6.0, 1.4);
        assert!((sine_gain(|x| bq.run(x), 1000.0) - db_to_gain(-6.0)).abs() < 0.03);
        let mut bq = Biquad::peaking(1000.0, -6.0, 1.4);
        assert!((sine_gain(|x| bq.run(x), 100.0) - 1.0).abs() < 0.05);
        let mut bq = Biquad::high_pass(100.0);
        assert!(sine_gain(|x| bq.run(x), 20.0) < 0.1);
        let mut bq = Biquad::high_pass(100.0);
        assert!((sine_gain(|x| bq.run(x), 2000.0) - 1.0).abs() < 0.02);
    }

    #[test]
    fn zero_db_shelf_is_transparent() {
        let mut bq = Biquad::low_shelf(100.0, 0.0);
        assert!((sine_gain(|x| bq.run(x), 60.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn eq10_skips_flat_bands_and_boosts_set_ones() {
        let mut eq = Eq10::default();
        assert_eq!(sine_gain(|x| eq.run(x), 440.0) - 1.0, 0.0);
        let mut g = [0.0; 10];
        g[5] = 6.0;
        eq.set(&g);
        assert!((sine_gain(|x| eq.run(x), 1000.0) - db_to_gain(6.0)).abs() < 0.1);
        g[5] = 99.0; // clamped to +12
        eq.set(&g);
        assert!(sine_gain(|x| eq.run(x), 1000.0) < db_to_gain(12.5));
    }

    #[test]
    fn presets_are_finite_and_flat_is_identity() {
        for i in 0..VOICE_PRESETS.len() as u32 {
            let mut c = voice_preset(i);
            let g = sine_gain(|x| run5(&mut c, x), 1000.0);
            assert!(g.is_finite() && g > 0.4 && g < 3.0, "voice {i}: {g}");
        }
        let mut flat = Eq10::default();
        flat.set(&[0.0; 10]);
        assert_eq!(flat.run(0.37), 0.37);
    }

    #[test]
    fn leveler_lifts_quiet_and_tames_loud() {
        let mut lv = Leveler::output_mode(2).unwrap();
        let quiet = sine_gain(|x| x * 0.03 * lv.gain(x * 0.03), 440.0) / 0.03;
        let mut lv = Leveler::output_mode(2).unwrap();
        let loud = sine_gain(|x| x * 0.9 * lv.gain(x * 0.9), 440.0) / 0.9;
        assert!(quiet > 2.0, "quiet gain {quiet}");
        assert!(loud < 0.8, "loud gain {loud}");
        let mut lv = Leveler::output_mode(1).unwrap();
        let silence = sine_gain(|x| x * 0.0005 * lv.gain(x * 0.0005), 440.0) / 0.0005;
        assert!((silence - 1.0).abs() < 0.05, "near-silence must not be boosted: {silence}");
        assert!(Leveler::output_mode(0).is_none());
    }

    // 1 s of 440 Hz at `amp` with a pause in every second 100 ms block, like speech
    fn speechy(amp: f32, i: usize) -> f32 {
        if (i / 4800) % 2 == 1 { 0.0 } else { amp * (TAU * 440.0 * i as f32 / RATE).sin() }
    }

    #[test]
    fn gate_adapts_to_the_noise_floor() {
        let mut rng = 1u32;
        let mut noise = |amp: f32| {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            amp * ((rng >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
        };
        // background hum at about -46 dBFS, above the old fixed -52 dB threshold
        let mut g = Gate::new();
        let mut noise_peak = 0f32;
        let mut voice_peak = 0f32;
        for i in 0..(48_000 * 6) {
            let n = noise(0.009);
            let v = if i >= 48_000 * 4 { speechy(0.2, i) } else { 0.0 };
            let y = g.run(n + v);
            if (48_000 * 3..48_000 * 4).contains(&i) {
                noise_peak = noise_peak.max(y.abs());
            }
            if i >= 48_000 * 5 {
                voice_peak = voice_peak.max(y.abs());
            }
        }
        assert!(noise_peak < 0.009 * 0.01, "noise alone must be gated to -40 dB or more: {noise_peak}");
        assert!(voice_peak > 0.18, "speech over the noise must pass: {voice_peak}");
    }

    #[test]
    fn gate_keeps_quiet_speech_in_a_quiet_room() {
        let mut g = Gate::new();
        let mut peak = 0f32;
        for i in 0..(48_000 * 4) {
            let y = g.run(speechy(0.01, i)); // -40 dBFS speech, silent room
            if i > 48_000 * 2 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak > 0.009, "quiet speech was cut: {peak}");
    }

    #[test]
    fn gate_follows_voice_detection() {
        let mut g = Gate::new();
        // loud keyboard but no speech for 2 s; closing is smooth (120 ms), so look at the end
        let mut closed = 1.0f32;
        for _ in 0..(48_000 * 2) {
            closed = (g.run_voice(0.3, false) / 0.3).abs();
        }
        assert!(closed <= 0.0011, "not muted: {closed}");
        let mut open = 0.0f32;
        for _ in 0..4800 {
            open = g.run_voice(0.3, true) / 0.3;
        }
        assert!(open > 0.99, "speech not passed: {open}");
    }

    #[test]
    fn gate_with_voice_detection_still_rejects_steady_noise() {
        // steady fan noise around -45 dBFS RMS that the detector mistakes for speech half of the time
        let mut rng = 7u32;
        let mut g = Gate::new();
        let mut peak = 0f32;
        for i in 0..(48_000 * 4) {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let n = 0.01 * ((rng >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0);
            let y = g.run_voice(n, (i / 480) % 2 == 0);
            if i > 48_000 * 3 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak < 0.01 * 0.01, "steady noise leaked: {peak}");
    }

    #[test]
    fn gate_holds_word_endings() {
        let mut g = Gate::new();
        for i in 0..48_000 {
            g.run(speechy(0.2, i % 4800)); // settle with voice on
        }
        // voice stops: 100 ms later the gate must still be (nearly) open
        let mut y = 0.0;
        for i in 0..4800 {
            y = g.run(0.1 * (TAU * 440.0 * i as f32 / RATE).sin().signum());
        }
        let _ = y;
        let mut g2 = Gate::new();
        for i in 0..48_000 {
            g2.run(speechy(0.2, i % 4800));
        }
        for _ in 0..(48_000 / 10) {
            g2.run(0.0);
        }
        assert!(g2.gain > 0.9, "gate closed during hold: {}", g2.gain);
    }

    #[test]
    fn broadcast_compresses_hard() {
        // speech-like bursts 12 dB apart around the AGC target end up much closer together
        // held level of the bursts, after the first 20 ms of each, so the onset before the attack does not count
        let level = |amp: f32| {
            let mut c = Leveler::broadcast();
            let (mut sum, mut n, mut peak) = (0f32, 0, 0f32);
            for i in 0..48000 {
                let x = speechy(amp, i);
                let y = x * c.gain(x);
                peak = peak.max(y.abs());
                if i > 24000 && (i / 4800) % 2 == 0 && i % 4800 > 960 {
                    sum += y * y;
                    n += 1;
                }
            }
            (10.0 * (sum / n as f32).log10(), 20.0 * peak.log10())
        };
        let ((quiet, _), (loud, loud_peak)) = (level(0.05), level(0.2));
        eprintln!("broadcast: 12 dB in, {:.1} dB out, loud {loud:.1} dBFS rms, peak {loud_peak:.1} dBFS", loud - quiet);
        assert!(loud - quiet < 4.0, "12 dB in, {:.1} dB out", loud - quiet);
        assert!(loud_peak < 0.0, "no clipping before the limiter: {loud_peak:.1} dBFS");
    }

    #[test]
    fn podcast_compresses_gently() {
        let spread = |mut c: Leveler| {
            let mut level = |amp: f32| {
                let (mut sum, mut n) = (0f32, 0);
                for i in 0..48000 {
                    let x = speechy(amp, i);
                    let y = x * c.gain(x);
                    if i > 24000 && (i / 4800) % 2 == 0 && i % 4800 > 960 {
                        sum += y * y;
                        n += 1;
                    }
                }
                10.0 * (sum / n as f32).log10()
            };
            let quiet = level(0.05);
            level(0.2) - quiet
        };
        let (podcast, broadcast) = (spread(Leveler::podcast()), spread(Leveler::broadcast()));
        eprintln!("12 dB in: podcast {podcast:.1} dB out, broadcast {broadcast:.1} dB out");
        assert!((4.0..8.0).contains(&podcast), "podcast {podcast:.1} dB");
        assert!(podcast > broadcast + 2.0, "podcast must stay lighter than broadcast");
        assert!(voice_comp(4).is_some() && voice_comp(3).is_some() && voice_comp(0).is_none() && voice_comp(5).is_none());
    }

    #[test]
    fn de_esser_tames_sibilants_only() {
        let run = |strength: u32, freq: f32| {
            let mut d = DeEsser::new(strength);
            20.0 * sine_gain(|x| d.run(x * 0.3) / 0.3, freq).log10()
        };
        eprintln!("de-esser normal: 300 Hz {:.1} dB, 7 kHz {:.1} dB", run(1, 300.0), run(1, 7000.0));
        eprintln!("de-esser strong: 300 Hz {:.1} dB, 4.5 kHz {:.1} dB, 7 kHz {:.1} dB", run(2, 300.0), run(2, 4500.0), run(2, 7000.0));
        for strength in [1, 2] {
            assert!(run(strength, 300.0).abs() < 1.0, "voice untouched: {:.1} dB", run(strength, 300.0));
            assert!(run(strength, 1000.0).abs() < 1.5, "voice band untouched: {:.1} dB", run(strength, 1000.0));
        }
        assert!(run(1, 7000.0) < -6.0, "sibilant down: {:.1} dB", run(1, 7000.0));
        assert!(run(2, 7000.0) < run(1, 7000.0) - 3.0, "strong is clearly stronger");
        assert!(run(2, 4500.0) < -6.0, "strong also catches Sch: {:.1} dB", run(2, 4500.0));
    }

    #[test]
    fn voice_presets_are_clearly_different() {
        let db = |i: u32, f: f32| {
            let mut c = voice_preset(i);
            20.0 * sine_gain(|x| run5(&mut c, x), f).log10()
        };
        let (natural, warm, radio, broadcast, podcast, laptop) = (0, 1, 2, 3, 4, 5);
        assert!(db(warm, 150.0) - db(natural, 150.0) > 3.0, "warm body");
        assert!(db(natural, 10_000.0) - db(warm, 10_000.0) > 2.5, "warm softer");
        assert!(db(natural, 5000.0) - db(warm, 5000.0) < 2.5, "warm not dull");
        assert!(db(natural, 150.0) - db(radio, 150.0) > 15.0, "radio low cut");
        assert!(db(natural, 7000.0) - db(radio, 7000.0) > 15.0, "radio high cut");
        assert!(db(broadcast, 4000.0) - db(natural, 4000.0) > 5.0, "broadcast presence");
        assert!(db(natural, 350.0) - db(broadcast, 350.0) > 2.5, "broadcast mud cut");
        assert!(db(broadcast, 120.0) - db(natural, 120.0) > 4.0, "broadcast proximity bass");
        assert!(db(podcast, 120.0) - db(natural, 120.0) > 3.0, "podcast full body");
        assert!(db(podcast, 3800.0) - db(natural, 3800.0) > 2.0, "podcast presence");
        assert!(db(natural, 450.0) - db(podcast, 450.0) > 1.0, "podcast box dip");
        assert!(db(podcast, 10_000.0) - db(natural, 10_000.0) > 1.0, "podcast air");
        assert!(db(broadcast, 4000.0) - db(podcast, 4000.0) > 2.5, "podcast less edgy than broadcast");
        assert!(db(podcast, 4000.0) - db(warm, 4000.0) > 3.0, "podcast clearer than warm");
        assert!(db(natural, 60.0) - db(laptop, 60.0) > 5.0, "laptop rumble cut");
        assert!(db(natural, 350.0) - db(laptop, 350.0) > 2.5, "laptop box cut");
        assert!(db(laptop, 2200.0) - db(natural, 2200.0) > 3.0, "laptop speech band");
        assert!(db(natural, 6500.0) - db(laptop, 6500.0) > 1.5, "laptop case resonance");
        for i in 0..VOICE_PRESETS.len() as u32 {
            assert!(db(i, 1000.0).abs() < 7.0, "preset {i} keeps the voice band level");
        }
    }

    #[test]
    fn envelope_tracks_and_decays_to_zero() {
        let mut e = Envelope::new(100.0);
        e.run(0.8);
        assert_eq!(e.value, 0.8);
        for _ in 0..(48_000 * 3) {
            e.run(0.0);
        }
        assert!(e.value < 1e-9, "{}", e.value);
    }

    #[test]
    fn nan_from_a_client_cannot_latch_the_filter() {
        let mut raw = Biquad::low_shelf(100.0, 6.0);
        raw.run(f32::NAN);
        assert!(raw.run(0.5).is_nan());

        let mut bq = Biquad::low_shelf(100.0, 6.0);
        for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30] {
            assert!(bq.run(sanitize(x)).is_finite());
        }
        let mut y = 0.0;
        for i in 0..48_000 {
            y = bq.run(sanitize(0.5 * (i as f32 * 0.01).sin()));
        }
        assert!(y.is_finite() && y.abs() < 2.0);
        assert_eq!(sanitize(1e30), 16.0);
        assert_eq!(sanitize(-0.25), -0.25);
    }

    #[test]
    fn silence_settles_to_exact_zero() {
        let mut s = Smooth::new(1.0, 20.0);
        for _ in 0..48_000 {
            s.next(0.0);
        }
        assert_eq!(s.value, 0.0);
        let mut bq = Biquad::low_shelf(100.0, 12.0);
        bq.run(1.0);
        for _ in 0..(48_000 * 3) {
            bq.run(0.0);
        }
        assert_eq!((bq.z1, bq.z2), (0.0, 0.0));
    }

    #[test]
    fn limiter_never_exceeds_ceiling_and_kills_nan() {
        let mut lim = Limiter::new(-1.0, 100.0);
        let c = db_to_gain(-1.0);
        for i in 0..10_000 {
            let x = 4.0 * (i as f32 * 0.05).sin();
            let (l, r) = lim.run(x, -x);
            assert!(l.abs() <= c + 1e-6 && r.abs() <= c + 1e-6);
        }
        for _ in 0..100 {
            let (l, r) = lim.run(f32::NAN, f32::INFINITY);
            assert!(l.is_finite() && r.is_finite() && l.abs() <= c + 1e-6);
        }
    }

    #[test]
    fn limiter_shapes_instead_of_clipping() {
        // 1 kHz sine 6 dB over the ceiling. Clipping shows up as flat tops: several samples in a row
        // stuck at the ceiling. A limiter that turns the gain down in time leaves round peaks.
        let mut lim = Limiter::new(-3.0, 100.0);
        let c = db_to_gain(-3.0);
        let (mut run, mut flat_tops) = (0, 0);
        for i in 0..48_000 {
            let x = 2.0 * c * (TAU * 1000.0 * i as f32 / RATE).sin();
            let (l, _) = lim.run(x, x);
            if i > 4800 && l.abs() >= c * 0.999 {
                run += 1;
                if run == 3 {
                    flat_tops += 1;
                }
            } else {
                run = 0;
            }
        }
        assert_eq!(flat_tops, 0, "{flat_tops} clipped peaks");
    }

    #[test]
    fn limiter_leaves_quiet_signal_alone() {
        let mut lim = Limiter::new(-1.0, 100.0);
        for i in 0..1000 {
            let out = lim.run(0.3, -0.3);
            if i >= LOOKAHEAD {
                assert_eq!(out, (0.3, -0.3));
            }
        }
    }

    fn frame_db(x: &[f32]) -> f32 {
        10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32 + 1e-12).log10()
    }

    /// Runs the AGC the way the microphone chain does, frame by frame.
    fn agc_frames(x: &[f32]) -> Vec<f32> {
        let mut agc = Agc::voice();
        let mut y = Vec::with_capacity(x.len());
        for frame in x.as_chunks::<480>().0 {
            let p = frame.iter().map(|v| v * v).sum::<f32>() / 480.0;
            agc.frame(p, p);
            y.extend(frame.iter().map(|v| v * agc.gain()));
        }
        y
    }

    #[test]
    fn agc_brings_quiet_and_loud_speech_to_the_target_without_overload() {
        // 0.03 needs 13.5 dB, close to the 15 dB the AGC may add
        for amp in [0.03, 0.1, 0.6, 0.95] {
            let x: Vec<f32> = (0..48_000 * 8).map(|i| speechy(amp, i)).collect();
            let y = agc_frames(&x);
            // the 100 ms gaps of speechy() count, as in any loudness reading
            let level = frame_db(&y[48_000 * 6..]);
            let peak = 20.0 * y[48_000 * 6..].iter().fold(0f32, |m, v| m.max(v.abs())).log10();
            assert!((level - AGC_TARGET).abs() < 1.5, "amp {amp}: {level:.1} dBFS");
            assert!(peak < -12.0, "amp {amp}: peak {peak:.1} dBFS, the limiter should have nothing to do");
        }
    }

    #[test]
    fn agc_does_not_creep_up_in_pauses() {
        // 8 s of talking, 3 s of room noise only, then talking again: the next word must not start louder
        let mut rng = 3u32;
        let x: Vec<f32> = (0..48_000 * 13)
            .map(|i| {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = 0.0005 * ((rng >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0);
                if (48_000 * 8..48_000 * 11).contains(&i) { noise } else { speechy(0.03, i) + noise }
            })
            .collect();
        let speech = |f: usize| !(800..1100).contains(&f) && (f / 10).is_multiple_of(2);
        let y = agc_frames(&x);
        let loudest = |from: usize, to: usize| (from..to).filter(|&f| speech(f)).map(|f| frame_db(&y[f * 480..(f + 1) * 480])).fold(-200f32, f32::max);
        let (before, after) = (loudest(700, 800), loudest(1100, 1130));
        assert!(after - before < 0.5, "first word after the pause {:.1} dB louder", after - before);
    }

    #[test]
    fn agc_ignores_what_the_gate_shuts_out() {
        // talking over room noise at -55 dBFS, then a 3 s pause with a few single clicks, then talking
        // again; the gate lets each click, its hold time and the noise in it through, the next word
        // must still not start louder. (Steady typing that passes the gate reads like chopped speech
        // to any level control, noise suppression is the tool for that.)
        let mut rng = 11u32;
        let x: Vec<f32> = (0..48_000 * 13)
            .map(|i| {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = 0.0031 * ((rng >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0);
                let ph = i % 43_200;
                let click = if ph < 192 { 0.3 * (-(ph as f32) / 40.0).exp() * ((rng >> 9) as f32 / (1u32 << 23) as f32 - 1.0) } else { 0.0 };
                if (48_000 * 8..48_000 * 11).contains(&i) { noise + click } else { speechy(0.1, i) + noise }
            })
            .collect();
        let (mut gate, mut agc) = (Gate::new(), Agc::voice());
        let mut y = Vec::with_capacity(x.len());
        for frame in x.as_chunks::<480>().0 {
            let gated: Vec<f32> = frame.iter().map(|&v| gate.run(v)).collect();
            let p = |v: &[f32]| v.iter().map(|s| s * s).sum::<f32>() / 480.0;
            agc.frame(p(&gated), p(frame));
            y.extend(gated.iter().map(|v| v * agc.gain()));
        }
        let speech = |f: usize| !(800..1100).contains(&f) && (f / 10).is_multiple_of(2);
        let loudest = |from: usize, to: usize| (from..to).filter(|&f| speech(f)).map(|f| frame_db(&y[f * 480..(f + 1) * 480])).fold(-200f32, f32::max);
        let (before, after) = (loudest(700, 800), loudest(1100, 1130));
        assert!(after - before < 1.0, "first word after typing {:.1} dB louder", after - before);
    }

    #[test]
    fn agc_learns_a_new_level_but_not_a_laugh() {
        let level_at = |y: &[f32], from_s: usize, to_s: usize| frame_db(&y[from_s * 48_000..to_s * 48_000]);
        // gain slider from 30 % to 200 %: 30 dB more, learned again within 6 s (the guard caps it meanwhile)
        let x: Vec<f32> = (0..48_000 * 14).map(|i| speechy(if i < 48_000 * 6 { 0.03 } else { 0.9 }, i)).collect();
        let y = agc_frames(&x);
        let after = level_at(&y, 12, 14);
        assert!((after - AGC_TARGET).abs() < 2.0, "new level not learned: {after:.1} dBFS");
        // a 0.6 s laugh 16 dB louder: the voice after it stays within 3 dB (the old AGC dropped it 5 dB and more)
        let x: Vec<f32> = (0..48_000 * 12).map(|i| speechy(if (48_000 * 6..48_000 * 6 + 28_800).contains(&i) { 0.19 } else { 0.03 }, i)).collect();
        let y = agc_frames(&x);
        let (before, after) = (level_at(&y, 4, 6), level_at(&y, 7, 9));
        assert!((after - before).abs() < 3.0, "a laugh moved the level: {before:.1} -> {after:.1} dBFS");
    }

    #[test]
    fn agc_catches_a_shout_within_its_frame() {
        // settled on normal speech, then suddenly 20 dB louder
        let x: Vec<f32> = (0..48_000 * 6).map(|i| speechy(if i < 48_000 * 5 { 0.03 } else { 0.3 }, i)).collect();
        let y = agc_frames(&x);
        // the first frame of the shout still glides down for 2 ms, the limiter takes that one
        let rest = &y[48_000 * 5 + 480..];
        let worst = rest.as_chunks::<480>().0.iter().map(|c| frame_db(c)).fold(-200f32, f32::max);
        let peak = 20.0 * rest.iter().fold(0f32, |m, v| m.max(v.abs())).log10();
        assert!(worst < AGC_TARGET + AGC_BURST + 1.0, "shout reaches {worst:.1} dBFS");
        assert!(peak < -6.0, "shout peaks at {peak:.1} dBFS");
    }

    #[test]
    fn faders_chatmix_and_mic_gain() {
        assert_eq!(fader_to_gain(0.0), 0.0);
        assert_eq!(fader_to_gain(100.0), 1.0);
        assert_eq!(fader_to_gain(250.0), 1.0);
        assert_eq!(chatmix(0.0), (1.0, 1.0));
        assert_eq!(chatmix(100.0), (0.0, 1.0));
        assert_eq!(chatmix(-50.0), (1.0, 0.5));
        assert_eq!(mic_gain(100.0), 1.0);
        assert!((mic_gain(200.0) - db_to_gain(18.0)).abs() < 0.01);
        assert_eq!(mic_gain(0.0), 0.0);
    }

    #[test]
    fn ui_shows_the_same_built_in_curves() {
        let js = include_str!("../../app/ui/app.js");
        for (name, bands) in crate::config::EQ_BUILTIN {
            let line = js.lines().find(|l| l.trim_start().starts_with(&format!("{name}: ["))).unwrap_or_else(|| panic!("{name} missing in EQ_BUILTIN of app.js"));
            let ui: Vec<f32> = line[line.find('[').unwrap() + 1..line.find(']').unwrap()].split(',').map(|v| v.trim().parse().unwrap()).collect();
            assert_eq!(ui, bands, "{name} differs between app.js and config.rs");
        }
    }

    #[test]
    fn retune_keeps_state() {
        let mut bq = Biquad::low_shelf(100.0, 6.0);
        for i in 0..100 {
            bq.run((i as f32).sin());
        }
        let before = (bq.z1, bq.z2);
        bq.retune(Biquad::low_shelf(100.0, 3.0));
        assert_eq!(before, (bq.z1, bq.z2));
    }
}
