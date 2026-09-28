const {chromium}=require(process.env.PLAYWRIGHT_MODULE || 'playwright');
(async()=>{
const browser=await chromium.launch({channel:'chrome',headless:true});
const page=await browser.newPage({viewport:{width:1440,height:1050},deviceScaleFactor:1});
const errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.goto((process.env.SWCH_UI_URL || 'http://127.0.0.1:4243/') + '?demo=1');await page.locator('.game-card').first().waitFor();await page.screenshot({path:'/tmp/swch-cards.png',fullPage:true});
await page.getByRole('button',{name:'Стиль: Epic Games'}).click();await page.screenshot({path:'/tmp/swch-styles.png'});
for(const name of ['Xbox','PlayStation','Steam','Riot Games','Epic Games']) {
 if(!await page.locator('#styles-dialog').isVisible())await page.locator('.page-heading [data-action="styles"]').click();
 await page.locator(`[data-style-choice="${{Xbox:'xbox',PlayStation:'playstation',Steam:'steam','Riot Games':'riot','Epic Games':'epic'}[name]}"]`).click();
}
await page.getByRole('button',{name:'Открыть Counter-Strike 2',exact:true}).click();
await page.screenshot({path:'/tmp/swch-detail.png',fullPage:true});
await page.locator('[data-action="select-account"][data-account="demo-shadow"]').click();
await page.getByRole('button',{name:'Изменить данные'}).click();await page.locator('input[name="skins"]').fill('333');await page.getByRole('button',{name:'Сохранить статистику'}).click();
if(!await page.locator('.metrics').innerText().then(t=>t.includes('333')))throw Error('Metrics were not updated');
await page.locator('[data-action="select-account"][data-account="demo-nova"]').click();if(await page.locator('.metrics').innerText().then(t=>t.includes('333')))throw Error('Account metrics leak');
await page.reload();await page.locator('.account-section').waitFor();await page.locator('[data-action="select-account"][data-account="demo-shadow"]').click();if(!await page.locator('.metrics').innerText().then(t=>t.includes('333')))throw Error('Metrics were not persisted');
await page.locator('#search-input').fill('nonexistent');await page.getByRole('heading',{name:'Пока ничего не найдено'}).waitFor();await page.getByRole('button',{name:'Сбросить фильтры'}).click();
await page.getByRole('button',{name:'Карточки игр',exact:true}).click();await page.locator('[data-action="favorite"]').first().click();await page.locator('[data-page="favorites"]').click();if(await page.locator('.game-card').count()!==1)throw Error('Favorites failed');
for(const width of [1280,1024,800,390]){await page.setViewportSize({width,height:950});if(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth))throw Error(`Overflow at ${width}`);}
await page.setViewportSize({width:1440,height:1050});
await page.goto('http://127.0.0.1:4243/');await page.locator('.empty-state').waitFor();await page.screenshot({path:'/tmp/swch-empty.png'});
console.log(JSON.stringify({errors,checks:'5 styles, layouts, metrics isolation/persistence, search, favorites, 4 viewport widths, offline state'}));
if(errors.length)throw Error(errors.join('\n'));await browser.close();
})();
