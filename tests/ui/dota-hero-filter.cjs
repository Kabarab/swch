const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || '/Users/nazarzukov/Desktop/zcode-account-manager-public-main/node_modules/playwright');

const rootDir = path.resolve(__dirname, '../..');

const mimeTypes = {
  '.html': 'text/html',
  '.js': 'application/javascript',
  '.mjs': 'application/javascript',
  '.css': 'text/css',
  '.json': 'application/json',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml'
};

const server = http.createServer((req, res) => {
  const parsed = new URL(req.url, 'http://127.0.0.1');
  let filePath = path.join(rootDir, parsed.pathname === '/' ? 'index.html' : parsed.pathname);
  if (fs.existsSync(filePath) && fs.statSync(filePath).isFile()) {
    const ext = path.extname(filePath).toLowerCase();
    res.writeHead(200, { 'Content-Type': mimeTypes[ext] || 'application/octet-stream' });
    fs.createReadStream(filePath).pipe(res);
  } else {
    res.writeHead(404, { 'Content-Type': 'text/plain' });
    res.end('Not found: ' + req.url);
  }
});

(async () => {
  const PORT = 4333;
  await new Promise(resolve => server.listen(PORT, '127.0.0.1', resolve));
  console.log(`Test static server listening on http://127.0.0.1:${PORT}`);

  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));

  const accId = '76561198000000001';
  const accounts = [
    { id: accId, username: 'testuser', display_name: 'Dota Player', launcher: 'steam', is_active: true }
  ];
  const games = [
    { id: 'steam_730', app_id: '730', name: 'Counter-Strike 2', launcher: 'steam', is_installed: true, account_id: accId, access: 'owned' },
    { id: 'steam_570', app_id: '570', name: 'Dota 2', launcher: 'steam', is_installed: true, account_id: accId, access: 'owned' }
  ];

  const csItem = {
    asset_id: '101',
    name: 'AK-47 | Redline',
    amount: 1,
    item_type: 'Rifle',
    tags: ['Rifle', 'Covert'],
    tag_details: [{ category: 'Type', category_name: 'Type', value: 'CSGO_Type_Rifle', name: 'Rifle' }],
    tradable: true,
    marketable: true,
  };

  const pudgeItem = {
    asset_id: '201',
    name: 'Dragonclaw Hook',
    amount: 1,
    item_type: 'Immortal Claw',
    tags: ['Immortal', 'Pudge'],
    tag_details: [
      { category: 'Hero', category_name: 'Герой', value: 'npc_dota_hero_pudge', name: 'Pudge' },
      { category: 'Rarity', category_name: 'Редкость', value: 'Rarity_Immortal', name: 'Immortal' }
    ],
    tradable: true,
    marketable: true,
  };

  const juggItem = {
    asset_id: '202',
    name: 'Bladeform Legacy',
    amount: 1,
    item_type: 'Arcana Mask',
    tags: ['Arcana', 'Juggernaut'],
    tag_details: [
      { category: 'Hero', category_name: 'Герой', value: 'npc_dota_hero_juggernaut', name: 'Juggernaut' },
      { category: 'Rarity', category_name: 'Редкость', value: 'Rarity_Arcana', name: 'Arcana' }
    ],
    tradable: true,
    marketable: true,
  };

  await page.route('**/api/**', async route => {
    const req = route.request();
    const u = new URL(req.url());
    if (u.pathname === '/api/steam/library') {
      return route.fulfill({ json: { accounts, games, warnings: [] } });
    }
    if (u.pathname === '/api/epic/accounts') {
      return route.fulfill({ json: { accounts: [] } });
    }
    if (u.pathname === '/api/steam/inventory-contexts') {
      return route.fulfill({
        json: {
          contexts: [
            { app_id: '730', context_id: '2', name: 'Counter-Strike 2', count: 1 },
            { app_id: '570', context_id: '2', name: 'Dota 2', count: 2 }
          ]
        }
      });
    }
    if (u.pathname === '/api/steam/inventory') {
      const appId = u.searchParams.get('app_id');
      if (appId === '730') {
        return route.fulfill({ json: { items: [csItem], total_count: 1, next_cursor: null, updated_at: '2026-09-19T12:00:00Z', cached: false } });
      }
      if (appId === '570') {
        return route.fulfill({ json: { items: [pudgeItem, juggItem], total_count: 2, next_cursor: null, updated_at: '2026-09-19T12:00:00Z', cached: false } });
      }
      // 'all'
      return route.fulfill({ json: { items: [csItem, pudgeItem, juggItem], total_count: 3, next_cursor: null, updated_at: '2026-09-19T12:00:00Z', cached: false } });
    }
    return route.fulfill({ json: {} });
  });

  try {
    await page.goto(`http://127.0.0.1:${PORT}/`);
    await page.locator('.game-card').first().waitFor();

    console.log('1. Navigating to Inventory...');
    await page.locator('[data-page="inventory"]').click();
    await page.locator('.steam-game-logos-bar').waitFor();

    console.log('2. Verifying hero filter is NOT shown on "Все игры"...');
    // Initially, inventoryApp is 'all'
    assert.equal(await page.locator('#inventory-app').inputValue(), 'all');
    assert.equal(await page.locator('.hero-filter-box').count(), 0, 'Hero filter must NOT be visible when all games are selected');
    // Verify that Dota 2 option exists in the dropdown even when "Все игры" is selected
    const dotaOption = page.locator('#inventory-app option[value="570"]');
    assert.equal(await dotaOption.count(), 1, 'Dota 2 must be available in inventory-app dropdown even on "Все игры"');

    console.log('3. Verifying Dota 2 logo in steam-game-logos-bar...');
    const dotaBtn = page.locator('.steam-game-logo-btn[data-app="570"]');
    assert.equal(await dotaBtn.count(), 1, 'Dota 2 button must be in logos bar');
    const dotaBadge = dotaBtn.locator('.dota2-badge');
    assert.equal(await dotaBadge.count(), 1, 'Dota 2 button must have dota2-badge');
    const dotaSvg = dotaBadge.locator('svg.dota2-icon-svg');
    assert.equal(await dotaSvg.count(), 1, 'Dota 2 logo SVG must be rendered inside badge');
    const svgPathFill = await dotaSvg.locator('path').getAttribute('fill');
    assert.equal(svgPathFill, '#e23b24', 'Dota 2 logo SVG path must have authentic Dota red fill (#e23b24)');

    console.log('4. Selecting Dota 2 game...');
    await dotaBtn.click();
    await page.locator('.hero-filter-box').waitFor();

    assert(await dotaBtn.getAttribute('class').then(cls => cls.includes('active')), 'Dota 2 button must be active');
    assert.equal(await page.locator('#inventory-app').inputValue(), '570', 'inventoryApp must be 570');

    console.log('5. Verifying hero filter appeared with Dota 2 branding and hero chips...');
    const heroBox = page.locator('.hero-filter-box');
    assert.equal(await heroBox.count(), 1, 'Hero filter must appear when Dota 2 is selected');
    assert.equal(await heroBox.locator('.dota2-badge').count(), 1, 'Hero filter header must display Dota 2 badge');
    const heroTitle = await heroBox.locator('.hero-filter-title strong').innerText();
    assert.equal(heroTitle, 'Фильтр по героям Dota 2');

    // Hero chips
    const allHeroesChip = heroBox.locator('.hero-chip[data-hero="all"]');
    assert.equal(await allHeroesChip.count(), 1);
    const pudgeChip = heroBox.locator('.hero-chip[data-hero="npc_dota_hero_pudge"]');
    assert.equal(await pudgeChip.count(), 1);
    assert((await pudgeChip.innerText()).includes('Pudge'));
    assert.equal(await pudgeChip.getAttribute('data-hero-slug'), 'pudge');

    const juggChip = heroBox.locator('.hero-chip[data-hero="npc_dota_hero_juggernaut"]');
    assert.equal(await juggChip.count(), 1);
    assert((await juggChip.innerText()).includes('Juggernaut'));
    assert.equal(await juggChip.getAttribute('data-hero-slug'), 'juggernaut');

    await page.screenshot({ path: '/tmp/swch-dota-selected-hero-filter.png', fullPage: true });

    console.log('6. Testing hero chip selection and filtering items...');
    await pudgeChip.click();
    assert(await pudgeChip.getAttribute('class').then(cls => cls.includes('active')));
    // Check items grid has only Pudge item
    await page.locator('.inventory-item').waitFor();
    assert.equal(await page.locator('.inventory-item').count(), 1);
    assert((await page.locator('.inventory-item').first().innerText()).includes('Dragonclaw Hook'));

    console.log('7. Resetting hero selection to "Все герои"...');
    await allHeroesChip.click();
    assert.equal(await page.locator('.inventory-item').count(), 2);

    console.log('8. Testing live hero search input (including slug matching)...');
    const heroSearch = heroBox.locator('#hero-filter-search');
    await heroSearch.fill('jugg');
    assert.equal(await pudgeChip.isHidden(), true, 'Pudge chip should be hidden by search');
    assert.equal(await juggChip.isHidden(), false, 'Juggernaut chip should remain visible');
    await heroSearch.fill('pudge');
    assert.equal(await pudgeChip.isHidden(), false, 'Pudge chip matched by slug');
    assert.equal(await juggChip.isHidden(), true, 'Juggernaut chip hidden');
    await heroSearch.fill('');

    console.log('9. Switching back to "Все игры" — hero filter must disappear...');
    const allGamesBtn = page.locator('.steam-game-logo-btn[data-app="all"]');
    await allGamesBtn.click();
    assert.equal(await page.locator('#inventory-app').inputValue(), 'all');
    assert.equal(await page.locator('.hero-filter-box').count(), 0, 'Hero filter must disappear on "Все игры"');

    await page.screenshot({ path: '/tmp/swch-all-games-no-hero-filter.png', fullPage: true });

    console.log('10. Switching to Counter-Strike 2 — hero filter must NOT appear...');
    const csBtn = page.locator('.steam-game-logo-btn[data-app="730"]');
    await csBtn.click();
    assert.equal(await page.locator('#inventory-app').inputValue(), '730');
    assert.equal(await page.locator('.hero-filter-box').count(), 0, 'Hero filter must not appear for CS2');

    console.log('11. Selecting Dota 2 via dropdown select...');
    await page.locator('#inventory-app').selectOption('570');
    await page.locator('.hero-filter-box').waitFor();
    assert.equal(await page.locator('.hero-filter-box').count(), 1, 'Hero filter must appear when selecting Dota 2 via select');
    assert(await dotaBtn.getAttribute('class').then(cls => cls.includes('active')), 'Dota 2 logo button must be active');

    console.log('12. Verifying switching accounts while Dota 2 is selected...');
    await page.locator('#inventory-account').selectOption('76561198000000001');
    assert.equal(await page.locator('#inventory-app').inputValue(), '570', 'Dota 2 remains selected across account switch');
    assert.equal(await page.locator('.hero-filter-box').count(), 1, 'Hero filter re-evaluates cleanly for account');

    assert.deepEqual(errors, [], 'No uncaught JS errors');
    console.log('ALL DOTA HERO FILTER BROWSER TESTS PASSED SUCCESSFULLY!');
  } finally {
    await browser.close();
    server.close();
  }
})().catch(err => {
  console.error('Test failed:', err);
  server.close();
  process.exit(1);
});
