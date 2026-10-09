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
  media: 'M9 18V5l12-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM21 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0z',
  aux: 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM7.8 7.8a6 6 0 0 0 0 8.4M16.2 7.8a6 6 0 0 1 0 8.4M5 5a10 10 0 0 0 0 14M19 5a10 10 0 0 1 0 14',
  master: 'M11 5 6 9H2v6h4l5 4V5zM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14',
};
const CHANNELS = [['game', 'Game'], ['chat', 'Chat'], ['media', 'Media'], ['aux', 'Aux']];
const CH_EQ = [['flat', 'Flat'], ['bass', 'Bass'], ['voice', t('Stimme')], ['gaming', 'Gaming'], ['clear', t('Klar')]];
const EQ_PRESETS = {
  Flat: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  Bass: [6, 5, 3, 1, 0, 0, 0, 0, 0, 0],
  Stimme: [-3, -2, -1, 0, 2, 3, 3, 2, 0, -1],
  Gaming: [3, 4, 2, 0, -1, 0, 1, 3, 2, 1],
  Klar: [0, 0, -1, -1, 0, 1, 2, 3, 3, 2],
};
const FREQ = ['31', '62', '125', '250', '500', '1k', '2k', '4k', '8k', '16k'];
const AUTOVOL = [
  ['off', t('Aus'), t('Kein Eingriff in die Lautstärke.')],
  ['soft', t('Sanft'), t('Gleicht Lautstärkesprünge zwischen Apps und Szenen behutsam aus.')],
  ['night', t('Nacht'), t('Leise Stellen lauter, Explosionen leiser. Für spät abends.')],
];
const NOISE = [['off', t('Aus')], ['normal', 'Normal'], ['strong', t('Stark')]];
const DEESS = NOISE;
const VOICE = [['natural', t('Natürlich')], ['warm', 'Warm'], ['radio', 'Radio'], ['broadcast', 'Broadcast']];
const VOICE_TEXT = {
  natural: t('Deine Stimme wie sie ist, nur Rumpeln unter 80 Hz wird entfernt.'),
  warm: t('Voller und weicher: mehr Körper, weniger Schärfe und Zischen.'),
  radio: t('Telefon- und Funkklang, nur das schmale Sprachband.'),
  broadcast: t('Moderatorenstimme: nah, präsent, dicht komprimiert.'),
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
};

let cfg = null;
let tab = 'mixer';
// device choices made in the setup dialog before the core has written a config
const pending = { output: '', input: '' };
const devCfg = () => cfg || pending;
// Master is the output device's own volume (the one GNOME and the volume keys change), not a Mixpilot stage
const masterDev = { volume: 0, mute: false, known: false };
// One volume command at a time, always with the newest value: parallel pactl calls can finish out of
// order and make the fader jump back and forth.
let masterPending = null;
let masterBusy = false;
let masterTouched = 0;
async function setMaster(v) {
  masterDev.volume = v;
  masterTouched = Date.now();
  masterPending = v;
  if (masterBusy) return;
  masterBusy = true;
  while (masterPending !== null) {
    const next = masterPending;
    masterPending = null;
    await invoke('set_master', { volume: next }).then(() => (masterDev.known = true), showError);
  }
  masterBusy = false;
  masterTouched = Date.now();
}
async function pollMaster() {
  // never read back while the user is moving the fader or right after: the device may not be there yet
  if (document.hidden || masterBusy || Date.now() - masterTouched < 1200) return;
  try {
    const m = await invoke('get_master');
    Object.assign(masterDev, m, { known: true });
  } catch {
    masterDev.known = false;
  }
  paintAll();
}
let devices = { sinks: [], sources: [], default_sink: '', default_source: '' };
const painters = [];
const paintAll = () => painters.forEach((p) => p());

function showError(e) {
  $('error').textContent = t(String(e));
  $('error').hidden = false;
}

// Faders follow the core's cubic taper: gain = (v/100)^3
const fdb = (v) => (v <= 0 ? '-∞ dB' : `${(60 * Math.log10(v / 100)).toFixed(1)} dB`);

