const {launchOptions,emptyApi}=require('./harness.cjs');
const assert = require('node:assert/strict');
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');

(async () => {
  const browser = await chromium.launch(launchOptions);
  try {
    const page = await browser.newPage({viewport: {width: 1280, height: 960}});
    const errors = [];
    const writes = [];
    page.on('pageerror', error => errors.push(error.message));
    const id = 'a'.repeat(64);
    let state = 'pending', library = 'syncing', legacy = true;
    await page.route('https://**', route => route.abort());
    await page.route('**/api/**', async route => {
      const request = route.request();
      const url = new URL(request.url());
      const path = url.pathname;
      if (path === '/api/i18n/selection') return route.fulfill({json:emptyApi(path)});
      if (request.method() === 'POST') writes.push({path, body: request.postDataJSON()});
      if (path.startsWith('/api/epic/auth/browser/')) {
        if (legacy) return route.fulfill({status: 405, body: ''});
        if (path.endsWith('/start')) return route.fulfill({json: {session_id: id, browser_url: `${url.origin}/epic/connect#session=${id}`, expires_in: 600}});
        if (path.endsWith('/open')) return route.fulfill({json: {status: 'opened'}});
        if (path.endsWith('/cancel')) { state = 'cancelled'; return route.fulfill({json: {status: state}}); }
        return route.fulfill({json: {status: state, library_status: library, account: {account_id: 'epic-test', display_name: 'Epic Test'}}});
      }
      let json = [];
      if (path === '/api/epic/auth/url') json = {url: 'https://www.epicgames.com/id/api/redirect'};
      if (path === '/api/epic/auth/login') json = {account: {account_id: 'manual', display_name: 'Manual'}};
      if (path === '/api/steam/library') json = {accounts: [], games: [], warnings: []};
      if (path === '/api/epic/accounts') json = {accounts: state === 'connected' ? [{account_id: 'epic-test', display_name: 'Epic Test'}] : []};
      if (path === '/api/modules') json = {modules: []};
      return route.fulfill({json});
    });
    await page.goto(process.env.SWCH_UI_URL || 'http://127.0.0.1:4242/');
    const openDialog = async () => {
      await page.locator('.connect-link').click();
      await page.locator('[data-tab="epic"]').click();
    };
    await openDialog();
    await page.locator('#open-epic-browser-btn').click();
    await page.locator('#login-error').filter({hasText: 'Обнови сборку и перезапусти SWCH'}).waitFor();
    legacy = false;
    await page.locator('#open-epic-browser-btn').click();
    await page.locator('#epic-browser-progress').waitFor();
    assert.equal(writes.at(-1).body.session_id, id);
    state = 'authenticating';
    await page.locator('#epic-browser-status').filter({hasText: 'Подключаем аккаунт'}).waitFor();
    assert(await page.locator('#cancel-epic-browser-btn').isDisabled());
    state = 'connected';
    await page.locator('#connect-dialog').waitFor({state: 'hidden'});
    library = 'ready';
    await page.locator('#toast').filter({hasText: 'Библиотека Epic Games загружена'}).waitFor();
    assert.equal(writes.filter(w => w.path === '/api/epic/library/sync').length, 0);
    await openDialog();
    await page.locator('#epic-manual-login summary').click();
    await page.locator('#oauth-code-input').fill('0123456789abcdef0123456789abcdef');
    await page.locator('#login-form button[type="submit"]').click();
    await page.locator('#connect-dialog').waitFor({state: 'hidden'});
    assert.equal(writes.filter(w => w.path === '/api/epic/auth/login').length, 1);
    assert.equal(writes.filter(w => w.path === '/api/epic/library/sync').length, 0, 'Manual login must not repeat server sync');
    state = 'pending';
    await openDialog();
    await page.locator('#open-epic-browser-btn').click();
    await page.locator('#epic-browser-progress').waitFor();
    await page.locator('#cancel-epic-browser-btn').click();
    await page.locator('#epic-browser-progress').waitFor({state: 'hidden'});
    assert.equal(writes.at(-1).path, '/api/epic/auth/browser/cancel');
    assert.deepEqual(errors, []);
    console.log('Epic UI: old-server 405, automatic login, background library, manual fallback and cancellation passed');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
