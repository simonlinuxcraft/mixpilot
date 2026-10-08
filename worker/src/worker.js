// Bug report proxy: keeps the Web3Forms key on the server. POST /submit only.
// Secrets: WEB3FORMS_KEY, optional APP_SHARED_SECRET. Optional KV binding RATE_LIMIT.

const MAX_SUBJECT = 300;
const MAX_MESSAGE = 20000;
const MAX_EMAIL = 200;
const RATE_LIMIT_PER_HOUR = 5;
const RATE_WINDOW_SECONDS = 3600;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname !== '/submit') return new Response('Not Found', { status: 404 });
    if (request.method === 'OPTIONS') return corsPreflight();
    if (request.method !== 'POST') return new Response('Method Not Allowed', { status: 405 });

    if (env.APP_SHARED_SECRET) {
      const provided = request.headers.get('X-App-Secret') || '';
      if (!constantTimeEqual(provided, env.APP_SHARED_SECRET)) {
        return jsonResponse({ success: false, message: 'forbidden' }, 403);
      }
    }

    const ip = request.headers.get('CF-Connecting-IP') || 'unknown';
    if (env.RATE_LIMIT) {
      const blocked = await checkRateLimit(env.RATE_LIMIT, ip);
      if (blocked) return jsonResponse({ success: false, message: 'rate-limited' }, 429);
    }

    let payload;
    try { payload = await request.json(); }
    catch { return jsonResponse({ success: false, message: 'invalid json' }, 400); }
    if (!payload || typeof payload !== 'object') {
      return jsonResponse({ success: false, message: 'invalid payload' }, 400);
    }

    const subject = sanitize(payload.subject, MAX_SUBJECT);
    const message = sanitize(payload.message, MAX_MESSAGE);
    if (!subject || !message) {
      return jsonResponse({ success: false, message: 'subject + message required' }, 400);
    }
    const email = payload.email ? sanitize(payload.email, MAX_EMAIL) : '';

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
};

function sanitize(v, max) {
  const s = String(v == null ? '' : v);
  let out = '';
  for (let i = 0; i < s.length && out.length < max; i++) {
    const c = s.charCodeAt(i);
    if (c >= 32 && c !== 127) out += s[i];
  }
  return out.trim();
}

function jsonResponse(body, status) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      'Content-Type': 'application/json',
      'Cache-Control': 'no-store',
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'POST, OPTIONS',
      'Access-Control-Allow-Headers': 'Content-Type, X-App-Secret'
    }
  });
}

function corsPreflight() {
  return new Response(null, {
    status: 204,
    headers: {
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'POST, OPTIONS',
      'Access-Control-Allow-Headers': 'Content-Type, X-App-Secret',
      'Access-Control-Max-Age': '86400'
    }
  });
}

function constantTimeEqual(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string' || a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

async function checkRateLimit(kv, ip) {
  const key = 'rl:' + ip;
  const raw = await kv.get(key);
  const now = Math.floor(Date.now() / 1000);
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
}
