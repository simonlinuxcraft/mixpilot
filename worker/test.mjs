// node worker/test.mjs: request filtering, sanitizing and the rate limit, with Web3Forms and KV faked
import assert from 'node:assert/strict';
import w from './src/worker.js';
let sent = [];
globalThis.fetch = async (url, opts) => { sent.push(JSON.parse(opts.body)); return new Response(JSON.stringify({ success: true }), { status: 200 }); };
const kvStore = new Map();
const kv = { get: async (k) => kvStore.get(k) ?? null, put: async (k, v) => { kvStore.set(k, v); } };
const env = { WEB3FORMS_KEY: 'k', RATE_LIMIT: kv };
const req = (body, headers = { 'Content-Type': 'application/json', 'CF-Connecting-IP': '1.2.3.4' }, method = 'POST', path = '/submit') =>
  new Request('https://x.dev' + path, { method, headers, body: method === 'POST' ? body : undefined });
const call = async (...a) => { const r = await w.fetch(req(...a), env); return [r.status, await r.text(), r.headers]; };

// line breaks and tabs survive in the message, not in subject/email
let [st] = await call(JSON.stringify({ subject: 'S\nX', message: 'a\nb\tc\r\u0000d', email: 'e@x\n.de' }));
assert.equal(st, 200);
assert.equal(sent[0].message, 'a\nb\tcd');
assert.equal(sent[0].subject, 'SX');
assert.equal(sent[0].email, 'e@x.de');
// text/plain (what a web page can post without preflight) is refused before parsing and before the rate limit
[st] = await call(JSON.stringify({ subject: 's', message: 'm' }), { 'Content-Type': 'text/plain', 'CF-Connecting-IP': '9.9.9.9' });
assert.equal(st, 415);
assert.equal(kvStore.has('rl:9.9.9.9'), false);
// no CORS: preflight gets 405 without Allow-Origin
let [s2, , h] = await call(null, { 'Origin': 'https://evil.example' }, 'OPTIONS');
assert.equal(s2, 405); assert.equal(h.get('Access-Control-Allow-Origin'), null);
// invalid payloads do not count against the limit
for (let i = 0; i < 10; i++) await call('{"subject":"","message":""}', { 'Content-Type': 'application/json', 'CF-Connecting-IP': '5.5.5.5' });
assert.equal(kvStore.has('rl:5.5.5.5'), false);
// 5 per hour, the 6th is refused
for (let i = 0; i < 5; i++) assert.equal((await call('{"subject":"s","message":"m"}', { 'Content-Type': 'application/json', 'CF-Connecting-IP': '7.7.7.7' }))[0], 200);
assert.equal((await call('{"subject":"s","message":"m"}', { 'Content-Type': 'application/json', 'CF-Connecting-IP': '7.7.7.7' }))[0], 429);
// KV outage: report still goes through
const broken = { get: async () => { throw new Error('kv down'); }, put: async () => {} };
const r = await w.fetch(req('{"subject":"s","message":"m"}'), { WEB3FORMS_KEY: 'k', RATE_LIMIT: broken });
assert.equal(r.status, 200);
// an unexpected throw still answers JSON with a message
const r2 = await w.fetch({ url: 'not a url' }, env);
assert.equal(r2.status, 500); assert.equal((await r2.json()).message, 'server error');
console.log('worker tests ok');