function defaults(c) {
  c.auto ??= true;
  for (const [id] of CHANNELS) {
    c[id] ??= {};
    c[id].volume ??= 100;
    c[id].mute ??= false;
    c[id].eq ??= 'flat';
  }
  c.master ??= 100;
  c.chatmix ??= 0;
  c.bass ??= 0;
  c.limiter ??= true;
  c.clarity ??= false;
  c.auto_volume ??= 'off';
  c.eq ??= [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  c.eq_presets = Object.fromEntries(Object.entries(c.eq_presets || {})
    .filter(([, g]) => Array.isArray(g) && g.length === 10 && g.every(Number.isFinite)));
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
  c.rules ??= [];
  return c;
}

// Save at most every 60 ms while dragging, always save the final value.
let lastSave = 0;
let saveTimer = null;
function save() {
  clearTimeout(saveTimer);
  const run = () => {
    lastSave = Date.now();
    invoke('set_config', { cfg }).then(() => ($('error').hidden = true), showError);
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

function bindRange(id, get, set) {
  const r = $(id);
  r.addEventListener('input', change(() => set(Number(r.value))));
  painters.push(() => { if (document.activeElement !== r) r.value = get(); });
}

// Mouse wheel over a control changes it (up = more, Shift = 10 steps); the page itself never scrolls.
document.addEventListener('wheel', (e) => {
  const t = e.target.closest('input');
  if (!t) return;
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
  head.append(el('span', 'strip-name', label), dot);
  const body = el('div', 'strip-body');
  const meter = el('div', 'meter');
  const cl = el('div', 'col');
  const cr = el('div', 'col');
  meter.append(cl, cr);
  const setL = makeSegments(cl, 20);
  const setR = makeSegments(cr, 20);
  const fwrap = el('div', 'fwrap');
  const fader = el('input', master ? 'fader master-fader' : 'fader');
  Object.assign(fader, { type: 'range', min: 0, max: 100, step: 1 });
  fader.setAttribute('aria-label', t('{0} Lautstärke', label));
  fwrap.append(fader);
  body.append(meter, fwrap);
  const value = el('input', 'value');
  root.append(head, body, value);

  const get = () => (master ? masterDev.volume : cfg[id].volume);
  if (master) {
    fader.max = 100;
    fader.addEventListener('pointerdown', () => (masterTouched = Date.now()));
    fader.addEventListener('input', () => { setMaster(Number(fader.value)); paintAll(); });
  } else {
    fader.addEventListener('input', change(() => (cfg[id].volume = Number(fader.value))));
  }
  let mute = null;
  let chip = null;
  const apps = el('div', 'appchips');
  if (master) {
    const btns = el('div', 'btns');
    mute = el('button', 'mute', t('STUMM'));
    mute.addEventListener('click', async () => {
      masterDev.mute = !masterDev.mute;
      masterTouched = Date.now();
      paintAll();
      await invoke('set_master_mute', { mute: masterDev.mute }).catch(showError);
    });
    btns.append(mute);
    root.append(btns, el('p', 'hint', t('Systemlautstärke deines Ausgabegeräts')));
  } else {
    const btns = el('div', 'btns');
    mute = el('button', 'mute', t('STUMM'));
    mute.addEventListener('click', change(() => (cfg[id].mute = !cfg[id].mute)));
    chip = el('button', 'eqchip');
    chip.addEventListener('click', change(() => {
      const i = CH_EQ.findIndex(([k]) => k === cfg[id].eq);
      cfg[id].eq = CH_EQ[(i + 1) % CH_EQ.length][0];
    }));
    btns.append(mute, chip);
    root.append(btns, apps);
  }
  painters.push(() => {
    if (document.activeElement !== fader) fader.value = get();
    const muted = master ? masterDev.mute : cfg[id].mute;

    if (mute) mute.setAttribute('aria-pressed', String(muted));
    if (chip) {
      const name = (CH_EQ.find(([k]) => k === cfg[id].eq) || CH_EQ[0])[1];
      chip.replaceChildren(el('b', null, 'EQ'), el('span', null, name));
      chip.setAttribute('aria-label', t('Klang {0}: {1}, wechseln', label, name));
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
      min: 0, max: 100, step: 1,
      toValue: (db) => (db <= -60 ? 0 : 100 * Math.pow(10, Math.min(0, db) / 60)),
      show: () => (cfg[id].mute ? t('stumm') : fdb(cfg[id].volume)),
    });
  }
  strips[id] = { root, setL, setR, dot, apps, l: 0, r: 0, appsKey: '' };
  return root;
}

function paintCardsMixer() {
  const c = cfg.chatmix;
  $('cm-text').textContent = `Game ${c > 0 ? 100 - c : 100} %   Chat ${c < 0 ? 100 + c : 100} %`;
}

// ---------- sound ----------
// more chips would wrap the card into a second row and push the page into scrolling
const EQ_OWN_MAX = 4;
function eqPreset() {
  const same = ([, g]) => g.every((v, i) => v === cfg.eq[i]);
  const b = Object.entries(EQ_PRESETS).find(same);
  if (b) return { name: b[0], label: t(b[0]) };
  const u = Object.entries(cfg.eq_presets).find(same);
  return u ? { name: u[0], label: u[0], own: true } : { name: '', label: t('Eigenes') };
}

function ownPresetChips(box) {
  let naming = false;
  let key = null;
  const nodes = [];
  const chip = (text, cls, label, onClick) => {
    const b = el('button', cls, text);
    if (label) b.setAttribute('aria-label', label);
    b.addEventListener('click', onClick);
    return b;
  };
  const nameField = () => {
    const input = el('input', 'chip-name');
    Object.assign(input, { type: 'text', maxLength: 20, spellcheck: false, placeholder: t('Name') });
    input.setAttribute('aria-label', t('Name für das eigene Preset'));
    const builtIn = (n) => n === 'Eigenes' || Object.keys(EQ_PRESETS).some((k) => k === n || t(k) === n);
    const done = () => { naming = false; paintAll(); };
    const commit = () => {
      const n = input.value.trim();
      if (!naming) return;
      if (!n) return done();
      if (builtIn(n)) {
        input.setAttribute('aria-invalid', 'true');
        input.title = t('So heißt schon ein eingebautes Preset');
        return;
      }
      cfg.eq_presets[n] = cfg.eq.slice();
      naming = false;
      paintAll();
      save();
    };
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') commit();
      else if (e.key === 'Escape') done();
    });
    input.addEventListener('input', () => input.removeAttribute('aria-invalid'));
    input.addEventListener('blur', () => { if (input.getAttribute('aria-invalid')) done(); else commit(); });
    return input;
  };
  return (cur) => {
    const names = Object.keys(cfg.eq_presets);
    const action = naming ? 'name' : cur.own ? 'delete' : cur.name ? '' : names.length < EQ_OWN_MAX ? 'save' : 'full';
    const k = JSON.stringify([names, cur.name, action]);
    if (k === key) return;
    key = k;
    nodes.splice(0).forEach((n) => n.remove());
    for (const n of names) {
      const b = chip(n, 'own', null, change(() => (cfg.eq = cfg.eq_presets[n].slice())));
      b.setAttribute('aria-pressed', String(n === cur.name));
      nodes.push(b);
    }
    if (action === 'save') {
      const b = chip(t('+ Speichern'), 'ghost', t('Aktuelle Kurve als eigenes Preset speichern'), () => { naming = true; paintAll(); });
      nodes.push(b);
    } else if (action === 'full') {
      const b = chip(t('+ Speichern'), 'ghost', null, () => {});
      b.setAttribute('aria-disabled', 'true');
      b.title = t('Höchstens {0} eigene Presets, lösch zuerst eins', EQ_OWN_MAX);
      nodes.push(b);
    } else if (action === 'delete') {
      nodes.push(chip(t('Löschen'), 'ghost', t('Preset {0} löschen', cur.name), change(() => delete cfg.eq_presets[cur.name])));
    } else if (action === 'name') {
      nodes.push(nameField());
    }
    box.append(...nodes);
    if (action === 'name') nodes.at(-1).focus();
  };
}

function buildSound() {
  bindSwitch('bass-on', () => cfg.bass > 0, (on) => {
    if (on) cfg.bass = cfg.bass_last || 50;
    else { cfg.bass_last = cfg.bass; cfg.bass = 0; }
  });
  bindRange('bass', () => cfg.bass, (v) => (cfg.bass = v));
  numField($('bass-value'), { label: t('Bass Boost in Prozent'), get: () => cfg.bass, set: (v) => (cfg.bass = Math.round(v)), min: 0, max: 100, show: () => `${cfg.bass} %` });
  painters.push(() => ($('bass-text').textContent = cfg.bass ? t('+{0} dB unter 100 Hz, mit Schutz gegen Übersteuern.', (cfg.bass * 0.12).toFixed(1)) : t('Aus.')));
  buttonGroup('autovol', AUTOVOL, () => cfg.auto_volume, (v) => (cfg.auto_volume = v));
  painters.push(() => ($('autovol-text').textContent = (AUTOVOL.find(([k]) => k === cfg.auto_volume) || AUTOVOL[0])[2]));
  bindSwitch('limiter', () => cfg.limiter, (v) => (cfg.limiter = v));
  bindSwitch('clarity', () => cfg.clarity, (v) => (cfg.clarity = v));

  const presets = $('eq-presets');
  const pbtn = Object.keys(EQ_PRESETS).map((name) => {
    const b = el('button', null, t(name));
    b.addEventListener('click', change(() => (cfg.eq = EQ_PRESETS[name].slice())));
    presets.append(b);
    return [name, b];
  });
  const paintOwn = ownPresetChips(presets);
  const bands = $('bands');
  const bandEls = FREQ.map((f, i) => {
    const box = el('div', 'band');
    const val = numField(el('input'), {
      label: `Band ${f} Hz in dB`,
      get: () => cfg.eq[i],
      set: (v) => (cfg.eq[i] = Math.round(v)),
      min: -12, max: 12,
      show: () => (cfg.eq[i] > 0 ? '+' : '') + cfg.eq[i],
    });
    const wrap = el('div', 'bwrap');
    const s = el('input', 'bslider');
    Object.assign(s, { type: 'range', min: -12, max: 12, step: 1 });
    s.setAttribute('aria-label', `Band ${f} Hz`);
    s.addEventListener('input', change(() => (cfg.eq[i] = Number(s.value))));
    wrap.append(s);
    box.append(val, wrap, el('span', 'mono freq', f));
    bands.append(box);
    return [s, val];
  });
  painters.push(() => {
    const cur = eqPreset();
    $('eq-name').textContent = cur.label;
    pbtn.forEach(([n, b]) => b.setAttribute('aria-pressed', String(n === cur.name)));
    paintOwn(cur);
    bandEls.forEach(([s, val], i) => {
      if (document.activeElement !== s) s.value = cfg.eq[i];
    });
  });
  const paintGrid = eqLeds($('eq-grid'));
  painters.push(() => paintGrid(cfg.eq));
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
let setMicMeter = () => {};
let setMicInMeter = () => {};
function buildMic() {
  setMicMeter = makeSegments($('mic-meter'), 40);
  setMicInMeter = makeSegments($('mic-in-meter'), 40);
  painters.push(() => ($('voice-text').textContent = VOICE_TEXT[cfg.mic.voice] || ''));
  buttonGroup('noise', NOISE, () => cfg.mic.noise, (v) => (cfg.mic.noise = v));
  buttonGroup('deess', DEESS, () => cfg.mic.deess, (v) => (cfg.mic.deess = v));
  buttonGroup('voice', VOICE, () => cfg.mic.voice, (v) => (cfg.mic.voice = v));
  bindRange('mic-gain', () => cfg.mic.gain, (v) => (cfg.mic.gain = v));
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
    await invoke('unmute_mic').catch(showError);
    cfg.mic.mute = false;
    paintAll();
  });
  const test = $('mic-test');
  test.addEventListener('click', async () => {
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
    await invoke('set_default_source', { name: 'mixpilot_mic' }).catch(showError);
    await refreshDevices();
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
  const mic = devices.sources.find((s) => s.name === (cfg.input || devices.default_source));
  $('mic-name').textContent = mic ? mic.description.toUpperCase() : '';
}

// ---------- devices ----------
function fillSelect(select, list, current, defaultName, allowDefault) {
  const def = list.find((d) => d.name === defaultName);
  select.replaceChildren();
  if (allowDefault) select.add(new Option(def ? t('System-Standard ({0})', def.description) : t('System-Standard'), ''));
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
  if (loop) showError('Dein System-Standard für die Ausgabe ist ein Mixpilot-Kanal. Wähle oben bei Ausgabe dein Headset oder deine Lautsprecher.');
  if (devices.duplicate) showError('Mixpilot läuft zweimal, zum Beispiel als Snap und als deb. Beende oder deinstalliere eine der beiden Versionen, sonst gibt es jeden Kanal doppelt.');
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
  for (const s of document.querySelectorAll('.output-select')) {
    s.addEventListener('focus', refreshDevices);
    s.addEventListener('change', () => { devCfg().output = s.value; if (cfg) save(); paintDevices(); });
  }
  for (const s of document.querySelectorAll('.input-select')) {
    s.addEventListener('focus', refreshDevices);
    s.addEventListener('change', () => { devCfg().input = s.value; if (cfg) save(); paintDevices(); });
  }
}

// ---------- automation ----------
function buildAuto() {
  bindSwitch('auto', () => cfg.auto, (v) => (cfg.auto = v));
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
    painters.push(() => (s.value = String(cfg.night[key])));
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
  const focusKey = force && box.contains(document.activeElement) ? document.activeElement.dataset.key : null;
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
  const args = (e.args || []).map((a) => a);
  if (e.key === 'output' || e.key === 'input') {
    const d = [...devices.sinks, ...devices.sources].find((x) => x.name === args[0]);
    args[0] = !args[0] ? t('System-Standard') : d ? d.description : args[0];
  }
  // e.text: a core from before keyed events
  return e.text ?? t(EVENTS[e.key] || e.key || '', ...args);
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
async function tick() {
  if (document.hidden) return;
  let s;
  try {
    s = await invoke('get_state', { micTest: tab === 'mic' });
  } catch {
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
  micLevel = lv.mic >= 0 ? decay(micLevel, frac(lv.mic)) : 0;
  micInLevel = lv.mic_in >= 0 ? decay(micInLevel, frac(lv.mic_in)) : 0;
  setMicMeter(micLevel);
  setMicInMeter(micInLevel);
  $('mic-state').textContent = !s.mic_active ? t('Mikrofon aus') : cfg.mic.mute ? t('stumm') : t('aktiv');
  $('mic-state').disabled = !cfg.mic.mute;
  paintEvents(s.events || []);
  apps = s.apps || [];
  paintApps();
}

async function pollCore() {
  const on = await invoke('core_status').catch(() => false);
  $('core-status').textContent = on ? t('Audio-Kern läuft') : t('Audio-Kern gestoppt');
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
    version: '0.0.2',
    title: 'Push-to-Mute und Presets',
    items: [
      {
        art: 'mute',
        tab: 'mic',
        title: 'Push-to-Mute',
        text: 'Eine Taste schaltet dein Mikrofon stumm und wieder an, auch mitten im Spiel. Einschalten im Mikrofon-Tab, die Taste wählst du im Dialog deines Desktops.',
      },
      {
        art: 'eq',
        tab: 'sound',
        title: 'Eigene EQ-Presets',
        text: 'Stell den Equalizer nach deinem Geschmack ein und speichere die Kurve unter eigenem Namen. Bis zu vier eigene Presets, ein Klick holt sie zurück.',
      },
      {
        art: 'mixer',
        tab: 'mixer',
        title: 'Neuer Look',
        text: 'Der Equalizer zeigt seine Kurve als LED-Anzeige, und der Mixer nutzt das ganze Fenster. Zieh es größer, die Fader wachsen mit.',
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

function headLeds(parent, n = 4, rows = 7) {
  const cols = ledColumns(parent, n, rows);
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

function eqArt(box) {
  const card = mini(box, 0.5, 600);
  const name = el('span', 'muted');
  const title = el('span', 'title', 'Equalizer ');
  title.append(name);
  const chips = el('div', 'chips');
  const shapes = [['Flat', EQ_PRESETS.Flat], ['Bass', EQ_PRESETS.Bass], ['Stimme', EQ_PRESETS.Stimme], ['Mein Kopfhörer', [6, 5, 3, 0, -2, -2, 1, 4, 3, 1]]];
  const btns = shapes.map(([n]) => chips.appendChild(el('button', null, t(n))));
  const head = el('div', 'card-head wrap');
  head.append(title, chips);
  const grid = el('div', 'eqgrid');
  card.append(head, grid);
  const paint = eqLeds(grid);
  const cur = Array(10).fill(0);
  let n = -1;
  let tick = 0;
  return () => {
    if (tick++ % 26 === 0) {
      n = (n + 1) % shapes.length;
      name.textContent = t(shapes[n][0]);
      btns.forEach((b, i) => b.setAttribute('aria-pressed', String(i === n)));
    }
    // 1 dB per step, like dragging the band sliders
    shapes[n][1].forEach((v, i) => (cur[i] += Math.sign(v - cur[i])));
    paint(cur);
  };
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
    strip('media', 'Media', 78, '-7.8 dB', 'Flat', 'Spotify'),
    strip('aux', 'Aux', 60, '-13.3 dB', 'Flat', ''),
    el('div', 'sep'),
    strip('master', 'Master', 68, '68 %'),
  );
  return () => steps.forEach((s) => s());
}

const NEWS_ART = { eq: eqArt, mute: muteArt, mixer: mixerArt };
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
    if (NEWS_ART[it.art]) steps.push(NEWS_ART[it.art](art));
    const text = el('div', 'news-text');
    const go = el('button', 'btn small', t('Ansehen'));
    go.addEventListener('click', () => {
      closeNews();
      document.querySelector(`.tab[data-tab="${it.tab}"]`)?.click();
    });
    text.append(el('h2', null, t(it.title)), el('p', 'hint', t(it.text)), go);
    row.append(art, text);
    list.append(row);
  }
  const step = () => steps.forEach((s) => s());
  step();
  clearInterval(newsTimer);
  if (!matchMedia('(prefers-reduced-motion: reduce)').matches) newsTimer = setInterval(step, 70);
  $('news').hidden = false;
  fitMinis();
  $('news').firstElementChild.focus();
}

function closeNews() {
  clearInterval(newsTimer);
  $('news').hidden = true;
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
  const info = $('support-info');
  $('support').addEventListener('click', () => { info.hidden = false; info.firstElementChild.focus(); });
  $('support-cancel').addEventListener('click', () => { info.hidden = true; });
  info.addEventListener('click', (e) => { if (e.target === info) info.hidden = true; });
  info.addEventListener('keydown', (e) => { if (e.key === 'Escape') info.hidden = true; });
  $('support-go').addEventListener('click', () => {
    info.hidden = true;
    invoke('open_support').catch(showError);
  });
  $('bug').addEventListener('click', () => invoke('open_bug_report').catch(showError));
  const about = $('about');
  const closeAbout = () => (about.hidden = true);
  $('about-btn').addEventListener('click', async () => {
    const a = await invoke('app_info').catch(() => null);
    if (a) {
      $('about-version').textContent = t('Version {0}, {1}', a.version, a.package);
      $('about-contact').textContent = t('Kontakt: {0}', a.email);
    }
    $('licenses-box').open = false;
    about.hidden = false;
    about.firstElementChild.focus();
  });
  $('about-close').addEventListener('click', closeAbout);
  about.addEventListener('click', (e) => { if (e.target === about) closeAbout(); });
  about.addEventListener('keydown', (e) => { if (e.key === 'Escape') closeAbout(); });
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
  news.addEventListener('click', (e) => { if (e.target === news) closeNews(); });
  news.addEventListener('keydown', (e) => { if (e.key === 'Escape') closeNews(); });
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
  // the internal master stage from earlier builds stays at unity; volume is the device's job now
  if (cfg.master !== 100) {
    cfg.master = 100;
    save();
  }
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
      document.querySelectorAll('.tab').forEach((t) => t.setAttribute('aria-pressed', String(t === b)));
      document.querySelectorAll('.page').forEach((p) => (p.hidden = p.id !== `tab-${tab}`));
    });
  }
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
  $('setup').hidden = false;
  await refreshDevices();
  await runChecks();
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
    $('setup').hidden = true;
    await init();
    $('autostart').setAttribute('aria-checked', String(await invoke('get_autostart')));
  });
}

async function main() {
  translatePage();
  bindChrome();
  pollCore();
  setInterval(pollCore, 2000);
  await invoke('ensure_core').catch(() => {});
  // the core writes the config on its first start
  cfg = await loadConfig(14);
  if (cfg && cfg.setup_done) {
    await init();
    newsAfterUpdate();
  } else await openSetup();
}

main().catch(showError);
