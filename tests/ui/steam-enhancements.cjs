const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
(async()=>{
 const browser=await chromium.launch({channel:'chrome',headless:true});
 const page=await browser.newPage({viewport:{width:1440,height:1050}});const errors=[],writes=[],priceCalls=[];page.on('pageerror',e=>errors.push(e.message));
 const a='76561197960265729',b='76561197960265730',c='76561197960265731';
 const accounts=[{id:a,username:'a',display_name:'Основной',launcher:'steam',is_active:true},{id:b,username:'b',display_name:'Второй',launcher:'steam'},{id:c,username:'c',display_name:'Закрытый',launcher:'steam'}];
 const row=(id,name,account,time,free,inv,access='owned')=>({app_id:id,id:'steam_'+id,name,account_id:account,playtime_minutes:time,is_free:free,has_inventory:inv,access,owner_steamids:[account],launcher:'steam',is_installed:true});
 const games=[row('730','Counter-Strike 2',a,120,true,true),row('730','Counter-Strike 2',b,60,true,true),row('620','Portal 2',a,600,false,false),row('570','Dota 2',a,1200,true,true)];
 const item=(id,name,amount,tradable,marketable)=>({asset_id:id,name,market_hash_name:name,amount,tradable,marketable,item_type:'Предмет',tags:['Rare']});
 const date='2026-09-15T00:00:00Z';
 await page.route('**/api/**',async route=>{
  const req=route.request(),u=new URL(req.url());if(req.method()==='POST')writes.push(u.pathname);
  if(u.pathname==='/api/steam/avatar')return route.fulfill({status:200,contentType:'image/svg+xml',body:'<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" fill="teal"/></svg>'});
  if(u.pathname==='/api/steam/library')return route.fulfill({json:{accounts,games,warnings:[]}});
  if(u.pathname==='/api/epic/accounts')return route.fulfill({json:{accounts:[]}});
  if(u.pathname==='/api/steam/price'){priceCalls.push(u.searchParams.get('market_hash_name'));return route.fulfill({json:{unit_price_cents:u.searchParams.get('market_hash_name')==='Skin'?100:null,currency:'USD',updated_at:new Date().toISOString(),cached:false,warning:null}});}
  if(u.pathname==='/api/steam/inventory'){
   const id=u.searchParams.get('account_id'),cursor=u.searchParams.get('cursor');
   if(id===c)return route.fulfill({status:502,json:{error:'Инвентарь закрыт'}});
   const items=id===b?[item('1','Skin',2,false,true)]:cursor?[item('2','Unknown',1,true,true),item('3','Gift',1,true,false)]:[item('1','Skin',3,true,true)];
   return route.fulfill({json:{items,total_count:id===b?1:3,next_cursor:id===a&&!cursor?'1':null,updated_at:date,cached:false,warning:null}});
  }
  return route.fulfill({json:{}});
 });
 await page.goto(process.env.SWCH_UI_URL||'http://127.0.0.1:4245/');await page.locator('.game-card').first().waitFor();
 await page.locator('[data-action="favorite"][data-game="steam:620"]').click();assert.equal(await page.locator('.game-card .poster-button').first().getAttribute('data-game'),'steam:620');
 await page.locator('#sort-select').selectOption('hours');assert.equal(await page.locator('.game-card .poster-button').first().getAttribute('data-game'),'steam:620');assert.equal(await page.locator('.game-card .poster-button').nth(1).getAttribute('data-game'),'steam:570');
 await page.getByRole('button',{name:'Открыть Portal 2',exact:true}).click();assert.equal(await page.locator('.account-chip').count(),1);assert.equal(await page.locator('[data-action="inventory"]').count(),0);assert.equal(await page.locator('.account-chip img[data-avatar]').count(),1);
 await page.locator('[data-action="select-game"][data-game="steam:730"]').click();assert.equal(await page.locator('.account-chip').count(),3);assert((await page.locator('.aggregate-metrics').first().innerText()).includes('3 ч'));
 await page.locator(`[data-action="select-account"][data-account="${b}"]`).click();assert((await page.locator('.metrics').innerText()).includes('1 ч'));assert.equal(writes.length,0);
 await page.getByRole('button',{name:'Инвентари всех аккаунтов',exact:true}).click();await page.getByRole('alert').filter({hasText:'Инвентарь закрыт'}).waitFor();assert.equal(await page.locator('.inventory-item').count(),2,'same asset IDs from two accounts must stay separate');
 await page.getByRole('button',{name:'Загрузить все страницы',exact:true}).click();await page.locator('.inventory-item').nth(3).waitFor();assert.equal(await page.locator('.inventory-item').count(),4);
 await page.locator('#inventory-availability').selectOption('tradable');assert.equal(await page.locator('.inventory-item').count(),3);
 await page.getByRole('button',{name:'Оценить стоимость',exact:true}).click();await page.getByRole('status').filter({hasText:'Оценка загруженных предметов завершена'}).waitFor();
 const valuation=await page.locator('#inventory-valuation').innerText();assert(valuation.includes('5,00'),'all-account valuation must include quantities');assert(valuation.includes('3,00'),'filtered valuation must only include tradable stack');assert(valuation.includes('без цены 1'));assert(valuation.includes('не продаётся 1'));assert(valuation.includes('частичная'));assert.equal(priceCalls.filter(n=>n==='Skin').length,1);
 await page.locator('#inventory-availability').selectOption('marketable');assert.equal(await page.locator('.inventory-item').count(),3);await page.locator('#inventory-availability').selectOption('all');
 await page.screenshot({path:'/tmp/swch-inventory-all-valued.png',fullPage:true});
 await page.locator('#inventory-account').selectOption(b);assert.equal(await page.locator('.inventory-item').count(),1);assert((await page.locator('#inventory-valuation').innerText()).includes('2,00'));
 await page.locator('#inventory-account').selectOption('all');
 for(const width of [800,390]){await page.setViewportSize({width,height:950});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,'mobile overflow');}
 await page.screenshot({path:'/tmp/swch-inventory-all-mobile.png',fullPage:true});
 assert.deepEqual(errors,[]);assert.equal(writes.length,0);console.log('PASS: all 8 requested Steam enhancements, partial inventory accounting, price deduplication, quantities, filters, avatars and mobile layout.');await browser.close();
})().catch(e=>{console.error(e);process.exit(1)});
