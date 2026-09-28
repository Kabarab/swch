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
  await new Promise(resolve => server.listen(4321, '127.0.0.1', resolve));
  console.log('Test static server listening on http://127.0.0.1:4321');

  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));

  try {
    await page.goto('http://127.0.0.1:4321/?demo=1');
    await page.locator('.game-card').first().waitFor();

    console.log('1. Testing navigation to Inventory tab...');
    await page.locator('[data-page="inventory"]').click();
    await page.locator('.inventory-launcher-bar').waitFor();

    // Check launcher switcher buttons
    assert.equal(await page.locator('[data-inventory-launcher="steam"]').count(), 1);
    assert.equal(await page.locator('[data-inventory-launcher="epic"]').count(), 1);

    console.log('2. Switching to Epic Games (Fortnite locker)...');
    await page.locator('[data-inventory-launcher="epic"]').click();
    await page.locator('.fortnite-categories-container').waitFor();

    // Verify cache timestamp notice
    const cacheNotice = await page.locator('.fortnite-cache-notice').innerText();
    assert(cacheNotice.includes('🕒 Информация о шкафчике актуальна на:'), 'Cache notice must be present');
    assert(cacheNotice.includes('Обновить сейчас'), 'Refresh button must be present in cache notice');

    // Verify valuation card
    const heroBanner = await page.locator('.fortnite-hero-banner').first().innerText();
    assert(heroBanner.toUpperCase().includes('ОЦЕНКА ШКАФЧИКА'), 'Valuation card must be present');
    assert(heroBanner.includes('Формула:') && heroBanner.includes('VB'), 'Valuation formula must specify V-Bucks + in-game currency');
    assert(!heroBanner.includes('30 базовая'), 'Valuation formula must not include $30 base');
    assert(heroBanner.toUpperCase().includes('БАЛАНС V-BUCKS'), 'V-Bucks balance card must be present');

    console.log('3. Verifying category sections (по отдельности, не в перемешку)...');
    const sections = page.locator('.fortnite-category-section');
    const secCount = await sections.count();
    console.log(`Found ${secCount} category sections.`);
    assert(secCount >= 6, 'Must have multiple category sections');

    // Verify specific sections exist
    await page.locator('#fortnite-sec-outfit').waitFor();
    await page.locator('#fortnite-sec-backpack').waitFor();
    await page.locator('#fortnite-sec-pickaxe').waitFor();
    await page.locator('#fortnite-sec-emote').waitFor();
    await page.locator('#fortnite-sec-glider').waitFor();
    await page.locator('#fortnite-sec-wrap').waitFor();

    // Verify outfits section contains ONLY outfits
    const outfitCards = page.locator('#fortnite-sec-outfit .fortnite-card');
    const outfitCount = await outfitCards.count();
    assert(outfitCount > 0, 'Outfits section must contain outfit cards');
    for (let i = 0; i < outfitCount; i++) {
      const cardType = await outfitCards.nth(i).getAttribute('data-cosmetic-type');
      assert.equal(cardType, 'outfit', `Card ${i} in outfits section must be outfit`);
    }

    // Verify backpacks section contains ONLY backpacks
    const backpackCards = page.locator('#fortnite-sec-backpack .fortnite-card');
    const backpackCount = await backpackCards.count();
    assert(backpackCount > 0, 'Backpacks section must contain backpack cards');
    for (let i = 0; i < backpackCount; i++) {
      const cardType = await backpackCards.nth(i).getAttribute('data-cosmetic-type');
      assert.equal(cardType, 'backpack', `Card ${i} in backpacks section must be backpack`);
    }

    // Verify pickaxes section contains ONLY pickaxes
    const pickaxeCards = page.locator('#fortnite-sec-pickaxe .fortnite-card');
    const pickaxeCount = await pickaxeCards.count();
    assert(pickaxeCount > 0, 'Pickaxes section must contain pickaxe cards');
    for (let i = 0; i < pickaxeCount; i++) {
      const cardType = await pickaxeCards.nth(i).getAttribute('data-cosmetic-type');
      assert.equal(cardType, 'pickaxe', `Card ${i} in pickaxes section must be pickaxe`);
    }

    // Verify emotes section contains ONLY emotes
    const emoteCards = page.locator('#fortnite-sec-emote .fortnite-card');
    const emoteCount = await emoteCards.count();
    assert(emoteCount > 0, 'Emotes section must contain emote cards');
    for (let i = 0; i < emoteCount; i++) {
      const cardType = await emoteCards.nth(i).getAttribute('data-cosmetic-type');
      assert.equal(cardType, 'emote', `Card ${i} in emotes section must be emote`);
    }

    console.log('4. Verifying category tab filtering...');
    // Click "Скины" tab
    await page.locator('[data-fortnite-tab="outfit"]').click();
    assert.equal(await page.locator('#fortnite-sec-outfit').count(), 1);
    assert.equal(await page.locator('#fortnite-sec-backpack').count(), 0);
    assert.equal(await page.locator('#fortnite-sec-pickaxe').count(), 0);

    // Click "Рюкзаки" tab
    await page.locator('[data-fortnite-tab="backpack"]').click();
    assert.equal(await page.locator('#fortnite-sec-outfit').count(), 0);
    assert.equal(await page.locator('#fortnite-sec-backpack').count(), 1);
    assert.equal(await page.locator('#fortnite-sec-pickaxe').count(), 0);

    // Click "⭐ Эксклюзивы" tab
    await page.locator('[data-fortnite-tab="exclusive"]').click();
    const exclusiveSections = page.locator('.fortnite-category-section');
    const exclSecCount = await exclusiveSections.count();
    assert(exclSecCount > 0, 'Exclusive items should be displayed');
    const exclCards = page.locator('.fortnite-category-section .fortnite-card');
    const totalExcl = await exclCards.count();
    for (let i = 0; i < totalExcl; i++) {
      const text = await exclCards.nth(i).innerText();
      assert(text.toUpperCase().includes('ЭКСКЛЮЗИВ'), `Card ${i} must have exclusive badge`);
    }

    // Click "Все категории" tab
    await page.locator('[data-fortnite-tab="all"]').click();
    assert.equal(await page.locator('#fortnite-sec-outfit').count(), 1);
    assert.equal(await page.locator('#fortnite-sec-backpack').count(), 1);
    assert.equal(await page.locator('#fortnite-sec-pickaxe').count(), 1);

    console.log('5. Verifying live search input...');
    const searchInput = page.locator('#inventory-fortnite-search');
    await searchInput.fill('Трэвис');
    assert.equal(await page.locator('#fortnite-sec-outfit .fortnite-card').count(), 1);
    assert((await page.locator('#fortnite-sec-outfit').innerText()).includes('Трэвис Скотт'));
    assert.equal(await page.locator('#fortnite-sec-backpack').count(), 0);
    // Clear search
    await searchInput.fill('');
    assert.equal(await page.locator('#fortnite-sec-outfit .fortnite-card').count(), 4);
    assert.equal(await page.locator('#fortnite-sec-backpack').count(), 1);

    console.log('6. Verifying live rarity dropdown filter...');
    const raritySelect = page.locator('#inventory-fortnite-rarity');
    await raritySelect.selectOption('legendary');
    const legCards = page.locator('.fortnite-card');
    const legCount = await legCards.count();
    assert(legCount > 0, 'Should find legendary items');
    for (let i = 0; i < legCount; i++) {
      const classAttr = await legCards.nth(i).getAttribute('class');
      assert(classAttr.includes('rarity-legendary'), 'All shown cards must be legendary');
    }
    await raritySelect.selectOption('all');

    console.log('7. Verifying Steam tab and Steam game selector logos...');
    await page.locator('[data-inventory-launcher="steam"]').click();
    await page.locator('.steam-game-logos-bar').waitFor();
    const gameLogoBtns = page.locator('.steam-game-logo-btn');
    assert(await gameLogoBtns.count() >= 1, 'Should display games with skins');
    const firstBtn = gameLogoBtns.first();
    await firstBtn.click();
    assert(await firstBtn.getAttribute('class').then(cls => cls.includes('active')));

    console.log('8. Verifying opening Fortnite from library/account button...');
    await page.locator('[data-page="library"]').click();
    await page.locator('.game-card').first().waitFor();
    // Open Fortnite modal or inventory
    const fnCard = page.locator('.game-card').filter({ hasText: 'Fortnite' }).first();
    await fnCard.getByRole('button', { name: 'Открыть Fortnite' }).click();
    await page.locator('#game-modal').waitFor();
    const openFnBtn = page.locator('[data-action="open-fortnite"]');
    if (await openFnBtn.count() > 0) {
      await openFnBtn.click();
      await page.locator('.fortnite-categories-container').waitFor();
      assert.equal(await page.locator('#inventory-view').isVisible(), true);
    }

    console.log('9. Verifying empty locker edge case (0 total cosmetics)...');
    await page.evaluate(() => {
      currentFortniteSummary = {
        account_id: 'demo-epic',
        display_name: 'empty.user',
        total_vbucks: 1000,
        cosmetics: []
      };
      updateFortniteItems();
    });
    const emptyNotice = await page.locator('#inventory-fortnite-content').innerText();
    assert(emptyNotice.includes('В шкафчике пока нет предметов'), 'Must show empty locker message when 0 items');

    console.log('10. Verifying unknown cosmetic type fallback into single "other" section...');
    await page.evaluate(() => {
      currentFortniteSummary = {
        account_id: 'demo-epic',
        display_name: 'test.user',
        total_vbucks: 500,
        cosmetics: [
          { id: 'custom_1', name: 'Секретный предмет', cosmetic_type: 'unknown_gadget', rarity: 'rare' },
          { id: 'custom_2', name: 'Другая штука', cosmetic_type: 'other', rarity: 'uncommon' }
        ]
      };
      updateFortniteItems();
    });
    const otherSections = page.locator('#fortnite-sec-other');
    assert.equal(await otherSections.count(), 1, 'Must have exactly ONE "other" section');
    const otherCards = page.locator('#fortnite-sec-other .fortnite-card');
    assert.equal(await otherCards.count(), 2, 'Both unrecognized and other items grouped in "other"');

    console.log('11. Restoring demo summary and verifying full rendering...');
    await page.evaluate(() => {
      currentFortniteSummary = getDemoFortniteSummary();
      updateFortniteItems();
    });
    assert.equal((await page.locator('.fortnite-category-section').count()) >= 6, true);

    assert.deepEqual(errors, [], 'No uncaught JS errors');
    console.log('ALL FORTNITE BROWSER TESTS PASSED!');
  } finally {
    await browser.close();
    server.close();
  }
})().catch(err => {
  console.error('Test failed:', err);
  server.close();
  process.exit(1);
});
