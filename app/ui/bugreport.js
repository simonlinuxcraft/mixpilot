const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
const win = window.__TAURI__.window?.getCurrentWindow?.();
let info = { can_send: false, email: '', info: '', log: '' };

const flag = (id) => $(id).getAttribute('aria-checked') === 'true';
const report = () => ({
  description: $('desc').value,
  errors: $('errors').value,
  wish: $('wish').value,
  email: $('email').value,
  include_info: flag('info'),
  include_log: flag('log'),
  info: info.info,
  log: info.log,
});

function show(view) {
  $('form').hidden = view !== 'form';
  $('done').hidden = view !== 'done';
  $('fail').hidden = view !== 'fail';
  $('footer').hidden = view !== 'form';
  // the clicked button is gone now, the heading takes the focus so the result is read out
  $(view).querySelector('h2')?.focus();
}

function paintPreview() {
  const parts = [];
  if (flag('info')) parts.push('--- System ---\n' + info.info);
  if (flag('log')) parts.push(t('--- Protokoll ---') + '\n' + (info.log || t('(leer)')));
  $('preview').textContent = parts.join('\n\n') || t('Nur deine Texte.');
}

function sync() {
  $('send').disabled = $('desc').value.trim().length < 5 && $('wish').value.trim().length < 5;
}

for (const id of ['info', 'log']) {
  $(id).addEventListener('click', () => {
    $(id).setAttribute('aria-checked', String(!flag(id)));
    paintPreview();
  });
}
$('add-errors').addEventListener('click', () => {
  $('err-field').hidden = false;
  $('add-errors').hidden = true;
  $('errors').focus();
});
// the error banner in the main window hands over its message
function prefillErrors(text) {
  if (!text || $('errors').value.includes(text)) return;
  $('errors').value = [$('errors').value.trim(), text].filter(Boolean).join('\n');
  $('err-field').hidden = false;
  $('add-errors').hidden = true;
  // a report already sent or failed: back to the form, the new error is the next report
  if ($('form').hidden) show('form');
}
$('desc').addEventListener('input', sync);
$('wish').addEventListener('input', sync);
$('win-close').addEventListener('click', () => win?.close());
$('cancel').addEventListener('click', () => win?.close());
$('close-done').addEventListener('click', () => win?.close());
if (win) {
  const square = async () => document.documentElement.classList.toggle('square', (await win.isMaximized()) || (await win.isFullscreen()));
  square();
  win.onResized(square);
}
$('back').addEventListener('click', () => { show('form'); $('desc').focus(); });
$('mail').addEventListener('click', () => invoke('mail_bug_report', { report: report() }));

$('send').addEventListener('click', async () => {
  // without an endpoint built in, the mail program is the way
  if (!info.can_send) {
    invoke('mail_bug_report', { report: report() });
    return;
  }
  $('send').disabled = true;
  $('send').textContent = t('Wird gesendet …');
  try {
    await invoke('send_bug_report', { report: report() });
    show('done');
  } catch (e) {
    $('fail-text').textContent = t('Grund: {0}. Du kannst den Bericht stattdessen per E-Mail schicken, er ist schon ausgefüllt.', t(String(e)).replace(/\.$/, ''));
    $('mail-addr').textContent = info.email;
    show('fail');
  } finally {
    $('send').textContent = info.can_send ? t('Senden') : t('Per E-Mail senden');
    sync();
  }
});

(async () => {
  translatePage();
  prefillErrors(window.BUG_ERRORS);
  info = await invoke('bug_info').catch(() => info);
  $('send').textContent = info.can_send ? t('Senden') : t('Per E-Mail senden');
  $('footer-note').textContent = info.can_send ? '' : t('Öffnet dein Mailprogramm, Empfänger {0}', info.email);
  $('privacy').hidden = !info.can_send;
  paintPreview();
  sync();
  show('form');
  $('desc').focus();
})();
