const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
};

const ICONS = {
  game: 'M6 11h4M8 9v4M15 12h.01M18 10h.01M7 6h10a5 5 0 0 1 4.9 6l-.7 3.6A3 3 0 0 1 16 17l-1.5-1.5h-5L8 17a3 3 0 0 1-5.2-1.4L2.1 12A5 5 0 0 1 7 6z',
  chat: 'M21 12a8 8 0 0 1-11.7 7.1L4 20l1-4.6A8 8 0 1 1 21 12z',
  media: 'M3 5h18v12H3zM8 21h8M10 8.5l5 3-5 3z',
  music: 'M9 18V5l12-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM21 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0z',
  aux: 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM7.8 7.8a6 6 0 0 0 0 8.4M16.2 7.8a6 6 0 0 1 0 8.4M5 5a10 10 0 0 0 0 14M19 5a10 10 0 0 1 0 14',
  master: 'M11 5 6 9H2v6h4l5 4V5zM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14',
};
const CHANNELS = [['game', 'Game'], ['chat', 'Chat'], ['media', 'Media'], ['music', t('Musik')], ['aux', 'Aux']];
// same curves as EQ_BUILTIN in core/src/config.rs
const EQ_BUILTIN = {
  flat: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  bass: [6, 5, 3, 1, 0, 0, 0, 0, 0, 0],
  voice: [-3, -2, -1, 0, 2, 3, 3, 2, 0, -1],
  gaming: [3, 4, 2, 0, -1, 0, 1, 3, 2, 1],
  clear: [0, 0, -1, -1, 0, 1, 2, 3, 3, 2],
  music: [4, 4, 2, 0, -1, -1, 0, 1, 2, 2],
  film: [3, 2, 0, -1, -1, 1, 3, 2, 0, -1],
  laptop: [-6, -4, -1, 2, 2, 1, 0, 1, 1, 0],
};
const EQ_LABEL = { flat: 'Flat', bass: 'Bass', voice: t('Stimme'), gaming: 'Gaming', clear: t('Klar'), music: t('Musik'), film: t('Film'), laptop: t('Laptop') };
// more own presets would wrap the chips and push the sound tab into scrolling
const EQ_OWN_MAX = 4;
const isBuiltIn = (k) => Object.hasOwn(EQ_BUILTIN, k);
const presetLabel = (k) => (isBuiltIn(k) ? EQ_LABEL[k] : k);
const presetKeys = () => [...Object.keys(EQ_BUILTIN), ...Object.keys(cfg.eq_presets)];
const curveOf = (k) => (isBuiltIn(k) ? cfg.eq_edits[k] || EQ_BUILTIN[k] : cfg.eq_presets[k] || EQ_BUILTIN.flat);
// a channel whose own preset is gone plays flat, the core does the same
const chPreset = (id) => (isBuiltIn(cfg[id].eq) || cfg.eq_presets[cfg[id].eq] ? cfg[id].eq : 'flat');
// the built-in a name stands for: its key or a label in either language
const LABELS = { stimme: 'voice', klar: 'clear', musik: 'music', movie: 'film' };
const builtinFor = (n) => {
  const l = n.toLowerCase();
  return isBuiltIn(l) ? l : Object.hasOwn(LABELS, l) ? LABELS[l] : null;
};
const reservedName = (n) => builtinFor(n) !== null;
const curves = (o) => Object.fromEntries(Object.entries(o || {})
  .filter(([, g]) => Array.isArray(g) && g.length === 10 && g.every(Number.isFinite)));
const FREQ = ['31', '62', '125', '250', '500', '1k', '2k', '4k', '8k', '16k'];
const AUTOVOL = [
  ['off', t('Aus'), t('Kein Eingriff in die Lautstärke.')],
  ['soft', t('Sanft'), t('Gleicht Lautstärkesprünge zwischen Apps und Szenen behutsam aus.')],
  ['night', t('Nacht'), t('Leise Stellen lauter, Explosionen leiser. Für spät abends.')],
];
const NOISE = [['off', t('Aus')], ['normal', 'Normal'], ['strong', t('Stark')]];
const VOICE = [['natural', t('Natürlich')], ['warm', 'Warm'], ['radio', 'Radio'], ['broadcast', 'Broadcast'], ['podcast', t('Podcast')], ['laptop', t('Laptop-Mikro')]];
const VOICE_TEXT = {
  natural: t('Deine Stimme wie sie ist, nur Rumpeln unter 80 Hz wird entfernt.'),
  warm: t('Voller und weicher: mehr Körper, weniger Schärfe und Zischen.'),
  radio: t('Telefon- und Funkklang, nur das schmale Sprachband.'),
  broadcast: t('Moderatorenstimme: nah, präsent, dicht komprimiert.'),
  podcast: t('Nah besprochene Podcast-Stimme: satt, klar und dicht.'),
  laptop: t('Für eingebaute Mikros in Laptop und Webcam: weniger Raumklang und Brummen, besser verständlich.'),
};
// autopilot events arrive from the core as key + arguments
const EVENTS = {
  easyeffects: 'Easy Effects läuft und zieht alle Apps zu sich, bitte schließen',
  sorted: '{0} in {1} einsortiert',
  moved: '{0} in {1} verschoben',
  direct: '{0} spielt wieder direkt',
  night_on: 'Nachtmodus an',
  night_off: 'Nachtmodus aus',
  output: 'Ausgabe: {0}',
  input: 'Mikrofon: {0}',
  mic_active: 'Mikrofon aktiv',
  mic_paused: 'Mikrofon pausiert, niemand nutzt es',
  mic_gone: 'Kein Mikrofon mehr da, das Mixpilot Mikrofon ist still',
};

let cfg = null;
let tab = 'mixer';
// device choices made in the setup dialog before the core has written a config
const pending = { output: '', input: '' };
const devCfg = () => cfg || pending;
// Master is the output device's own volume (the one GNOME and the volume keys change), not a Mixpilot stage
const masterDev = { volume: 0, mute: false, known: false };
const MIC_MUTED = 'Das Mikrofon ist stumm. Klick oben auf "stumm", um es wieder einzuschalten.';
const MASTER_FAILED = 'Lautstärke des Ausgabegeräts konnte nicht geändert werden';
let masterPending = null;
let masterBusy = false;
let masterTouched = 0;
// One volume command at a time, always with the newest value: parallel pactl calls can finish out of
// order and make the fader jump back and forth.
async function setMaster(v) {
  masterDev.volume = v;
  masterTouched = Date.now();
  masterPending = v;
  if (masterBusy) return;
  masterBusy = true;
  while (masterPending !== null) {
    const next = masterPending;
    masterPending = null;
    await invoke('set_master', { volume: next }).then(() => {
      masterDev.known = true;
      clearError(MASTER_FAILED);
    }, showError);
  }
  masterBusy = false;
  masterTouched = Date.now();
}
let masterPolling = false;
async function pollMaster() {
  // never read back while the user is moving the fader or right after: the device may not be there yet
  // and never start a second read while one waits for a slow pipewire-pulse
  if (document.hidden || masterBusy || masterPolling || Date.now() - masterTouched < 1200) return;
  masterPolling = true;
  try {
    const m = await invoke('get_master');
    Object.assign(masterDev, m, { known: true });
  } catch {
    masterDev.known = false;
  } finally {
    masterPolling = false;
  }
  paintAll();
}
let devices = { sinks: [], sources: [], default_sink: '', default_source: '' };
const painters = [];
const paintAll = () => painters.forEach((p) => p());

// what to try next, so a message never ends at "failed"
const HINTS = {
  'Audio-Kern nicht gefunden': 'Die Installation ist unvollständig. Installier Mixpilot neu.',
  'Audio-Kern gestoppt': 'Klick oben auf "Audio-Kern starten" und versuch es dann nochmal.',
  'Standard-Mikrofon konnte nicht gesetzt werden': 'Prüf, ob das Mikrofon noch angesteckt ist, und wähle es oben bei Eingabe neu aus.',
  'Aufnahme fehlgeschlagen': 'Prüf, ob ein Mikrofon angesteckt und oben bei Eingabe gewählt ist. In der Snap-Version muss außerdem der Mikrofon-Zugriff freigegeben sein, das zeigt die Einrichtung.',
  'Wiedergabe fehlgeschlagen': 'Prüf, ob oben bei Ausgabe dein Headset oder deine Lautsprecher gewählt sind.',
  'Lautstärke des Ausgabegeräts konnte nicht geändert werden': 'Das Gerät wurde vielleicht gerade getrennt. Wähle oben bei Ausgabe ein verbundenes Gerät.',
  'Der Audio-Kern startet nicht wieder.': 'Klick oben auf "Audio-Kern starten". Hilft das nicht, schick einen Bericht mit dem Protokoll des Audio-Kerns, es zeigt meist die Ursache.',
};
const HINT_RULES = [
  [/os error 28/, 'Der Speicherplatz ist voll. Mach etwas Platz frei, sonst werden Einstellungen nicht gespeichert.'],
  [/os error 13/, 'Mixpilot darf seine Einstellungen nicht speichern. Prüf, ob du in deinem Home-Verzeichnis schreiben darfst.'],
  [/^(pactl|pw-record|pw-play): .*(No such file|not found)/, 'Ein PipeWire-Werkzeug fehlt. Installier die PipeWire-Werkzeuge deiner Distribution (pactl, pw-record, pw-play).'],
  [/^(pactl|pw-record|pw-play): /, 'PipeWire antwortet nicht. Melde dich ab und wieder an oder starte den Rechner neu.'],
];
function errorHint(raw) {
  if (Object.hasOwn(HINTS, raw)) return HINTS[raw];
  if (Object.hasOwn(EN, raw)) return null;
  // unknown texts get the bug report pointer, add a rule once a message turns up often
  return HINT_RULES.find(([re]) => re.test(raw))?.[1] ?? 'Wenn das wieder passiert, schick bitte über "Fehler melden" einen Bericht.';
}

