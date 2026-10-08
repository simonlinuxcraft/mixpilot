const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
const win = window.__TAURI__.window?.getCurrentWindow?.();
let info = { can_send: false, email: '', info: '', log: '' };

const flag = (id) => $(id).getAttribute('aria-checked') === 'true';
const report = () => ({
  description: $('desc').value,
  errors: $('errors').value,
  email: $('email').value,
  include_info: flag('info'),
  include_log: flag('log'),
});

function show(view) {
  $('form').hidden = view !== 'form';
  $('done').hidden = view !== 'done';
  $('fail').hidden = view !== 'fail';
  $('footer').hidden = view !== 'form';
}

function paintPreview() {
  const parts = [];
  if (flag('info')) parts.push('--- System ---\n' + info.info);
  if (flag('log')) parts.push(t('--- Protokoll ---') + '\n' + (info.log || t('(leer)')));
  $('preview').textContent = parts.join('\n\n') || t('Nur deine Beschreibung.');
}

function sync() {
  $('send').disabled = $('desc').value.trim().length < 5;
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
$('desc').addEventListener('input', sync);
$('win-close').addEventListener('click', () => win?.close());
$('cancel').addEventListener('click', () => win?.close());
$('close-done').addEventListener('click', () => win?.close());
$('back').addEventListener('click', () => show('form'));
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
    $('fail-text').textContent = t('Grund: {0}. Du kannst den Bericht stattdessen per E-Mail schicken, er ist schon ausgefüllt.', t(String(e)));
    $('mail-addr').textContent = info.email;
    show('fail');
  } finally {
    $('send').textContent = info.can_send ? t('Senden') : t('Per E-Mail senden');
    sync();
  }
});

(async () => {
  translatePage();
  info = await invoke('bug_info').catch(() => info);
  $('send').textContent = info.can_send ? t('Senden') : t('Per E-Mail senden');
  $('footer-note').textContent = info.can_send ? '' : t('Öffnet dein Mailprogramm, Empfänger {0}', info.email);
  $('privacy').hidden = !info.can_send;
  paintPreview();
  sync();
  show('form');
  $('desc').focus();
})();
