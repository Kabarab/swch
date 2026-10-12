const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

(async () => {
  const data = {};
  const writes = [];
  const opened = [];
  const closedTabs = [];
  let onMessage, onRemoved, nextTab = 10;
  const chrome = {
    storage: {session: {
      get: async key => key === null ? {...data} : {[key]: data[key]},
      set: async values => Object.assign(data, values),
      remove: async key => { delete data[key]; },
    }},
    runtime: {onMessage: {addListener: fn => { onMessage = fn; }}},
    windows: {
      create: async options => { opened.push(options); return {id: 5, tabs: [{id: nextTab++}]}; },
      remove: async () => { throw new Error('Never close a whole window after authentication'); },
    },
    tabs: {
      update: async (id, options) => { opened.push({id, ...options}); },
      remove: async id => { closedTabs.push(id); },
      onRemoved: {addListener: fn => { onRemoved = fn; }},
    },
  };
  const authUrl = 'https://www.epicgames.com/id/api/redirect?clientId=34a02cf8f4414e29b15921876da36f9a&responseType=code';
  const context = vm.createContext({chrome, URL, URLSearchParams, Date, AbortSignal,
    fetch: async (url, options) => {
      writes.push({url, body: JSON.parse(options.body)});
      return {ok: true, json: async () => url.endsWith('/prepare')
        ? {auth_url: authUrl, expires_in: 600} : {status: 'connected'}};
    },
  });
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../../browser-extension/epic/background.js'), 'utf8'), context);
  const send = (message, sender) => new Promise(resolve => onMessage(message, sender, resolve));
  const id = 'a'.repeat(64);
  const source = {url: `http://127.0.0.1:4242/epic/connect#session=${id}`, tab: {id: 1}, frameId: 0};
  assert.equal((await send({type: 'swch-epic-begin', session_id: id}, source)).status, 'opened');
  assert.equal(opened[0].type, 'normal', 'Use a normal browser window with the existing profile');
  assert.equal(opened[1].url, authUrl);
  await send({type: 'swch-epic-begin', session_id: id}, source);
  assert.equal(opened.length, 2, 'Repeated bridge notifications must not open extra windows');
  const code = '0123456789abcdef0123456789abcdef';
  assert.equal((await send({type: 'swch-epic-complete', code}, {url: authUrl, tab: {id: 999}, frameId: 0})).status, 'ignored');
  assert.equal(writes.filter(w => w.url.endsWith('/complete')).length, 0, 'Unrelated Epic tabs must never submit codes');
  assert.equal((await send({type: 'swch-epic-complete', code}, {url: authUrl.replace('www.epicgames.com', 'evil.test'), tab: {id: 10}, frameId: 0})).status, 'error');
  const sender = {url: authUrl, tab: {id: 10}, frameId: 0};
  const results = await Promise.all([send({type: 'swch-epic-complete', code}, sender), send({type: 'swch-epic-complete', code}, sender)]);
  assert.equal(results[0].status, 'connected');
  assert.equal(writes.filter(w => w.url.endsWith('/complete')).length, 1, 'Redeem each code only once');
  assert.deepEqual(closedTabs, [10], 'Other browser tabs and windows must survive');
  assert.equal(Object.keys(data).length, 0, 'Remove session capabilities after completion');
  assert.equal((await send({type: 'swch-epic-begin', session_id: id}, {...source, url: source.url.replace('127.0.0.1', 'example.com')})).status, 'error');
  const secondId = 'b'.repeat(64);
  await send({type: 'swch-epic-begin', session_id: secondId}, {...source, url: source.url.replace(id, secondId)});
  onRemoved(11);
  // Wait for the queued removal using another event behind it.
  await send({type: 'swch-epic-complete', code}, {url: authUrl, tab: {id: 999}, frameId: 0});
  assert.equal(writes.at(-1).url.endsWith('/cancel'), true);
  assert.equal(Object.keys(data).length, 0);
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, '../../browser-extension/epic/manifest.json'), 'utf8'));
  assert.deepEqual(manifest.permissions, ['storage']);
  assert(!JSON.stringify(manifest).includes('<all_urls>'));
  console.log('Epic browser extension: normal profile window, bound tabs, validation, duplicate code suppression and cancellation passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