let errorShown = null;
let errorRank = 0;
const errorClosed = new Set();
// `again`: the rank of a message a poll repeats (higher wins); once closed it stays closed until its cause was gone.
// A poll only replaces a lower polled message, and a one-shot error only when the audio core is down (rank 3+).
function showError(e, again = 0) {
  const raw = String(e);
  const box = $('error');
  if (again && errorClosed.has(raw)) return;
  if (again && !box.hidden && errorShown !== raw && (errorRank ? errorRank > again : again < 3)) return;
  // a raw message from pactl or the system gets a translated frame and is marked as English
  const known = Object.hasOwn(EN, raw);
  // rewriting the same text would make screen readers repeat the alert
  if (box.hidden || errorShown !== raw) {
    const hadFocus = box.contains(document.activeElement);
    const part = el('span', null, raw);
    part.lang = 'en';
    const close = el('button', 'error-close', '×');
    close.type = 'button';
    close.setAttribute('aria-label', t('Meldung schließen'));
    close.addEventListener('click', () => {
      errorClosed.add(errorShown);
      clearError();
      document.querySelector('.tab[aria-selected="true"]')?.focus();
    });
    // first in the flow, so it floats to the top right corner
    const hint = errorHint(raw);
    const report = el('button', 'error-report', t('Fehler melden'));
    report.type = 'button';
    report.addEventListener('click', () => invoke('open_bug_report', { errors: raw }).catch(console.error));
    // no report button for messages that already say how the user fixes it
    box.replaceChildren(close, ...(known ? [t(raw)] : [`${t('Fehler:')} `, part]), ...(hint ? [el('span', 'error-hint', t(hint)), report] : []));
    if (hadFocus) close.focus();
  }
  errorShown = raw;
  errorRank = again;
  box.hidden = false;
}

// hides the banner, or with a message only while it still shows that one: once its cause is gone
// it must not stay up until some unrelated setting is saved
function clearError(raw) {
  if (raw !== undefined) errorClosed.delete(raw);
  if (raw !== undefined && errorShown !== raw) return;
  $('error').hidden = true;
  errorShown = null;
  errorRank = 0;
}

// A dialog takes the focus and makes the window behind it inert; closing gives the focus back.
function openDialog(o) {
  if (o.hidden) o.from = document.activeElement;
  o.hidden = false;
  document.querySelector('.app').inert = true;
  o.firstElementChild.focus();
}
// click on the backdrop or Escape closes the dialog
function dismissOn(o, close) {
  o.addEventListener('click', (e) => { if (e.target === o) close(); });
  o.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
}

function closeDialog(o) {
  o.hidden = true;
  document.querySelector('.app').inert = !!document.querySelector('.overlay:not([hidden])');
  o.from?.focus();
}

// Faders follow the core's cubic taper: gain = (v/100)^3
const fdb = (v) => (v <= 0 ? '-∞ dB' : `${(60 * Math.log10(v / 100)).toFixed(1)} dB`);

function defaults(c) {
  c.auto ??= true;
  for (const [id] of CHANNELS) {
    // as in the core: a missing channel starts with its own preset, a channel without one plays flat
    c[id] ??= { eq: { game: 'gaming', chat: 'voice', music: 'music' }[id] ?? 'flat' };
    c[id].volume ??= 100;
    c[id].mute ??= false;
    c[id].eq ??= 'flat';
  }
  c.chatmix ??= 0;
  c.bass ??= 0;
  c.limiter ??= true;
  c.clarity ??= false;
  c.auto_volume ??= 'off';
  // no prototype: a preset named "__proto__" or "toString" is a plain key, never an inherited one
  c.eq_presets = Object.assign(Object.create(null), curves(c.eq_presets));
  c.eq_edits = Object.fromEntries(Object.entries(curves(c.eq_edits)).filter(([k]) => isBuiltIn(k)));
  // an own preset named like a later built-in ("Musik" before 0.0.6) becomes it; a different curve stays as its tuning
  for (const [name, g] of Object.entries(c.eq_presets)) {
    const k = builtinFor(name);
    if (!k) continue;
    if (!c.eq_edits[k] && g.some((v, i) => v !== EQ_BUILTIN[k][i])) c.eq_edits[k] = g;
    for (const [id] of CHANNELS) if (c[id].eq === name) c[id].eq = k;
    delete c.eq_presets[name];
  }
  // up to 0.0.2 one equalizer worked on everything; a tuned curve stays as an own preset
  if (Array.isArray(c.eq)) {
    const g = c.eq;
    const known = [...Object.values(EQ_BUILTIN), ...Object.values(c.eq_presets)].some((p) => p.every((v, i) => v === g[i]));
    if (!known && Object.keys(curves({ g })).length && Object.keys(c.eq_presets).length < EQ_OWN_MAX) c.eq_presets[t('Mein EQ')] ??= g.slice();
    delete c.eq;
  }
  c.output ??= '';
  c.input ??= '';
  c.mic ??= {};
  Object.entries({ gain: 100, mute: false, noise: 'normal', agc: true, gate: true, voice: 'natural', monitor: false, deess: 'normal' })
    .forEach(([k, v]) => (c.mic[k] ??= v));
  c.ducking ??= true;
  c.night ??= {};
  c.night.enabled ??= false;
  c.night.from ??= 22;
  c.night.to ??= 7;
  c.auto_route ??= true;
  if (!Array.isArray(c.rules)) c.rules = [];
  return c;
}

// Save at most every 60 ms while dragging, always save the final value.
let lastSave = 0;
let saveTimer = null;
function save() {
  clearTimeout(saveTimer);
  const run = () => {
    lastSave = Date.now();
    const ack = cfg.config_reset === null;
    invoke('set_config', { cfg }).then(() => {
      // the null went out once; sending it again would delete a later note before it is shown
      if (ack && cfg.config_reset === null) delete cfg.config_reset;
      if (!errorRank) clearError();
    }, showError);
  };
  if (Date.now() - lastSave > 60) run();
  else saveTimer = setTimeout(run, 60);
}
const change = (fn) => () => { fn(); paintAll(); save(); };

// The tray menu writes "auto" and "mic.mute" into the same file. Take them over, otherwise the next
// save from this window would silently undo a mute made in the tray.
let refreshHotkey = async () => {};
async function pollConfig() {
  if (!cfg || document.hidden) return;
  // the desktop may answer late at login, and the key can change in the desktop's own settings
  if (tab === 'mic') refreshHotkey();
  if (Date.now() - lastSave < 500) return;
  const c = await invoke('get_config').catch(() => null);
  if (!c || Date.now() - lastSave < 500) return;
  if (c.config_reset && c.config_reset !== cfg.config_reset) showReset(c.config_reset);
  const auto = c.auto ?? true;
  const mute = c.mic?.mute ?? false;
  if (auto !== cfg.auto || mute !== cfg.mic.mute) {
    cfg.auto = auto;
    cfg.mic.mute = mute;
    paintAll();
  }
}

function bindSwitch(id, get, set) {
  const b = $(id);
  b.addEventListener('click', change(() => set(!get())));
  painters.push(() => b.setAttribute('aria-checked', String(!!get())));
}

function buttonGroup(container, options, get, set) {
  const box = $(container);
  const buttons = options.map(([value, label]) => {
    const b = el('button', null, label);
    b.addEventListener('click', change(() => set(value)));
    box.append(b);
    return [value, b];
  });
  painters.push(() => buttons.forEach(([v, b]) => b.setAttribute('aria-pressed', String(get() === v))));
}

// A value you can also type: click, type, Enter. Esc reverts, arrow keys step (Shift: 10 steps).
// German decimal commas and units ("75 %", "-6,5 dB") are accepted.
function numField(input, { get, set, min, max, step = 1, show, toValue = (n) => n, label }) {
  input.classList.add('num');
  input.type = 'text';
  input.inputMode = 'decimal';
  input.spellcheck = false;
  input.setAttribute('aria-label', label);
  const commit = () => {
    // blur after Enter, Esc or Tab: an untouched field must not round the value or save
    if (input.value === show()) return;
    const n = parseFloat(input.value.replace(',', '.').replace(/[^0-9.+-]/g, ''));
    if (Number.isFinite(n)) {
      const v = Math.min(max, Math.max(min, toValue(n)));
      set(v);
      paintAll();
      save();
    }
    input.value = show();
  };
  input.addEventListener('focus', () => input.select());
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { commit(); input.blur(); }
    else if (e.key === 'Escape') { input.value = show(); input.blur(); }
    else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault();
      const d = (e.key === 'ArrowUp' ? 1 : -1) * step * (e.shiftKey ? 10 : 1);
      set(Math.min(max, Math.max(min, get() + d)));
      paintAll();
      save();
      input.value = show();
      input.select();
    }
  });
  input.addEventListener('blur', commit);
  painters.push(() => { if (document.activeElement !== input) input.value = show(); });
  return input;
}

// what a screen reader says for a slider, like the readout next to it; written only when it changes
function valueText(r, s) {
  if (r.getAttribute('aria-valuetext') !== s) r.setAttribute('aria-valuetext', s);
}

function bindRange(id, get, set, text) {
  const r = $(id);
  r.addEventListener('input', change(() => set(Number(r.value))));
  painters.push(() => {
    if (document.activeElement !== r) r.value = get();
    if (text) valueText(r, text());
  });
}

// Mouse wheel over a control changes it (up = more, Shift = 10 steps); the page itself never scrolls.
document.addEventListener('wheel', (e) => {
  const t = e.target.closest('input');
  // Ctrl+wheel zooms the page
  if (!t || e.ctrlKey) return;
  if (t.type === 'range') {
    e.preventDefault();
    const step = Number(t.step || 1) * (e.shiftKey ? 10 : 1);
    const v = Number(t.value) + (e.deltaY < 0 ? step : -step);
    t.value = String(Math.min(Number(t.max), Math.max(Number(t.min), v)));
    t.dispatchEvent(new Event('input', { bubbles: true }));
  } else if (t.classList.contains('num')) {
    e.preventDefault();
    t.dispatchEvent(new KeyboardEvent('keydown', { key: e.deltaY < 0 ? 'ArrowUp' : 'ArrowDown', shiftKey: e.shiftKey }));
    t.blur();
  }
}, { passive: false });

