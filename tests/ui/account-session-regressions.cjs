const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../../ui/launcher.js'), 'utf8');
const slice = (start, end) => source.slice(source.indexOf(start), source.indexOf(end, source.indexOf(start)));

(async () => {
  const {isModPlatform}=await import('../../ui/mods.js');
  const saved = { id: 'riot_test', username: 'same-name', display_name: 'Test#123', launcher: 'riot' };
  const savedRequests=[];
  const elements = new Map();
  const context = vm.createContext({
    isModPlatform,
    demo: false, busy: false, platform: 'epic', page: 'library', ubisoftStatus:null, gogStatus:null,
    accounts: [{ id: 'epic_test', username: 'same-name', launcher: 'epic' }],
    $: id => { if (!elements.has(id)) elements.set(id, { open: false }); return elements.get(id); },
    toast() {}, render() {}, refresh: async () => {},
    riotProfileErrors: new Map(), applyRiotProfiles() {},
    api: async (path,body) => {savedRequests.push([path,body]);return path==='/api/riot/accounts/save'?{account:saved}:{profiles:[]};},
  });
  vm.runInContext(slice('function updateConnectTabBadges()', 'function switchConnectTab('), context);
  vm.runInContext(slice('async function saveRiotSession()', 'async function switchAccount('), context);
  await context.saveRiotSession();
  assert.equal(context.platform, 'riot');
  assert.equal(context.page, 'accounts');
  assert.equal(context.accounts.length, 2, 'Saving Riot must preserve same-named Epic accounts');
  assert.equal(elements.get('riot-tab-badge').textContent, 1);
  assert.equal(elements.get('epic-tab-badge').textContent, 1);
  assert.equal(elements.get('accounts-count').textContent, 2);
  assert.equal(savedRequests[1][0],'/api/riot/accounts/sync');
  assert.equal(savedRequests[1][1].account_id,'riot_test');
  assert.equal(savedRequests[1][1].app_id,'valorant','Saving a new login must collect VALORANT data without starting a game');
  await context.saveRiotSession();
  assert.equal(context.accounts.length, 2, 'Repeated saves must not duplicate the account');
  context.api=async path=>{if(path==='/api/riot/accounts/save')return {account:saved};throw new Error('Profile temporarily unavailable');};
  await context.saveRiotSession();
  assert.equal(context.accounts.length,2,'Profile failure must preserve the successful session save');
  assert.equal(context.riotProfileErrors.get('riot_test:valorant'),'Profile temporarily unavailable');
  const calls = [];
  context.api = async path => { calls.push(path); return { message: 'Started' }; };
  vm.runInContext(slice('async function switchAccount(', 'async function openLauncher('), context);
  await context.switchAccount('riot_test');
  assert.deepEqual(calls, ['/api/riot/switch-account'], 'Switch must launch Riot only once');
  context.accounts.find(a => a.launcher === 'riot').is_active = false;
  context.api = async () => { throw new Error('No saved session'); };
  assert.equal(await context.switchAccount('riot_test'), false);
  assert.equal(context.accounts.find(a => a.launcher === 'riot').is_active, false);
  context.confirm = () => true;
  context.riotProfiles = [{ account_id: 'riot_test' }, { account_id: 'riot_other' }];
  context.selectedAccounts = { 'riot:valorant': 'riot_test', 'steam:730': 'steam_other' };
  context.api = async (path, body) => {
    assert.equal(path, '/api/riot/accounts/remove');
    assert.equal(body.account_id, 'riot_test');
    return { status: 'success' };
  };
  vm.runInContext(slice('async function removeRiot(', 'async function scanRoblox('), context);
  await context.removeRiot('riot_test');
  assert.equal(context.accounts.length, 1);
  assert.equal(context.accounts[0].launcher, 'epic');
  assert.equal(context.riotProfiles.length, 1);
  assert.equal(context.selectedAccounts['riot:valorant'], undefined);
  assert.equal(context.selectedAccounts['steam:730'], 'steam_other');
  context.accounts.push(saved);
  context.api = async () => { throw new Error('Delete failed'); };
  await context.removeRiot('riot_test');
  assert.ok(context.accounts.some(a => a.id === 'riot_test'), 'Failed deletion must preserve the account');
  assert.equal(context.busy, false);
  let requests = 0;
  const transport = vm.createContext({
    AbortSignal,
    fetch: async () => { requests++; throw new TypeError('Failed to fetch'); },
  });
  vm.runInContext(slice('async function api(', 'function demoData()'), transport);
  await assert.rejects(transport.api('/api/riot/accounts/save', {}), /Нет соединения с сервером SWCH/);
  assert.equal(requests, 1, 'A failed save must not be replayed automatically');
  transport.fetch = async () => { const error = new Error('timeout'); error.name = 'TimeoutError'; throw error; };
  await assert.rejects(transport.api('/api/riot/accounts/save', {}), /не ответил вовремя/);
  transport.fetch = async () => ({
    ok: false, status: 400, headers: new Map([['content-type', 'application/json']]),
    json: async () => ({ code: 'no_session', error: 'Включи Stay signed in' }),
  });
  await assert.rejects(transport.api('/api/riot/accounts/save', {}), /Включи Stay signed in/);
  console.log('Account session regressions passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
