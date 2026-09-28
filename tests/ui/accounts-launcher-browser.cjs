const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');

(async () => {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 960 } });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  const writes = [];

  await page.route('**/api/**', async route => {
    const req = route.request();
    const url = new URL(req.url());
    if (req.method() === 'POST') {
      writes.push([url.pathname, req.postDataJSON()]);
      return route.fulfill({
        status: 200,
        json: {
          status: 'success',
          message: 'Лаунчер успешно открыт с выбранным профилем.'
        }
      });
    }
    let json = [];
    if (url.pathname === '/api/games') {
      json = [{ id: '730', app_id: '730', launcher: 'steam', name: 'Counter-Strike 2', is_installed: true }];
    }
    if (url.pathname === '/api/steam/library') {
      json = {
        accounts: [
          { id: 's1', username: 'steam1', display_name: 'Steam 1', launcher: 'steam', is_active: true },
          { id: 's2', username: 'steam2', display_name: 'Steam 2', launcher: 'steam', is_active: false }
        ],
        games: [{ id: '730', app_id: '730', launcher: 'steam', name: 'Counter-Strike 2', is_installed: true, account_id: 's1', access: 'local' }],
        warnings: []
      };
    }
    if (url.pathname === '/api/epic/accounts') {
      json = {
        active_account_id: 'e1',
        accounts: [
          { account_id: 'e1', display_name: 'Epic One', is_active: true, launcher: 'epic' },
          { account_id: 'e2', display_name: 'Epic Two', is_active: false, launcher: 'epic' }
        ]
      };
    }
    if (url.pathname === '/api/epic/library') {
      json = [{ app_name: 'shared', title: 'Epic shared game', is_installed: true }];
    }
    await route.fulfill({ json });
  });

  await page.goto(process.env.SWCH_UI_URL || 'http://127.0.0.1:4243/');
  await page.locator('.game-card').first().waitFor();

  // Navigate to Accounts tab
  await page.locator('[data-page="accounts"]').click();
  await page.locator('.account-tile').first().waitFor();

  // Verify accounts are rendered and have "Открыть в лаунчере" buttons
  const tiles = page.locator('.account-tile');
  const count = await tiles.count();
  assert.ok(count >= 2, `Expected at least 2 account tiles, got ${count}`);

  const openButtons = page.locator('button[data-action="open-launcher"]');
  const btnCount = await openButtons.count();
  assert.equal(btnCount, count, 'Every account tile must have an "Открыть в лаунчере" button');

  // Verify button text
  for (let i = 0; i < btnCount; i++) {
    const text = await openButtons.nth(i).innerText();
    assert.equal(text.trim(), 'Открыть в лаунчере');
  }

  // Click "Открыть в лаунчере" on steam account s2
  await page.locator('button[data-action="open-launcher"][data-account="s2"]').click();
  await page.waitForTimeout(100);
  assert.equal(writes.at(-1)[0], '/api/steam/open-launcher');
  assert.equal(writes.at(-1)[1].account_name, 's2');

  // Click "Открыть в лаунчере" on epic account e2
  await page.locator('button[data-action="open-launcher"][data-account="e2"]').click();
  await page.waitForTimeout(100);
  assert.equal(writes.at(-1)[0], '/api/epic/open-launcher');
  assert.equal(writes.at(-1)[1].account_id, 'e2');

  assert.deepEqual(errors, []);
  console.log('PASS: accounts tab renders "Открыть в лаунчере" buttons and triggers correct launcher endpoints.');
  await browser.close();
})();