// ---------- meters ----------
// peak (linear) -> fraction of the meter, -60 dB .. 0 dB
const frac = (v) => (v > 0 ? Math.max(0, Math.min(1, (20 * Math.log10(v) + 60) / 60)) : 0);
function makeSegments(parent, n) {
  const segs = [];
  for (let i = 0; i < n; i++) {
    const s = el('i');
    // scale is -60..0 dBFS: red from -3 dB, yellow from -12 dB
    const db = (i / n) * 60 - 60;
    s.dataset.zone = db >= -3 ? 'hi' : db >= -12 ? 'mid' : 'low';
    parent.append(s);
    segs.push(s);
  }
  let shown = 0;
  return (f) => {
    const lit = Math.round(f * n);
    if (lit === shown) return;
    // touch only the segments between the old and the new height
    for (let i = Math.min(lit, shown); i < Math.max(lit, shown); i++) segs[i].className = i < lit ? segs[i].dataset.zone : '';
    shown = lit;
  };
}
// smooth fall like a real meter: rise instantly, fall about 30 dB per second (50 ms ticks)
function decay(prev, now) {
  return Math.max(now, prev - 0.025);
}

// ---------- mixer ----------
const strips = {};
function strip(id, label, master) {
  const root = el('div', master ? 'strip master' : 'strip');
  const head = el('div', 'strip-head');
  head.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="${ICONS[id]}"></path></svg>`;
  const dot = el('span', 'dot');
  head.append(el('h2', 'strip-name', label), dot);
  const body = el('div', 'strip-body');
  const meter = el('div', 'meter');
  const cl = el('div', 'col');
  const cr = el('div', 'col');
  meter.append(cl, cr);
  const setL = makeSegments(cl, 20);
  const setR = makeSegments(cr, 20);
  const fwrap = el('div', 'fwrap');
  const fader = el('input', 'fader');
  Object.assign(fader, { type: 'range', min: 0, max: 100, step: 1 });
  fader.setAttribute('aria-label', t('{0} Lautstärke', label));
  fwrap.append(fader);
  body.append(meter, fwrap);
  const value = el('input', 'value');
  root.append(head, body, value);

  const get = () => (master ? masterDev.volume : cfg[id].volume);
  // while the user moves it the fader leads; otherwise it follows changes from outside (volume keys,
  // the tray), also when it still has the focus from an earlier click
  let moved = 0;
  fader.addEventListener('input', () => (moved = Date.now()));
  if (master) {
    fader.addEventListener('pointerdown', () => (masterTouched = Date.now()));
    fader.addEventListener('input', () => { setMaster(Number(fader.value)); paintAll(); });
  } else {
    fader.addEventListener('input', change(() => (cfg[id].volume = Number(fader.value))));
  }
  const mute = el('button', 'mute', t('STUMM'));
  mute.setAttribute('aria-label', t('{0} stumm', label));
  let chip = null;
  const apps = el('div', 'appchips');
  const btns = el('div', 'btns');
  if (master) {
    mute.addEventListener('click', async () => {
      masterDev.mute = !masterDev.mute;
      masterTouched = Date.now();
      paintAll();
      await invoke('set_master_mute', { mute: masterDev.mute }).catch(showError);
    });
    btns.append(mute);
    root.append(btns, el('p', 'hint', t('Systemlautstärke deines Ausgabegeräts')));
  } else {
    mute.addEventListener('click', change(() => (cfg[id].mute = !cfg[id].mute)));
    chip = el('button', 'eqchip');
    chip.setAttribute('aria-haspopup', 'menu');
    chip.setAttribute('aria-expanded', 'false');
    chip.addEventListener('click', (e) => {
      e.stopPropagation();
      if (eqMenu?.chip === chip) closeEqMenu();
      else openEqMenu(id, chip);
    });
    btns.append(mute, chip);
    root.append(btns, apps);
  }
  painters.push(() => {
    // while it moves, range and value are the user's; above 100 % (the volume keys go up to 150) the
    // range is 150, so touching it does not cut the volume down
    if (Date.now() - moved > 800) {
      if (master) fader.max = masterDev.volume > 100 ? 150 : 100;
      fader.value = get();
    }
    const muted = master ? masterDev.mute : cfg[id].mute;
    const level = master ? (masterDev.known ? `${masterDev.volume} %` : t('kein Gerät')) : fdb(cfg[id].volume);
    valueText(fader, muted ? `${level}, ${t('stumm')}` : level);

    mute.setAttribute('aria-pressed', String(muted));
    if (chip) {
      const name = presetLabel(chPreset(id));
      chip.replaceChildren(el('b', null, 'EQ'), el('span', null, name));
      chip.title = name;
      chip.setAttribute('aria-label', t('Klang {0}: {1}, wählen', label, name));
    }
  });
  if (master) {
    numField(value, {
      label: t('Master in Prozent'),
      get: () => masterDev.volume,
      set: (v) => setMaster(Math.round(v)),
      min: 0, max: 100,
      show: () => (masterDev.mute ? t('stumm') : masterDev.known ? `${masterDev.volume} %` : t('kein Gerät')),
    });
  } else {
    // typed in dB like the readout; the fader position follows the core's cubic taper
    numField(value, {
      label: t('{0} in dB', label),
      get: () => cfg[id].volume,
      set: (v) => (cfg[id].volume = Math.round(v * 10) / 10),
      min: 0, max: 100,
      toValue: (db) => (db <= -60 ? 0 : 100 * Math.pow(10, Math.min(0, db) / 60)),
      show: () => (cfg[id].mute ? t('stumm') : fdb(cfg[id].volume)),
    });
  }
  strips[id] = { setL, setR, dot, apps, l: 0, r: 0, appsKey: '' };
  return root;
}

function paintCardsMixer() {
  const c = cfg.chatmix;
  const [game, chat] = [c > 0 ? 100 - c : 100, c < 0 ? 100 + c : 100];
  $('cm-text').textContent = `Game ${game} %   Chat ${chat} %`;
  valueText($('chatmix'), `Game ${game} %, Chat ${chat} %`);
}

// ---------- sound ----------
// Presets are edited here, the mixer picks one per channel. Built-ins can be tuned and reset,
// a new preset starts as a copy of the one on screen.
let selP = 'flat';
let editFor = null; // channel whose "Edit" opened the sound tab, gets a new preset right away
let naming = false;

function setBand(i, v) {
  if (isBuiltIn(selP)) {
    const g = curveOf(selP).slice();
    g[i] = v;
    if (g.every((x, j) => x === EQ_BUILTIN[selP][j])) delete cfg.eq_edits[selP];
    else cfg.eq_edits[selP] = g;
  } else if (cfg.eq_presets[selP]) {
    cfg.eq_presets[selP][i] = v;
  }
}

function nameField() {
  const input = el('input', 'chip-name');
  Object.assign(input, { type: 'text', maxLength: 20, spellcheck: false, placeholder: t('Name') });
  input.dataset.k = 'name';
  input.setAttribute('aria-label', t('Name für das neue Preset'));
  // stays in place while empty, so the alert is read out when the text arrives
  const msg = el('span', 'name-error');
  msg.id = 'name-error';
  msg.setAttribute('role', 'alert');
  const done = () => { naming = false; paintAll(); };
  const commit = () => {
    const n = input.value.trim();
    if (!naming) return;
    if (!n) return done();
    // a name already in use would silently replace that preset's curve
    const taken = reservedName(n) ? 'So heißt schon ein eingebautes Preset' : n !== selP && Object.hasOwn(cfg.eq_presets, n) ? 'So heißt schon eines deiner Presets' : null;
    if (taken) {
      input.setAttribute('aria-invalid', 'true');
      input.setAttribute('aria-describedby', msg.id);
      msg.textContent = t(taken);
      return;
    }
    cfg.eq_presets[n] = curveOf(selP).slice();
    selP = n;
    if (editFor) cfg[editFor].eq = n;
    naming = false;
    paintAll();
    save();
  };
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') commit();
    else if (e.key === 'Escape') done();
  });
  input.addEventListener('input', () => {
    input.removeAttribute('aria-invalid');
    input.removeAttribute('aria-describedby');
    msg.textContent = '';
  });
  input.addEventListener('blur', () => { if (input.getAttribute('aria-invalid')) done(); else commit(); });
  return [input, msg];
}

function presetChips(box) {
  let key = null;
  return () => {
    const own = Object.keys(cfg.eq_presets);
    const edited = isBuiltIn(selP) && !!cfg.eq_edits[selP];
    const k = JSON.stringify([own, selP, edited, naming]);
    if (k === key) return;
    key = k;
    // the chips are rebuilt, so keyboard focus moves to the new chip with the same job
    const focused = box.contains(document.activeElement) ? document.activeElement.dataset.k : null;
    box.replaceChildren();
    const chip = (text, k, cls, onClick, label) => {
      const b = el('button', cls, text);
      b.dataset.k = k;
      if (label) b.setAttribute('aria-label', label);
      b.addEventListener('click', onClick);
      box.append(b);
      return b;
    };
    for (const p of presetKeys()) {
      chip(presetLabel(p), `p:${p}`, null, () => { selP = p; naming = false; paintAll(); }).setAttribute('aria-pressed', String(p === selP));
    }
    if (edited) {
      chip(t('Zurücksetzen'), 'reset', 'ghost', change(() => delete cfg.eq_edits[selP]), t('Preset {0} zurücksetzen', presetLabel(selP)));
    } else if (!isBuiltIn(selP)) {
      chip(t('Löschen'), 'delete', 'ghost', change(() => {
        delete cfg.eq_presets[selP];
        for (const [id] of CHANNELS) if (cfg[id].eq === selP) cfg[id].eq = 'flat';
        selP = 'flat';
      }), t('Preset {0} löschen', selP));
    }
    if (naming) {
      const [input, msg] = nameField();
      box.append(input, msg);
      input.focus();
    } else {
      const full = own.length >= EQ_OWN_MAX;
      const b = chip(t('+ Neu'), 'new', 'ghost', () => { if (!full) { naming = true; paintAll(); } }, t('Neues Preset als Kopie von {0}', presetLabel(selP)));
      if (full) {
        b.setAttribute('aria-disabled', 'true');
        b.title = t('Höchstens {0} eigene Presets, lösch zuerst eins', EQ_OWN_MAX);
        // hidden text still serves as the description for screen readers
        const why = el('span', null, b.title);
        why.id = 'new-full';
        why.hidden = true;
        b.setAttribute('aria-describedby', why.id);
        box.append(why);
      }
    }
    // after naming, the focus lands on the preset that is now selected
    const next = focused === 'name' ? `p:${selP}` : focused;
    if (next) box.querySelector(`[data-k="${CSS.escape(next)}"]`)?.focus();
  };
}

// The EQ button of a mixer strip opens this list: every preset, plus the way into the sound tab.
let eqMenu = null;
function closeEqMenu(refocus) {
  if (!eqMenu) return;
  eqMenu.box.remove();
  eqMenu.chip.setAttribute('aria-expanded', 'false');
  if (refocus) eqMenu.chip.focus();
  eqMenu = null;
}

function openEqMenu(id, chip) {
  closeEqMenu();
  const box = el('div', 'eqmenu');
  box.setAttribute('role', 'menu');
  const item = (text, role, onClick) => {
    const b = el('button', null, text);
    b.setAttribute('role', role);
    b.addEventListener('click', () => { closeEqMenu(true); onClick(); });
    box.append(b);
    return b;
  };
  for (const p of presetKeys()) {
    item(presetLabel(p), 'menuitemradio', change(() => (cfg[id].eq = p))).setAttribute('aria-checked', String(p === chPreset(id)));
  }
  box.append(el('div', 'eqmenu-sep'));
  item(t('Bearbeiten…'), 'menuitem', () => {
    selP = chPreset(id);
    naming = false;
    document.querySelector('.tab[data-tab="sound"]').click();
    editFor = id;
  });
  box.addEventListener('keydown', (e) => {
    const items = [...box.querySelectorAll('button')];
    const i = items.indexOf(document.activeElement);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      items[(i + (e.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length].focus();
    } else if (e.key === 'Escape' || e.key === 'Tab') {
      // Tab then moves on from the chip, not from the removed menu
      closeEqMenu(true);
    }
  });
  document.body.append(box);
  const r = chip.getBoundingClientRect();
  box.style.minWidth = `${Math.max(150, r.width)}px`;
  box.style.left = `${Math.max(8, Math.min(r.left, innerWidth - box.offsetWidth - 8))}px`;
  // opens upwards when the strip sits near the bottom edge
  if (r.bottom + 4 + box.offsetHeight > innerHeight) box.style.bottom = `${innerHeight - r.top + 4}px`;
  else box.style.top = `${r.bottom + 4}px`;
  chip.setAttribute('aria-expanded', 'true');
  eqMenu = { box, chip };
  box.querySelector('[aria-checked="true"]').focus();
}

function buildSound() {
  bindSwitch('bass-on', () => cfg.bass > 0, (on) => {
    if (on) cfg.bass = cfg.bass_last || 50;
    else { cfg.bass_last = cfg.bass; cfg.bass = 0; }
  });
  bindRange('bass', () => cfg.bass, (v) => (cfg.bass = v), () => `${cfg.bass} %`);
  numField($('bass-value'), { label: t('Bass Boost in Prozent'), get: () => cfg.bass, set: (v) => (cfg.bass = Math.round(v)), min: 0, max: 100, show: () => `${cfg.bass} %` });
  painters.push(() => ($('bass-text').textContent = cfg.bass ? t('+{0} dB unter 100 Hz, mit Schutz gegen Übersteuern.', (cfg.bass * 0.12).toFixed(1)) : t('Aus.')));
  buttonGroup('autovol', AUTOVOL, () => cfg.auto_volume, (v) => (cfg.auto_volume = v));
  painters.push(() => ($('autovol-text').textContent = (AUTOVOL.find(([k]) => k === cfg.auto_volume) || AUTOVOL[0])[2]));
  bindSwitch('limiter', () => cfg.limiter, (v) => (cfg.limiter = v));
  bindSwitch('clarity', () => cfg.clarity, (v) => (cfg.clarity = v));

  const paintChips = presetChips($('eq-presets'));
  const bands = $('bands');
  const bandEls = FREQ.map((f, i) => {
    const box = el('div', 'band');
    const val = numField(el('input'), {
      label: `Band ${f} Hz in dB`,
      get: () => curveOf(selP)[i],
      set: (v) => setBand(i, Math.round(v)),
      min: -12, max: 12,
      show: () => (curveOf(selP)[i] > 0 ? '+' : '') + curveOf(selP)[i],
    });
    const wrap = el('div', 'bwrap');
    const s = el('input', 'bslider');
    Object.assign(s, { type: 'range', min: -12, max: 12, step: 1 });
    s.setAttribute('aria-label', `Band ${f} Hz`);
    s.addEventListener('input', change(() => setBand(i, Number(s.value))));
    wrap.append(s);
    box.append(val, wrap, el('span', 'mono freq', f));
    bands.append(box);
    return s;
  });
  const paintGrid = eqLeds($('eq-grid'));
  painters.push(() => {
    if (!isBuiltIn(selP) && !cfg.eq_presets[selP]) selP = 'flat';
    const used = CHANNELS.filter(([id]) => chPreset(id) === selP).map(([, l]) => l);
    $('eq-name').textContent = presetLabel(selP) + (isBuiltIn(selP) && cfg.eq_edits[selP] ? ` ${t('(geändert)')}` : '');
    $('eq-used').textContent = used.length ? t('Benutzt von {0}. Änderungen hörst du dort sofort.', used.join(', '))
      : t('Kein Kanal benutzt dieses Preset. Im Mixer wählst du es über den EQ-Knopf eines Kanals.');
    paintChips();
    const g = curveOf(selP);
    bandEls.forEach((s, i) => {
      if (document.activeElement !== s) s.value = g[i];
      valueText(s, `${g[i] > 0 ? '+' : ''}${g[i]} dB`);
    });
    paintGrid(g);
  });
}

// LED columns around an amber zero row, three per band, the outer two blend into the neighbours.
// A square-root scale so +2 dB already shows while +12 dB still fits; a dim segment marks the half step.
function eqLeds(parent) {
  const grid = ledColumns(parent, 30, 9);
  return (eq) => grid.forEach((segs, k) => {
    const x = (k + 0.5) / 3 - 0.5;
    const b = Math.max(0, Math.min(9, Math.floor(x)));
    const f = Math.max(0, Math.min(1, x - b));
    const v = eq[b] * (1 - f) + eq[Math.min(9, b + 1)] * f;
    const n = Math.sign(v) * 4 * Math.sqrt(Math.abs(v) / 12);
    segs.forEach((s, i) => {
      const h = 4 - i;
      const d = Math.abs(h);
      s.className = h === 0 ? 'mid' : Math.sign(h) !== Math.sign(n) ? '' : d <= Math.abs(n) ? 'low' : d - 0.5 <= Math.abs(n) ? 'low dim' : '';
    });
  });
}

// ---------- microphone ----------
// The level in words for screen readers, same zones as the colours: yellow from -12 dB, red from -3 dB.
function levelMeter(meter) {
  const set = makeSegments(meter, 40);
  let shown = '';
  return (f) => {
    set(f);
    const db = f * 60 - 60;
    const text = f <= 0 ? t('kein Signal') : db < -12 ? t('zu leise') : db < -3 ? t('gut') : t('zu laut');
    const now = String(Math.round(f * 10) * 10);
    if (text + now === shown) return;
    shown = text + now;
    meter.setAttribute('aria-valuenow', now);
    meter.setAttribute('aria-valuetext', text);
  };
}
let setMicMeter = () => {};
let setMicInMeter = () => {};
function buildMic() {
  setMicMeter = levelMeter($('mic-meter'));
  setMicInMeter = levelMeter($('mic-in-meter'));
  painters.push(() => ($('voice-text').textContent = VOICE_TEXT[cfg.mic.voice] || ''));
  buttonGroup('noise', NOISE, () => cfg.mic.noise, (v) => (cfg.mic.noise = v));
  buttonGroup('deess', NOISE, () => cfg.mic.deess, (v) => (cfg.mic.deess = v));
  buttonGroup('voice', VOICE, () => cfg.mic.voice, (v) => (cfg.mic.voice = v));
  bindRange('mic-gain', () => cfg.mic.gain, (v) => (cfg.mic.gain = v), () => `${cfg.mic.gain} %`);
  numField($('gain-text'), { label: t('Mikrofon-Verstärkung in Prozent'), get: () => cfg.mic.gain, set: (v) => (cfg.mic.gain = Math.round(v)), min: 0, max: 200, show: () => `${cfg.mic.gain} %` });
  bindSwitch('agc', () => cfg.mic.agc, (v) => (cfg.mic.agc = v));
  bindSwitch('gate', () => cfg.mic.gate, (v) => (cfg.mic.gate = v));
  bindSwitch('monitor', () => cfg.mic.monitor, (v) => (cfg.mic.monitor = v));
  // The desktop owns the key: switching on asks for it once in the desktop's own dialog.
  const hkOn = $('hotkey-on');
  const hk = $('hotkey');
  let hkBusy = false;
  const showHotkey = (s) => {
    hkOn.setAttribute('aria-checked', String(s.on));
    hkOn.disabled = !s.available || hkBusy;
    hk.disabled = hkBusy;
    $('hotkey-row').hidden = !s.on;
    hk.textContent = s.trigger;
    $('hotkey-text').textContent = hkBusy ? t('Wähle die Taste im Dialog deines Desktops.')
      : !s.available ? t('Dein Desktop bietet Apps keine globalen Tastenkürzel an.')
      : s.on ? t('Einmal drücken stumm, nochmal drücken wieder an. Auch im Spiel.')
      : t('Eine Taste schaltet das Mikrofon stumm und wieder an, egal welches Fenster vorne ist.');
  };
  refreshHotkey = () => invoke('hotkey_status').then((s) => {
    if (hkBusy) return;
    showHotkey(s);
    // bound while the window was closed: remember it for the next start
    if (s.on && !cfg.mic_hotkey) {
      cfg.mic_hotkey = true;
      save();
    }
  }).catch(() => showHotkey({ available: false, on: false, trigger: '' }));
  const hotkeyCall = async (cmd, args) => {
    if (hkBusy) return;
    hkBusy = true;
    showHotkey({ available: true, on: hkOn.getAttribute('aria-checked') === 'true', trigger: hk.textContent });
    const s = await invoke(cmd, args).catch((e) => (showError(e), null));
    hkBusy = false;
    if (s) showHotkey(s);
    if (s && cfg.mic_hotkey !== s.on) {
      cfg.mic_hotkey = s.on;
      save();
    }
    refreshHotkey();
  };
  hkOn.addEventListener('click', () => hotkeyCall('set_hotkey', { on: hkOn.getAttribute('aria-checked') !== 'true' }));
  hk.addEventListener('click', () => hotkeyCall('change_hotkey'));
  refreshHotkey();
  $('mic-state').addEventListener('click', async () => {
    try {
      await invoke('unmute_mic');
    } catch (e) {
      // still muted on disk: showing "active" would be a lie
      return showError(e);
    }
    cfg.mic.mute = false;
    clearError(MIC_MUTED);
    paintAll();
  });
  const test = $('mic-test');
  test.addEventListener('click', async () => {
    // a muted microphone would record five seconds of silence
    if (cfg.mic.mute) return showError(MIC_MUTED);
    test.disabled = true;
    let left = 5;
    test.textContent = t('Aufnahme {0} s', left);
    const count = setInterval(() => (test.textContent = t('Aufnahme {0} s', (left = Math.max(1, left - 1)))), 1000);
    const recorded = await invoke('mic_test_record').then(() => true, (e) => (showError(e), false));
    clearInterval(count);
    if (recorded) {
      test.textContent = t('Wiedergabe');
      await invoke('mic_test_play').catch(showError);
    }
    test.textContent = t('Mikrofon testen');
    test.disabled = false;
  });
  $('make-default').addEventListener('click', async () => {
    // pin the real microphone first, otherwise "system default" would point Mixpilot at itself
    if (!cfg.input && devices.default_source && devices.default_source !== 'mixpilot_mic') {
      cfg.input = devices.default_source;
      save();
    }
    try {
      await invoke('set_default_source', { name: 'mixpilot_mic' });
      // WirePlumber applies it a moment later, the core's list catches up on the next refresh
      devices.default_source = 'mixpilot_mic';
      paintDevices();
    } catch (e) {
      showError(e);
    }
  });
}

function paintDefaultMic() {
  const isDefault = devices.default_source === 'mixpilot_mic';
  const cur = devices.sources.find((s) => s.name === devices.default_source);
  $('default-mic-text').textContent = isDefault
    ? t('Mixpilot Mikrofon ist dein Standard-Mikrofon. Apps bekommen automatisch die bearbeitete Stimme.')
    : t('Apps nutzen gerade: {0}. Setze Mixpilot als Standard oder wähle in Discord und Co. "Mixpilot Mikrofon".', cur ? cur.description : devices.default_source || t('unbekannt'));
  $('make-default').disabled = isDefault;
  $('make-default').textContent = isDefault ? t('Ist Standard') : t('Als Standard-Mikrofon setzen');
  const micName = cfg.input || devices.default_source;
  const mic = micName !== 'mixpilot_mic' && devices.sources.find((s) => s.name === micName);
  $('mic-name').textContent = mic ? mic.description.toUpperCase() : '';
}

// ---------- devices ----------
function fillSelect(select, list, current, defaultName, allowDefault) {
  const def = list.find((d) => d.name === defaultName);
  select.replaceChildren();
  if (allowDefault) select.add(new Option(def ? t('System-Standard ({0})', def.description) : t('System-Standard'), ''));
  // without the default entry an empty choice would leave the picker blank
  else if (!current) select.add(Object.assign(new Option(t('Mikrofon wählen'), ''), { disabled: true }));
  for (const d of list) select.add(new Option(d.description, d.name));
  if (current && !list.some((d) => d.name === current)) select.add(new Option(t('{0} (nicht verbunden)', current), current));
  select.value = current;
}

function paintDevices() {
  const c = devCfg();
  document.querySelectorAll('.output-select').forEach((s) => {
    if (document.activeElement !== s) fillSelect(s, devices.sinks, c.output, devices.default_sink, true);
  });
  // "system default" is not offered while the system default is Mixpilot's own microphone
  const micIsDefault = devices.default_source === 'mixpilot_mic';
  document.querySelectorAll('.input-select').forEach((s) => {
    if (document.activeElement !== s) fillSelect(s, devices.sources, c.input, devices.default_source, !micIsDefault);
  });
  if (cfg) paintDefaultMic();
  const loop = !c.output && devices.default_sink.startsWith('mixpilot_');
  const loopMsg = 'Dein System-Standard für die Ausgabe ist ein Mixpilot-Kanal. Wähle oben bei Ausgabe dein Headset oder deine Lautsprecher.';
  const dupMsg = 'Mixpilot läuft zweimal, zum Beispiel als Snap und als deb. Beende oder deinstalliere eine der beiden Versionen, sonst gibt es jeden Kanal doppelt.';
  if (loop) showError(loopMsg, 1);
  else clearError(loopMsg);
  if (devices.duplicate) showError(dupMsg, 2);
  else clearError(dupMsg);
}

async function refreshDevices() {
  try {
    devices = await invoke('list_devices');
    paintDevices();
  } catch (e) {
    showError(e);
  }
}

function bindDevices() {
  for (const key of ['output', 'input']) {
    for (const s of document.querySelectorAll(`.${key}-select`)) {
      s.addEventListener('focus', refreshDevices);
      s.addEventListener('change', () => { devCfg()[key] = s.value; if (cfg) save(); paintDevices(); });
    }
  }
}

// ---------- automation ----------
function buildAuto() {
  bindSwitch('auto', () => cfg.auto, (v) => {
    cfg.auto = v;
    invoke('set_auto', { on: v }).catch(showError);
  });
  painters.push(() => {
    $('led').className = cfg.auto ? 'led on' : 'led';
    $('auto-note').textContent = cfg.auto
      ? t('Der Autopilot oben rechts ist an: Apps werden automatisch einsortiert, Ducking und Nachtmodus arbeiten, soweit sie hier eingeschaltet sind. Deine eigenen Zuordnungen gelten immer.')
      : t('Der Autopilot ist aus: nichts passiert automatisch. Neue Apps bleiben, wo sie sind, Ducking und Nachtmodus ruhen. Deine eigenen Zuordnungen und alle Regler gelten weiter.');
    for (const id of ['ducking', 'auto-route', 'night']) $(id).closest('.rule').classList.toggle('inactive', !cfg.auto);
  });
  bindSwitch('ducking', () => cfg.ducking, (v) => (cfg.ducking = v));
  bindSwitch('auto-route', () => cfg.auto_route, (v) => (cfg.auto_route = v));
  bindSwitch('night', () => cfg.night.enabled, (v) => (cfg.night.enabled = v));
  for (const [id, key] of [['night-from', 'from'], ['night-to', 'to']]) {
    const s = $(id);
    for (let h = 0; h < 24; h++) s.add(new Option(String(h).padStart(2, '0'), String(h)));
    s.addEventListener('change', change(() => (cfg.night[key] = Number(s.value))));
    const other = key === 'from' ? 'to' : 'from';
    painters.push(() => {
      s.value = String(cfg.night[key]);
      // the same hour on both sides is an empty window, night mode would never start
      for (const o of s.options) o.disabled = Number(o.value) === cfg.night[other];
    });
  }
  const auto = $('autostart');
  auto.addEventListener('click', async () => {
    const on = auto.getAttribute('aria-checked') !== 'true';
    await invoke('set_autostart', { on }).catch(showError);
    auto.setAttribute('aria-checked', String(await invoke('get_autostart')));
  });
  invoke('get_autostart').then((on) => auto.setAttribute('aria-checked', String(on)));
}

function assign(key, channel) {
  forget(key);
  cfg.rules.unshift({ match: key, channel, user: true });
  save();
}
function forget(key) {
  cfg.rules = cfg.rules.filter((r) => !(r.user && r.match === key));
}

// Playing apps plus saved assignments of apps that are not playing, so a wrong choice can always be undone.
let apps = [];
let appsKey = '';
function paintApps(force) {
  const box = $('apps');
  if (!force && box.contains(document.activeElement)) return;
  const focusKey = box.contains(document.activeElement) ? document.activeElement.dataset.key : null;
  const saved = cfg.rules.filter((r) => r.user);
  const key = JSON.stringify([apps, saved]);
  if (key === appsKey) return;
  appsKey = key;
  const rows = apps.map((a) => ({ ...a, playing: true }));
  for (const r of saved) {
    if (!rows.some((a) => a.key === r.match)) rows.push({ name: r.match, key: r.match, channel: r.channel, playing: false });
  }
  box.replaceChildren();
  if (!rows.length) box.append(el('p', 'hint', t('Gerade spielt keine App Ton ab.')));
  for (const a of rows) {
    const own = saved.some((r) => r.match === a.key);
    const row = el('div', a.playing ? 'app-row' : 'app-row saved');
    const info = el('div');
    info.append(el('div', 'title small', a.name), el('p', 'hint', a.playing ? (own ? t('spielt gerade') : `${t('spielt gerade')}, ${t('automatisch')}`) : t('gemerkt, spielt gerade nicht')));
    const s = el('select');
    s.setAttribute('aria-label', t('Kanal für {0}', a.name));
    for (const [id, label] of CHANNELS) s.add(new Option(label, id));
    s.add(new Option(t('Nicht einsortieren'), 'none'));
    s.value = CHANNELS.some(([id]) => id === a.channel) ? a.channel : 'none';
    s.dataset.key = a.key;
    s.addEventListener('change', () => { assign(a.key, s.value); paintApps(true); });
    const x = el('button', 'forget', '×');
    x.title = t('Zuordnung vergessen');
    x.setAttribute('aria-label', t('Zuordnung für {0} vergessen', a.name));
    // keeps its space so all channel pickers line up
    x.style.visibility = own ? '' : 'hidden';
    x.addEventListener('click', () => { forget(a.key); save(); paintApps(true); });
    row.append(info, s, x);
    box.append(row);
  }
  if (focusKey) [...box.querySelectorAll('select')].find((e) => e.dataset.key === focusKey)?.focus();
}

// device events carry the node name; show what the device pickers show
function eventText(e) {
  const args = [...(e.args || [])];
  if (e.key === 'output' || e.key === 'input') {
    const d = [...devices.sinks, ...devices.sources].find((x) => x.name === args[0]);
    args[0] = !args[0] ? t('System-Standard') : d ? d.description : args[0];
  }
  // the core names channels in English, Music is the one with a German name
  if (e.key === 'sorted' || e.key === 'moved') args[1] = CHANNELS.find(([id]) => id === String(args[1]).toLowerCase())?.[1] ?? args[1];
  return t(EVENTS[e.key] || e.key || '', ...args);
}

let eventsKey = '';
function paintEvents(events) {
  const key = JSON.stringify(events);
  if (key === eventsKey) return;
  eventsKey = key;
  const ul = $('events');
  ul.replaceChildren();
  if (!events.length) ul.append(el('li', 'hint', t('Noch nichts passiert.')));
  for (const e of events.slice(-4).reverse()) {
    const li = el('li');
    li.append(el('span', 'mono', e.time), el('span', null, eventText(e)));
    ul.append(li);
  }
}

// ---------- live state ----------
let micLevel = 0;
let micInLevel = 0;
let clippedAt = 0;
let soundAt = Date.now();
let shownHint = '';
function paintGainHint(hint) {
  // with Mixpilot as the system default the core records some other source, name unknown here
  const mic = devices.sources.find((d) => d.name === (cfg.input || devices.default_source) && !d.name.startsWith('mixpilot'));
  const key = hint + (mic ? mic.name : '');
  if (key === shownHint) return;
  shownHint = key;
  $('gain-hint').classList.toggle('alert', hint !== '');
  $('gain-hint').textContent = hint === 'clip'
    ? t('Dein Mikrofon übersteuert, bevor Mixpilot den Ton bekommt. Stell den Mikrofonpegel in den Sound-Einstellungen deines Systems leiser.')
    : hint === 'silent'
      ? t('Von {0} kommt kein Ton. Sprich etwas, oder wähle oben bei Eingabe ein anderes Mikrofon.', mic ? mic.description : t('diesem Mikrofon'))
      : t('Beim Sprechen sollte "Eingang" fast bis ans rechte Ende reichen, aber nicht ganz.');
}
// get_state runs on the main thread; when that stalls, the 50 ms interval must not pile up calls
let ticking = false;
async function tick() {
  if (ticking) return;
  ticking = true;
  try {
    await tickOnce();
  } finally {
    ticking = false;
  }
}
async function tickOnce() {
  if (document.hidden) {
    // the silence count restarts when the window comes back
    soundAt = Date.now();
    return;
  }
  let s;
  try {
    s = await invoke('get_state', { micTest: tab === 'mic' });
  } catch {
    // no core, no level: an old clip warning would stay forever
    clippedAt = 0;
    soundAt = Date.now();
    paintGainHint('');
    return;
  }
  const lv = s.levels || {};
  for (const [id, st] of Object.entries(strips)) {
    const [l, r] = lv[id] || [0, 0];
    st.l = decay(st.l, frac(l));
    st.r = decay(st.r, frac(r));
    st.setL(st.l);
    st.setR(st.r);
    st.dot.className = st.l > 0.05 ? 'dot on' : 'dot';
    if (id !== 'master') {
      const names = (s.apps || []).filter((a) => a.channel === id).map((a) => a.name);
      const key = names.join('\n');
      if (key !== st.appsKey) {
        st.appsKey = key;
        st.apps.replaceChildren(...names.map((n) => el('span', null, n)));
      }
    }
  }
  // full scale from the device: clipped in its own preamp, no processing can undo that
  if (lv.mic_raw >= 0.99) clippedAt = Date.now();
  // an empty jack or a wrong default source gives digital silence or hiss far below -70 dBFS,
  // a real microphone in a quiet room does not; only a hint, the device is never switched
  if (!(lv.mic_raw >= 0 && lv.mic_raw <= 0.0003) || cfg.mic.mute) soundAt = Date.now();
  paintGainHint(Date.now() - clippedAt < 5000 ? 'clip' : Date.now() - soundAt > 10000 ? 'silent' : '');
  micLevel = lv.mic >= 0 ? decay(micLevel, frac(lv.mic)) : 0;
  micInLevel = lv.mic_in >= 0 ? decay(micInLevel, frac(lv.mic_in)) : 0;
  setMicMeter(micLevel);
  setMicInMeter(micInLevel);
  $('mic-state').textContent = cfg.mic.mute ? t('stumm') : !s.mic_active ? ((s.devices?.sources ?? devices.sources).length ? t('Mikrofon aus') : t('kein Mikrofon')) : t('aktiv');
  $('mic-state').disabled = !cfg.mic.mute;
  paintEvents(s.events || []);
  apps = s.apps || [];
  paintApps();
}

const CORE_CRASHED = 'Der Audio-Kern ist ausgefallen. Mixpilot startet ihn gleich neu, bis dahin spielen Apps direkt über dein Gerät.';
const CORE_DOWN = 'Der Audio-Kern startet nicht wieder.';
let coreWasOn = false;
let coreOffSince = 0;
async function pollCore() {
  const on = await invoke('core_status').catch(() => false);
  // only after it ran once: before that the setup check explains why it does not start
  if (on) {
    coreWasOn = true;
    coreOffSince = 0;
    clearError(CORE_CRASHED);
    clearError(CORE_DOWN);
  } else if (coreWasOn) {
    coreOffSince ||= Date.now();
    if (Date.now() - coreOffSince > 30000) {
      showError(CORE_DOWN, 4);
    } else {
      showError(CORE_CRASHED, 3);
    }
  }
  const s = on ? t('Audio-Kern läuft') : t('Audio-Kern gestoppt');
  // live region: only a real change is announced
  if ($('core-status').textContent !== s) $('core-status').textContent = s;
  $('core-status').className = on ? 'core-status' : 'core-status off';
  $('start-core').hidden = on;
}

// ---------- setup ----------
const SNAP_PLUGS = {
  pipewire: t('PipeWire: damit Mixpilot Kanäle anlegen, Apps einsortieren und Effekte rechnen kann.'),
  'audio-record': t('Mikrofon: für Rauschunterdrückung und dein bearbeitetes "Mixpilot Mikrofon".'),
};

const CHECK_SVG = '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M2 6.5 5 9.5 10 3" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"></path></svg>';

async function runChecks() {
  const r = await invoke('setup_check');
  const core = await invoke('core_status').catch(() => false);
  const items = [];
  items.push(r.pipewire ? { kind: 'ok', text: t('PipeWire läuft') } : { kind: 'bad', text: t('PipeWire läuft nicht. Mixpilot braucht PipeWire (Standard ab Ubuntu 22.10).') });
  for (const name of r.conflicts) {
    items.push({ kind: 'warn', text: t('{0} läuft. Bitte schließen, sonst zieht es die App-Streams immer wieder zu sich und die Kanäle halten nicht.', name) });
  }
  if (r.snap && r.snap_missing.length) {
    const n = r.snap_missing.length;
    items.push({
      kind: 'bad',
      text: n === 1 ? t('Mixpilot braucht noch eine Freigabe vom System') : t('Mixpilot braucht noch {0} Freigaben vom System', n),
      sub: t('Snap-Apps laufen abgeschottet. Du erteilst die Freigaben einmal im Terminal, danach merkt sich das System sie.'),
      details: r.snap_missing.map((p) => SNAP_PLUGS[p] || p),
      cmd: r.snap_missing.map((p) => `sudo snap connect mixpilot:${p}`).join(' && '),
    });
  } else if (r.snap) {
    items.push({ kind: 'ok', text: t('Alle Snap-Freigaben sind erteilt') });
  }
  items.push(core ? { kind: 'ok', text: t('Audio-Kern läuft') } : { kind: 'bad', text: t('Audio-Kern läuft noch nicht'), sub: r.snap && r.snap_missing.length ? t('Er startet, sobald die Freigaben erteilt sind. Danach "Erneut prüfen" klicken.') : t('"Erneut prüfen" startet ihn.') });

  const ul = $('checks');
  ul.replaceChildren();
  for (const it of items) {
    const li = el('li', it.kind);
    const mark = el('span', 'mark');
    if (it.kind === 'ok') mark.innerHTML = CHECK_SVG;
    else mark.textContent = '!';
    const body = el('div', 'check-body');
    body.append(el('div', 'check-title', it.text));
    if (it.sub) body.append(el('p', 'hint', it.sub));
    if (it.details) {
      const list = el('ul', 'check-list');
      for (const d of it.details) list.append(el('li', null, d));
      body.append(list);
    }
    if (it.cmd) {
      const row = el('div', 'cmd-row');
      const code = el('code', null, it.cmd);
      const copy = el('button', 'btn small', t('Kopieren'));
      copy.addEventListener('click', async () => {
        try {
          await navigator.clipboard.writeText(it.cmd);
          copy.textContent = t('Kopiert');
        } catch {
          // clipboard blocked: select the text so Ctrl+C works
          const range = document.createRange();
          range.selectNodeContents(code);
          getSelection().removeAllRanges();
          getSelection().addRange(range);
          copy.textContent = t('Strg+C');
        }
        setTimeout(() => (copy.textContent = t('Kopieren')), 2000);
      });
      row.append(code, copy);
      body.append(row);
    }
    li.append(mark, body);
    ul.append(li);
  }
  // finishing needs a running core: it owns the settings file
  $('setup-done').disabled = !core;
  $('setup-done').title = core ? '' : t('Geht, sobald der Audio-Kern läuft');
}

// ---------- what's new ----------
// Shown once after an update. `version` stays null until the release that ships it sets it.
const NEWS = [
  {
    version: '0.0.7',
    title: 'Ruhigere Nächte, klarere Fehler',
    items: [
      {
        art: 'mixer',
        title: 'Sauberer Klang, leisere Nächte',
        points: [
          'Nachts werden laute Stellen leiser, statt dass alles bis zu 12 dB lauter wird',
          'Bass, Klarheit und EQ übersteuern nicht mehr, der Limiter verzerrt keinen Bass',
          'Mit starkem Bass oder EQ spielt ein Kanal dafür leiser, dreh ihn etwas auf',
          'Ducking greift nach 100 ms statt nach fast 1 Sekunde, auch bei leisem Chat-Regler',
          'Nach einem Gerätewechsel hängt der Ton nicht mehr hinterher',
        ],
      },
      {
        art: 'mute',
        title: 'Stimme und Mikrofon',
        points: [
          'Neue Stimm-Presets: Podcast und Laptop-Mikro',
          'Der Auto-Pegel kriecht in Pausen nicht mehr hoch, Lacher fängt er in 10 ms ab',
          'Kommt 10 Sekunden kein Ton, nennt die Mikrofon-Seite das Gerät und den Ausweg',
          'Übersteuert das Mikrofon schon vor Mixpilot, sagt sie das auch',
          'Stummschalten wirkt ab dem ersten Moment',
          'Ohne Mikrofon hören die anderen Stille statt deinem Spielton',
        ],
      },
      {
        art: 'error',
        title: 'Fehler mit Ausweg',
        points: [
          'Fehlermeldungen sagen, was du tun kannst, etwa welches Gerät du prüfen solltest',
          '„Fehler melden“ öffnet den Bericht mit der Meldung schon ausgefüllt',
          'Fällt der Audio-Kern aus, sagt Mixpilot das sofort und nach 30 s, was zu tun ist',
          'Meldungen lassen sich schließen und gehen von selbst, wenn die Ursache weg ist',
          'Im Bericht kannst du jetzt auch Wünsche loswerden, ein Wunsch allein reicht',
        ],
      },
    ],
  },
  {
    version: '0.0.6',
    title: 'Leichter für alle',
    items: [
      {
        art: 'mixer',
        title: 'Mit Tastatur und Screenreader',
        text: 'Die Tabs wechseln mit den Pfeiltasten, Dialoge behalten den Fokus, jeder Regler sagt seinen Wert an und Fehler werden vorgelesen. Strg+Plus macht alles größer.',
      },
      {
        art: 'mute',
        title: 'Push-to-Mute hält',
        text: 'Ist der Desktop beim Anmelden noch nicht bereit oder startet sein Dienst für Tastenkürzel neu, verbindet sich die Taste von selbst wieder. Andere Apps können sie nicht mehr auslösen.',
      },
      {
        art: 'monitor',
        title: 'Ganzer Ton bei großem Puffer',
        text: 'Läuft PipeWire mit großem Puffer, kommt der Ton ohne Lücken an, auch beim Mithören. Ein hängender Audio-Kern wird auch bei geschlossenem Fenster ersetzt.',
      },
    ],
  },
  {
    version: '0.0.5',
    title: 'Stabiler im Alltag',
    items: [
      {
        art: 'mixer',
        title: 'Läuft einfach weiter',
        text: 'Startet PipeWire neu, ist der Ton nach Sekunden von selbst wieder da. Ein hängender Audiodienst friert Mixpilot nicht mehr ein, und doppelt gestartet läuft es trotzdem nur einmal.',
      },
      {
        art: 'monitor',
        title: 'Mithören ohne Aussetzer',
        text: 'Sind Mikrofon und Headset getrennte Geräte, laufen ihre Takte leicht auseinander. Mixpilot gleicht das aus, das Mithören bleibt sauber.',
      },
      {
        art: 'reset',
        title: 'Einstellungen in Sicherheit',
        text: 'Ist die Einstellungsdatei beschädigt, etwa nach einem Stromausfall, sichert Mixpilot sie, lädt eine funktionierende Fassung und sagt dir Bescheid.',
      },
    ],
  },
];

// LED columns like the level meters. Each art returns a step function the dialog calls every 70 ms.
function ledColumns(parent, cols, rows) {
  parent.style.setProperty('--rows', rows);
  return Array.from({ length: cols }, () => {
    const col = el('div', 'ledcol');
    const segs = Array.from({ length: rows }, () => col.appendChild(el('i')));
    parent.append(col);
    return segs;
  });
}

function headLeds(parent) {
  const rows = 7;
  const cols = ledColumns(parent, 4, rows);
  const level = cols.map(() => Math.random() * rows);
  return () => cols.forEach((segs, c) => {
    level[c] = Math.max(1, Math.min(rows, level[c] + (Math.random() - 0.5) * 3));
    // segments run bottom to top: blue, the top two amber
    segs.forEach((s, i) => {
      const h = rows - 1 - i;
      s.className = h < level[c] ? (h >= rows - 2 ? 'mid' : 'low') : '';
    });
  });
}

// The pictures are the real interface in miniature: same markup classes and styles, scaled down,
// and inert so nothing in them can be clicked. A transform and not zoom, WebKit keeps a minimum
// font size under zoom and the text would overflow.
function mini(box, scale, width, cls = 'card') {
  const stage = el('div', 'mini-ui');
  stage.inert = true;
  stage.dataset.scale = scale;
  stage.style.width = `${width * scale}px`;
  const root = el('div', cls);
  Object.assign(root.style, { width: `${width}px`, transform: `scale(${scale})`, transformOrigin: '0 0' });
  stage.append(root);
  box.append(stage);
  return root;
}

// a transform does not shrink the layout box, so the stage gets its height once the dialog is shown
function fitMinis() {
  document.querySelectorAll('.mini-ui').forEach((s) => (s.style.height = `${s.firstElementChild.offsetHeight * s.dataset.scale}px`));
}

// the push-to-mute part of the microphone tab: each key press flips aktiv and stumm
function muteArt(box) {
  const card = mini(box, 0.66, 400);
  const state = el('button', 'mono mic-state');
  const head = el('div', 'card-head');
  head.append(el('span', 'label', t('MIKROFON')), state);
  const meter = el('div', 'hmeter');
  const set = makeSegments(meter, 24);
  const row = el('div', 'meter-row');
  row.append(el('span', 'meter-label', t('Nach Bearbeitung')), meter);
  const sw = el('button', 'switch');
  sw.setAttribute('aria-checked', 'true');
  sw.append(el('span'));
  const ptm = el('div', 'card-head line');
  ptm.append(el('span', 'title small', 'Push-to-Mute'), sw);
  const key = el('button', 'btn small', t('Strg+Alt+M'));
  const keyRow = el('div', 'card-head');
  keyRow.append(el('span', 'title small', t('Taste')), key);
  card.append(head, row, ptm, keyRow);
  let tick = 0;
  let muted = true;
  let level = 0.6;
  return () => {
    const phase = tick++ % 40;
    if (phase === 0) {
      muted = !muted;
      state.textContent = muted ? t('stumm') : t('aktiv');
      state.disabled = !muted;
    }
    key.classList.toggle('pressed', phase < 3);
    level = Math.max(0.35, Math.min(0.85, level + (Math.random() - 0.45) * 0.2));
    set(muted ? 0 : level);
  };
}

// the mixer strips as they are in the mixer tab, meters moving
function mixerArt(box) {
  const strips = mini(box, 0.33, 800, 'strips');
  const steps = [];
  const strip = (id, label, vol, val, eq, app) => {
    const master = id === 'master';
    const root = el('div', master ? 'strip master' : 'strip');
    const head = el('div', 'strip-head');
    head.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="${ICONS[id]}"></path></svg>`;
    head.append(el('span', 'strip-name', label), el('span', 'dot on'));
    const meter = el('div', 'meter');
    const cl = el('div', 'col');
    const cr = el('div', 'col');
    meter.append(cl, cr);
    const setL = makeSegments(cl, 20);
    const setR = makeSegments(cr, 20);
    const fader = el('input', 'fader');
    Object.assign(fader, { type: 'range', min: 0, max: 100, value: vol });
    const fwrap = el('div', 'fwrap');
    fwrap.append(fader);
    const body = el('div', 'strip-body');
    body.append(meter, fwrap);
    const value = el('input', 'num value');
    value.value = val;
    const btns = el('div', 'btns');
    btns.append(el('button', 'mute', t('STUMM')));
    root.append(head, body, value, btns);
    if (master) root.append(el('p', 'hint', t('Systemlautstärke deines Ausgabegeräts')));
    else {
      const chip = el('button', 'eqchip');
      chip.append(el('b', null, 'EQ'), el('span', null, eq));
      btns.append(chip);
      const apps = el('div', 'appchips');
      if (app) apps.append(el('span', null, app));
      root.append(apps);
    }
    let l = 0.4 + Math.random() * 0.4;
    steps.push(() => {
      l = Math.max(0.25, Math.min(0.95, l + (Math.random() - 0.5) * 0.12));
      setL(l);
      setR(Math.max(0, l - 0.04));
    });
    return root;
  };
  strips.append(
    strip('game', 'Game', 88, '-2.2 dB', 'Gaming', 'SuperTuxKart'),
    strip('chat', 'Chat', 100, '0.0 dB', t('Stimme'), 'Discord'),
    strip('media', 'Media', 78, '-7.8 dB', 'Flat', 'Firefox'),
    strip('music', t('Musik'), 70, '-9.3 dB', 'Flat', 'Spotify'),
    strip('aux', 'Aux', 60, '-13.3 dB', 'Flat', ''),
    el('div', 'sep'),
    strip('master', 'Master', 68, '68 %'),
  );
  return () => steps.forEach((s) => s());
}

