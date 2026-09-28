const {chromium}=require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert=require('node:assert/strict');
(async()=>{
 const browser=await chromium.launch({channel:'chrome',headless:true});
 const page=await browser.newPage({viewport:{width:1440,height:1050}}), errors=[],writes=[];
 page.on('pageerror',e=>errors.push(e.message));
 const a='76561197960265729',b='76561197960265730';
 const accounts=[{id:a,username:'one',display_name:'Основной',launcher:'steam',is_active:true},{id:b,username:'two',display_name:'Второй',launcher:'steam',is_active:false}];
 const games=[{id:'steam_730',app_id:'730',name:'Counter-Strike 2',launcher:'steam',is_installed:true,account_id:a,access:'owned',playtime_minutes:120,source:'steam_api',updated_at:'2026-09-14T12:00:00Z',owner_steamids:[a]},{id:'steam_730',app_id:'730',name:'Counter-Strike 2',launcher:'steam',is_installed:true,account_id:b,access:'local',playtime_minutes:0,source:'local',owner_steamids:[]},{id:'steam_620',app_id:'620',name:'Portal 2',launcher:'steam',is_installed:false,account_id:a,access:'family',owner_steamids:[b]}];
 const item=(id,name)=>({asset_id:id,name,amount:1,item_type:'Винтовка',tags:['Засекреченное','Винтовка'],tradable:true,marketable:false});
 await page.route('**/api/**',async route=>{
  const req=route.request(),u=new URL(req.url());
  if(req.method()==='POST'){writes.push([u.pathname,req.postDataJSON()]);return route.fulfill({status:502,json:{error:'Проверочная ошибка Steam'}});}
  let json={};
  if(u.pathname==='/api/steam/library')json={accounts,games,warnings:[]};
  if(u.pathname==='/api/epic/accounts')json={accounts:[]};
  if(u.pathname==='/api/steam/inventory'){
   if(u.searchParams.get('account_id')===b){await new Promise(r=>setTimeout(r,250));return route.fulfill({status:502,json:{error:'Инвентарь закрыт'}});}
   json={items:u.searchParams.has('cursor')?[item('1','AK-47 | Красная линия'),item('2','M4A1-S | Страж')]:[item('1','AK-47 | Красная линия')],total_count:2,next_cursor:u.searchParams.has('cursor')?null:'1',updated_at:'2026-09-14T12:00:00Z',cached:false};
  }
  return route.fulfill({json});
 });
 await page.goto(process.env.SWCH_UI_URL||'http://127.0.0.1:4245/');await page.locator('.game-card').first().waitFor();assert.equal(await page.locator('.game-card').count(),2);
 await page.locator('#steam-access-filter').selectOption('family');assert.equal(await page.locator('.game-card').count(),1);await page.getByRole('button',{name:'Открыть Portal 2',exact:true}).click();assert((await page.locator('.account-section').innerText()).includes('Семейный доступ'));
 await page.locator('#steam-access-filter').selectOption('all');await page.locator('[data-action="select-game"][data-game="steam:730"]').click();assert((await page.locator('.metrics').innerText()).includes('2 ч'));
 await page.locator(`[data-action="select-account"][data-account="${b}"]`).click();assert((await page.locator('.metrics').innerText()).includes('0 ч'));assert.equal(writes.length,0);
 await page.locator(`[data-action="select-account"][data-account="${a}"]`).click();await page.getByRole('button',{name:'Предметы и скины',exact:true}).click();await page.locator('.inventory-item').waitFor();
 await page.getByRole('button',{name:'Загрузить ещё',exact:true}).click();await page.locator('.inventory-item').nth(1).waitFor();assert.equal(await page.locator('.inventory-item').count(),2);
 await page.locator('#inventory-search').fill('страж');assert.equal(await page.locator('.inventory-item').count(),1);await page.locator('#inventory-search').fill('');
 await page.screenshot({path:'/tmp/swch-steam-inventory.png',fullPage:true});
 await page.locator('#inventory-account').selectOption(b);assert.equal(await page.locator('.inventory-item').count(),0);await page.getByRole('alert').filter({hasText:'Инвентарь закрыт'}).waitFor();assert.equal(await page.locator('.inventory-item').count(),0);assert.equal(writes.length,0);
 await page.locator('#inventory-account').selectOption(a);await page.locator('.inventory-item').nth(1).waitFor();
 for(const width of [1280,800,390]){await page.setViewportSize({width,height:950});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,`overflow at ${width}`);}
 await page.screenshot({path:'/tmp/swch-steam-inventory-mobile.png',fullPage:true});
 await page.setViewportSize({width:1440,height:1050});await page.locator('.connect-link').click();await page.locator('#steam-api-key').fill('test-secret');await page.locator('.steam-connect summary').click();await page.locator('#steam-access-token').fill('test-token');await page.getByRole('button',{name:'Обновить игры и семейный доступ',exact:true}).click();await page.locator('#steam-sync-error').filter({hasText:'Проверочная ошибка Steam'}).waitFor();assert.equal(await page.locator('#steam-api-key').inputValue(),'');assert.equal(await page.locator('#steam-access-token').inputValue(),'');assert.equal(await page.evaluate(()=>JSON.stringify(localStorage).includes('test-secret')),false);
 assert.deepEqual(writes[0],['/api/steam/library/sync',{account_id:a,api_key:'test-secret',access_token:'test-token'}]);assert.deepEqual(errors,[]);
 console.log('PASS: Steam library and family filters, per-account hours, inventory without switching, pagination, search, private inventory, account isolation, mobile layout, sync errors, credential clearing.');await browser.close();
})().catch(e=>{console.error(e);process.exit(1)});
