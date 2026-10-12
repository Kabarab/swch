/* Session capabilities are kept in memory, never in browser sync or on disk. */
'use strict';
const ext = globalThis.browser || chrome;
const AUTH_CLIENT = '34a02cf8f4414e29b15921876da36f9a';
const sessionKey = tabId => `swch-epic:${tabId}`;

function bridgeOrigin(raw) {
  const url = new URL(raw);
  if (url.protocol !== 'http:' || !['127.0.0.1', 'localhost'].includes(url.hostname)
      || url.pathname !== '/epic/connect' || url.username || url.password) {
    throw new Error('Начни вход из локального SWCH.');
  }
  return url.origin;
}

function isEpicResult(raw) {
  try {
    const url = new URL(raw);
    return url.protocol === 'https:' && ['www.epicgames.com', 'epicgames.com'].includes(url.hostname)
      && url.pathname === '/id/api/redirect' && url.searchParams.get('clientId') === AUTH_CLIENT;
  } catch { return false; }
}

async function localApi(origin, path, body) {
  const response = await fetch(`${origin}/api/epic/auth/browser/${path}`, {
    method: 'POST', headers: {'Content-Type': 'application/json'},
    body: JSON.stringify(body), cache: 'no-store', signal: AbortSignal.timeout(45000),
  });
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || 'Не удалось связаться со SWCH.');
  return result;
}

async function begin(message, sender) {
  if (!sender.tab || sender.frameId !== 0) throw new Error('Открой страницу входа SWCH.');
  const origin = bridgeOrigin(sender.url);
  const id = new URL(sender.url).hash.slice(1);
  const expected = new URLSearchParams(id).get('session');
  if (message.session_id !== expected || !/^[a-f0-9]{64}$/.test(expected || '')) {
    throw new Error('Сессия входа не найдена. Начни вход заново в SWCH.');
  }
  const active = await ext.storage.session.get(null);
  for (const [key, flow] of Object.entries(active)) {
    if (!key.startsWith('swch-epic:')) continue;
    if (flow.expires <= Date.now()) { await ext.storage.session.remove(key); continue; }
    if (flow.origin === origin && flow.session_id === expected) {
      return {status: 'opened'};
    }
  }
  const prepared = await localApi(origin, 'prepare', {session_id: expected});
  if (!isEpicResult(prepared.auth_url)) throw new Error('SWCH вернул неверный адрес входа Epic.');
  // Create an ordinary browser window in this profile. Bind its tab before
  // navigating, so an already signed-in Epic session cannot outrun the binding.
  const window = await ext.windows.create({url: 'about:blank', type: 'normal', focused: true});
  const tabId = window.tabs?.[0]?.id;
  if (tabId === undefined) throw new Error('Не удалось открыть окно браузера.');
  await ext.storage.session.set({[sessionKey(tabId)]: {
    origin, session_id: expected, window_id: window.id,
    expires: Date.now() + Math.min(prepared.expires_in, 600) * 1000, submitting: false,
  }});
  try { await ext.tabs.update(tabId, {url: prepared.auth_url}); }
  catch (error) {
    await ext.storage.session.remove(sessionKey(tabId));
    await ext.windows.remove(window.id).catch(() => {});
    throw error;
  }
  return {status: 'opened'};
}

async function complete(message, sender) {
  if (!sender.tab || sender.frameId !== 0 || !isEpicResult(sender.url)
      || !/^[a-f0-9]{32}$/i.test(message.code || '')) {
    throw new Error('Неверный результат входа Epic.');
  }
  const key = sessionKey(sender.tab.id);
  const flow = (await ext.storage.session.get(key))[key];
  // Ignore any Epic tabs which were not opened for this explicit SWCH flow.
  if (!flow) return {status: 'ignored'};
  if (flow.expires <= Date.now()) {
    await ext.storage.session.remove(key);
    return {status: 'expired'};
  }
  if (flow.submitting) return {status: 'authenticating'};
  flow.submitting = true;
  await ext.storage.session.set({[key]: flow});
  try {
    const result = await localApi(flow.origin, 'complete', {session_id: flow.session_id, code: message.code});
    await ext.storage.session.remove(key);
    if (result.status === 'connected') {
      // Close only our sign-in tab; any other tabs added to the window survive.
      await ext.tabs.remove(sender.tab.id).catch(() => {});
    }
    return {status: result.status};
  } catch (error) {
    // Do not redeem a one-use code again after a lost HTTP response.
    await ext.storage.session.remove(key);
    throw error;
  }
}

// Serialise events: duplicate DOM notifications must not submit a code twice.
let queue = Promise.resolve();
ext.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!['swch-epic-begin', 'swch-epic-complete'].includes(message?.type)) return false;
  queue = queue.then(() => message.type === 'swch-epic-begin' ? begin(message, sender) : complete(message, sender))
    .then(sendResponse, () => sendResponse({status: 'error', error: 'Не удалось завершить вход. Проверь SWCH и начни вход заново.'}));
  return true;
});

ext.tabs.onRemoved.addListener(tabId => {
  queue = queue.then(async () => {
    const key = sessionKey(tabId);
    const flow = (await ext.storage.session.get(key))[key];
    if (!flow) return;
    await ext.storage.session.remove(key);
    if (!flow.submitting) await localApi(flow.origin, 'cancel', {session_id: flow.session_id}).catch(() => {});
  }).catch(() => {});
});