// the microphone meters with monitoring on, both moving without a gap
function monitorArt(box) {
  const card = mini(box, 0.66, 400);
  const head = el('div', 'card-head');
  const state = el('button', 'mono mic-state', t('aktiv'));
  state.disabled = true;
  head.append(el('span', 'label', t('MIKROFON')), state);
  const meters = [t('Eingang'), t('Nach Bearbeitung')].map((label) => {
    const meter = el('div', 'hmeter');
    const row = el('div', 'meter-row');
    row.append(el('span', 'meter-label', label), meter);
    card.append(row);
    return makeSegments(meter, 24);
  });
  const sw = el('button', 'switch');
  sw.setAttribute('aria-checked', 'true');
  sw.append(el('span'));
  const mon = el('div', 'card-head line');
  mon.append(el('span', 'title small', t('Mithören')), sw);
  card.prepend(head);
  card.append(mon);
  let level = 0.6;
  return () => {
    level = Math.max(0.35, Math.min(0.85, level + (Math.random() - 0.45) * 0.2));
    meters[0](level);
    meters[1](Math.max(0, level - 0.08));
  };
}

// the note the window shows after the core replaced a damaged settings file
function resetArt(box) {
  const dialog = mini(box, 0.48, 580, 'dialog');
  // a real dialog is capped to the window height, the miniature is not
  Object.assign(dialog.style, { maxHeight: 'none', overflow: 'visible' });
  const actions = el('div', 'actions');
  actions.append(el('button', 'btn primary', t('Alles klar')));
  dialog.append(
    el('h1', null, t('Einstellungen ersetzt')),
    el('p', null, t('Die Einstellungsdatei konnte nicht gelesen werden. Mixpilot hat sie gesichert und eine funktionierende Fassung geladen. Prüf kurz, ob alles passt.')),
    el('p', 'mono path', '~/.config/mixpilot/config.json.broken-20261016-121503'),
    actions,
  );
  return () => {};
}

