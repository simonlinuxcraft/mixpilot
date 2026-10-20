// Bug report proxy for the app's curl: keeps the Web3Forms key on the server. POST /submit only.
// Secret: WEB3FORMS_KEY. Optional KV binding RATE_LIMIT.

const MAX_SUBJECT = 300;
const MAX_MESSAGE = 20000;
const MAX_EMAIL = 200;
const RATE_LIMIT_PER_HOUR = 5;
const RATE_WINDOW_SECONDS = 3600;

export default {
  async fetch(request, env) {
    try {
      return await handle(request, env);
    } catch (e) {
      console.log('worker error', String(e));
      return jsonResponse({ success: false, message: 'server error' }, 500);
    }
  }
};

async function handle(request, env) {
  const url = new URL(request.url);
  if (url.pathname !== '/submit') return new Response('Not Found', { status: 404 });
  if (request.method !== 'POST') return new Response('Method Not Allowed', { status: 405 });
  // browsers send JSON cross-origin only after a CORS preflight, which is never granted here
  if (!(request.headers.get('Content-Type') || '').toLowerCase().startsWith('application/json')) {
    return jsonResponse({ success: false, message: 'json only' }, 415);
  }

  let payload;
  try { payload = await request.json(); }
  catch { return jsonResponse({ success: false, message: 'invalid json' }, 400); }
  if (!payload || typeof payload !== 'object') {
    return jsonResponse({ success: false, message: 'invalid payload' }, 400);
  }

  const subject = sanitize(payload.subject, MAX_SUBJECT);
  const message = sanitize(payload.message, MAX_MESSAGE, true);
  if (!subject || !message) {
    return jsonResponse({ success: false, message: 'subject + message required' }, 400);
  }
  const email = payload.email ? sanitize(payload.email, MAX_EMAIL) : '';

  // counted only for reports that would reach Web3Forms
  const ip = request.headers.get('CF-Connecting-IP') || 'unknown';
  if (env.RATE_LIMIT && await rateLimited(env.RATE_LIMIT, ip)) {
    return jsonResponse({ success: false, message: 'rate-limited' }, 429);
  }

  const forwardBody = {
    access_key: env.WEB3FORMS_KEY,
    subject,
    from_name: 'Mixpilot App',
    message,
    botcheck: ''
  };
  if (email) forwardBody.email = email;

  try {
    const res = await fetch('https://api.web3forms.com/submit', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Accept': 'application/json',
        // without an Origin, Web3Forms treats the call as server-side (paid plan only) and answers 403
        'Origin': 'https://localhost',
        'Referer': 'https://localhost/',
        // Cloudflare's WAF in front of web3forms blocks the default worker UA (error 1106)
        'User-Agent': 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36'
      },
      body: JSON.stringify(forwardBody)
    });
    const rawBody = await res.text();
    let data = {};
    try { data = JSON.parse(rawBody); } catch {}
    const ok = res.ok && data.success === true;
    // the client only needs ok / not ok, the full answer goes to the worker log
    if (!ok) console.log('web3forms upstream', res.status, rawBody.slice(0, 500));
    return jsonResponse({
      success: ok,
      message: ok ? 'sent' : 'upstream rejected'
    }, res.ok ? 200 : 502);
  } catch {
    return jsonResponse({ success: false, message: 'upstream error' }, 502);
  }
}

// Control characters out; the message keeps its line breaks and tabs, so log and system info stay readable.
function sanitize(v, max, multiline = false) {
  const s = String(v == null ? '' : v);
  let out = '';
  for (let i = 0; i < s.length && out.length < max; i++) {
    const c = s.charCodeAt(i);
    if ((c >= 32 && c !== 127) || (multiline && (c === 10 || c === 9))) out += s[i];
  }
  return out.trim();
}

function jsonResponse(body, status) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }
  });
}

async function rateLimited(kv, ip) {
  const key = 'rl:' + ip;
  const now = Math.floor(Date.now() / 1000);
  try {
    const raw = await kv.get(key);
    let count = 0;
    let windowStart = now;
    if (raw) {
      try {
        const obj = JSON.parse(raw);
        if (typeof obj.windowStart === 'number' && now - obj.windowStart < RATE_WINDOW_SECONDS) {
          count = obj.count || 0;
          windowStart = obj.windowStart;
        }
      } catch {}
    }
    if (count >= RATE_LIMIT_PER_HOUR) return true;
    await kv.put(key, JSON.stringify({ count: count + 1, windowStart }), { expirationTtl: RATE_WINDOW_SECONDS });
    return false;
  } catch (e) {
    // a KV outage must not swallow bug reports
    console.log('rate limit unavailable', String(e));
    return false;
  }
}
