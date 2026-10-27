const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../../ui/launcher.js'), 'utf8');
const slice = (start, end) => source.slice(source.indexOf(start), source.indexOf(end, source.indexOf(start)));

(async () => {
  const update = (phase, downloaded = 0) => ({required: phase !== 'ready', phase, downloaded, download_total: 100, staged: 50, stage_total: 100});
  const game = {launcher: 'steam', app_id: '730', is_installed: false, update: update('required')};
  const row = {...game};
  const status = {innerHTML: ''}, launch = {}, badge = {style: {}};
  const modal = {open: true, dataset: {game: 'steam:730'}, scrollTop: 250,
    querySelector: selector => ({'[data-game-update]': status, '.game-modal-launch-btn': launch, '[data-game-install-status]': badge})[selector]};
  let next = update('downloading', 25), installed = false, renders = 0;
  const context = vm.createContext({
    demo: false, busy: false, document: {hidden: false}, page: 'library', games: [game], steamRows: [row],
    isModPlatform: () => false, modCapability: () => false,
    gogStatus: {client_installed: true, launch_supported: true},
    ubisoftStatus: {client_installed: true},
    riotStatus: {client_installed: true, capabilities: {launch: true}},
    $: () => modal, gameKey: g => `${g.launcher}:${g.app_id}`, icon: () => '', esc: s => s,
    format: new Intl.NumberFormat('ru-RU'), hydrateIcons() {},
    renderLibrary() { renders++; },
    api: async () => ({games: {'730': {update: next, is_installed: installed}}}),
  });
  vm.runInContext(slice('const canRequestLaunch =', 'async function launchRiot') + '\nglobalThis.canRequestLaunch = canRequestLaunch;', context);
  vm.runInContext(slice('function gameState(g)', 'let launchWarningResolver'), context);
  vm.runInContext(slice('let downloadPolling=false;', " document.addEventListener('click'"), context);
  assert.equal(context.canRequestLaunch({launcher: 'steam', app_id: '620', is_installed: false, update: null}), true);
  assert.equal(context.playLabel({launcher: 'steam', app_id: '620', is_installed: false, update: null}), 'Установить');
  assert.equal(context.canRequestLaunch({launcher: 'epic', app_id: 'Fortnite', is_installed: false}), true);
  assert.equal(context.playLabel({launcher: 'epic', app_id: 'Fortnite', is_installed: false}), 'Установить');
  assert.equal(context.canRequestLaunch({launcher: 'gog', app_id: '101', is_installed: false}), true);
  assert.equal(context.playLabel({launcher: 'gog', app_id: '101', is_installed: false}), 'Установить');
  assert.equal(context.canRequestLaunch({launcher: 'ubisoft', app_id: '42', is_installed: false}), true);
  assert.equal(context.playLabel({launcher: 'ubisoft', app_id: '42', is_installed: false}), 'Установить');
  await context.pollDownloads();
  assert.match(status.innerHTML, /value="25"/);
  assert.equal(row.update.downloaded, 25, 'All account rows receive progress');
  next = update('downloading', 75);
  await context.pollDownloads();
  assert.match(status.innerHTML, /value="75"/);
  next = update('paused', 75);
  await context.pollDownloads();
  assert.match(status.innerHTML, /Обновление приостановлено/);
  next = update('installing', 100);
  await context.pollDownloads();
  assert.match(status.innerHTML, /value="50"/);
  assert.match(status.innerHTML, /на диске/);
  next = update('ready', 100); installed = true;
  await context.pollDownloads();
  assert.doesNotMatch(status.innerHTML, /<progress/);
  assert.match(status.innerHTML, /игра готова/);
  assert.equal(launch.innerHTML, 'Играть');
  assert.equal(badge.textContent, '● Установлена');
  assert.equal(modal.scrollTop, 250);
  assert.equal(modal.open, true);
  assert.equal(renders, 5);
  // A modal opened after the model changed must also sync on an unchanged poll.
  status.innerHTML = 'stale';
  await context.pollDownloads();
  assert.match(status.innerHTML, /игра готова/);
  modal.open = false; status.innerHTML = 'closed';
  next = update('downloading', 10);
  await context.pollDownloads();
  assert.equal(status.innerHTML, 'closed', 'Polling must not reopen closed dialogs');
  console.log('PASS: Steam modal progress, pause, staging, completion and closed-dialog regression');
})().catch(error => { console.error(error); process.exitCode = 1; });