function errorArt(box) {
  const banner = mini(box, 0.5, 520, 'error');
  banner.append(t('Wiedergabe fehlgeschlagen'), el('span', 'error-hint', t(HINTS['Wiedergabe fehlgeschlagen'])), el('button', 'error-report', t('Fehler melden')));
  return () => {};
}

const NEWS_ART = { mute: muteArt, mixer: mixerArt, monitor: monitorArt, reset: resetArt, error: errorArt };
let newsTimer = null;

function openNews(entry) {
  $('news-leds-l').replaceChildren();
  $('news-leds-r').replaceChildren();
  const steps = [headLeds($('news-leds-l')), headLeds($('news-leds-r'))];
  $('news-logo').replaceChildren(document.querySelector('.brand svg').cloneNode(true));
  $('news-version').textContent = entry.version ? t('Version {0}: {1}', entry.version, t(entry.title)) : t(entry.title);
  const list = $('news-items');
  list.replaceChildren();
  for (const it of entry.items) {
    const row = el('div', 'news-item');
    const art = el('div', 'news-art');
    steps.push(NEWS_ART[it.art](art));
    const text = el('div', 'news-text');
    const body = it.points ? el('ul', 'hint') : el('p', 'hint', t(it.text));
    for (const p of it.points || []) body.append(el('li', null, t(p)));
    text.append(el('h2', null, t(it.title)), body);
    row.append(art, text);
    list.append(row);
  }
  const step = () => steps.forEach((s) => s());
  step();
  clearInterval(newsTimer);
  if (!matchMedia('(prefers-reduced-motion: reduce)').matches) newsTimer = setInterval(step, 70);
  openDialog($('news'));
  fitMinis();
}

function closeNews() {
  clearInterval(newsTimer);
  closeDialog($('news'));
}

// The about dialog shows the notes of this version, or the newest ones in a development build.
async function showNews() {
  const a = await invoke('app_info').catch(() => null);
  const entry = NEWS.find((n) => n.version === a?.version) || NEWS[0];
  openNews(entry);
}

async function newsAfterUpdate() {
  const a = await invoke('app_info').catch(() => null);
  if (!a || cfg.seen_version === a.version) return;
  cfg.seen_version = a.version;
  save();
  const entry = NEWS.find((n) => n.version === a.version);
  if (entry) openNews(entry);
}

// ---------- start ----------
async function loadConfig(tries) {
  for (let i = 0; i < tries; i++) {
    const c = await invoke('get_config').catch(() => null);
    if (c) return c;
    await sleep(150);
  }
  return null;
}

// title bar and device pickers work from the first moment, also during setup
function bindChrome() {
  const win = window.__TAURI__.window?.getCurrentWindow?.();
  $('win-min').addEventListener('click', () => win?.minimize());
  $('win-max').addEventListener('click', () => win?.toggleMaximize());
  $('win-close').addEventListener('click', () => win?.close());
  if (win) {
    const square = async () => document.documentElement.classList.toggle('square', (await win.isMaximized()) || (await win.isFullscreen()));
    square();
    win.onResized(square);
  }
  const info = $('support-info');
  $('support').addEventListener('click', () => openDialog(info));
  $('support-cancel').addEventListener('click', () => closeDialog(info));
  dismissOn(info, () => closeDialog(info));
  $('support-go').addEventListener('click', () => {
    closeDialog(info);
    invoke('open_support').catch(showError);
  });
  const reset = $('reset-info');
  const closeReset = async () => {
    closeDialog(reset);
    // ask first: a pollConfig during the wait still sees the same note and does not reopen it
    const version = cfg.seen_version ?? (await invoke('app_info').catch(() => null))?.version;
    // null tells set_config the note was shown; the file had been in use, so setup is not needed
    cfg.config_reset = null;
    cfg.setup_done = true;
    cfg.seen_version = version;
    save();
  };
  $('reset-ok').addEventListener('click', closeReset);
  reset.addEventListener('keydown', (e) => { if (e.key === 'Escape') closeReset(); });
  $('bug').addEventListener('click', () => invoke('open_bug_report').catch(showError));
  const about = $('about');
  const closeAbout = () => closeDialog(about);
  $('about-btn').addEventListener('click', async () => {
    const a = await invoke('app_info').catch(() => null);
    if (a) {
      $('about-version').textContent = t('Version {0}, {1}', a.version, a.package);
      $('about-contact').textContent = t('Kontakt: {0}', a.email);
    }
    $('licenses-box').open = false;
    openDialog(about);
  });
  $('about-close').addEventListener('click', closeAbout);
  dismissOn(about, closeAbout);
  $('licenses-box').addEventListener('toggle', async () => {
    // the dialog has no room for both: the license texts take the place of privacy and credits
    about.classList.toggle('licenses-open', $('licenses-box').open);
    if ($('licenses').textContent) return;
    $('licenses').textContent = await fetch('third-party-licenses.txt').then((r) => r.text()).catch(() => '');
  });
  $('quit').addEventListener('click', () => invoke('quit_app'));
  $('about-news').addEventListener('click', () => { closeAbout(); showNews(); });
  const news = $('news');
  $('news-close').addEventListener('click', closeNews);
  dismissOn(news, closeNews);
  $('start-core').addEventListener('click', async () => {
    await invoke('ensure_core').catch(showError);
    setTimeout(pollCore, 600);
  });
  bindDevices();
}

let initialized = false;
async function init() {
  if (initialized) return;
  initialized = true;
  defaults(cfg);
  const box = $('strips');
  for (const [id, label] of CHANNELS) box.append(strip(id, label, false));
  box.append(el('div', 'sep'), strip('master', 'Master', true));
  // a rotated range keeps its horizontal width, so the fader length follows the strip height by hand
  const fit = new ResizeObserver((entries) => entries.forEach(({ target: w }) => {
    const h = w.clientHeight;
    Object.assign(w.firstElementChild.style, { width: `${h}px`, left: `${(44 - h) / 2}px`, top: `${(h - 44) / 2}px` });
  }));
  // each wrapper on its own: app chips change a strip's inner height while #strips stays the same
  box.querySelectorAll('.fwrap').forEach((w) => fit.observe(w));
  bindRange('chatmix', () => cfg.chatmix, (v) => (cfg.chatmix = v));
  painters.push(paintCardsMixer);
  buildSound();
  buildMic();
  buildAuto();
  for (const b of document.querySelectorAll('.tab')) {
    b.addEventListener('click', () => {
      tab = b.dataset.tab;
      editFor = null;
      document.querySelectorAll('.tab').forEach((t) => { t.setAttribute('aria-selected', String(t === b)); t.tabIndex = t === b ? 0 : -1; });
      document.querySelectorAll('.page').forEach((p) => (p.hidden = p.id !== `tab-${tab}`));
    });
  }
  // arrow keys move between the tabs, Tab leaves the row
  document.querySelector('.tablist').addEventListener('keydown', (e) => {
    const tabs = [...document.querySelectorAll('.tab')];
    const i = tabs.indexOf(document.activeElement);
    const j = { ArrowLeft: i - 1, ArrowRight: i + 1, Home: 0, End: tabs.length - 1 }[e.key];
    if (j === undefined) return;
    e.preventDefault();
    const b = tabs[(j + tabs.length) % tabs.length];
    b.click();
    b.focus();
  });
  document.addEventListener('click', (e) => { if (eqMenu && !eqMenu.box.contains(e.target)) closeEqMenu(); });
  addEventListener('resize', () => closeEqMenu());
  paintAll();
  paintApps();
  await refreshDevices();
  setInterval(tick, 50);
  setInterval(() => document.hidden || refreshDevices(), 5000);
  setInterval(pollConfig, 1000);
  document.addEventListener('visibilitychange', pollConfig);
  pollMaster();
  setInterval(pollMaster, 400);
}

// First start (or a snap without its permissions yet): the setup dialog works without a config;
// "Fertig" waits for the core, which creates the settings.
async function openSetup() {
  openDialog($('setup'));
  $('recheck').addEventListener('click', async () => {
    await invoke('ensure_core').catch(() => {});
    await sleep(800);
    if (!cfg) cfg = await loadConfig(10);
    await refreshDevices();
    runChecks();
  });
  $('setup-done').addEventListener('click', async () => {
    if (!cfg) cfg = await loadConfig(10);
    if (!cfg) return runChecks();
    if (pending.output) cfg.output = pending.output;
    if (pending.input) cfg.input = pending.input;
    await invoke('set_autostart', { on: $('setup-autostart').checked }).catch(showError);
    cfg.setup_done = true;
    // a fresh install has nothing to catch up on
    cfg.seen_version = (await invoke('app_info').catch(() => null))?.version;
    save();
    closeDialog($('setup'));
    await init();
  });
  // after the handlers, so a failing check never leaves the buttons dead
  await refreshDevices();
  await runChecks().catch(showError);
}

async function main() {
  translatePage();
  bindChrome();
  pollCore();
  setInterval(pollCore, 2000);
  await invoke('ensure_core').catch(() => {});
  // the core writes the config on its first start
  cfg = await loadConfig(14);
  if (cfg && (cfg.setup_done || cfg.config_reset)) {
    await init();
    if (cfg.config_reset) showReset(cfg.config_reset);
    else newsAfterUpdate();
  } else await openSetup();
}

// The core kept an unreadable settings file under this name and put a working one in its place.
function showReset(path) {
  cfg.config_reset = path;
  $('reset-file').textContent = path;
  openDialog($('reset-info'));
}

main().catch(showError);
