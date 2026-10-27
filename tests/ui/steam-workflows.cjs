const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
(async()=>{
 const browser=await chromium.launch({channel:'chrome',headless:true});const page=await browser.newPage({viewport:{width:1440,height:1050}});
 const errors=[],writes=[],prices=[];page.on('pageerror',e=>errors.push(e.message));
 const a='76561197960265729',b='76561197960265730';let active=a,progress=25;
 const accounts=()=>[{id:a,username:'a',display_name:'Основной',launcher:'steam',is_active:active===a},{id:b,username:'b',display_name:'Семейный',launcher:'steam',is_active:active===b}];
 const update=()=>({required:true,phase:'downloading',downloaded:progress*1024,download_total:100*1024,staged:0,stage_total:0});
 const row=(app,name,id,time,access,installed=true)=>({app_id:app,name,account_id:id,launcher:'steam',is_installed:installed,is_free:false,has_inventory:app!=='620',access,owner_steamids:[a],playtime_minutes:time,update:app==='730'?update():null});
 const games=()=>[row('730','Counter-Strike 2',a,120,'owned'),row('730','Counter-Strike 2',b,60,'family'),row('570','Dota 2',a,600,'owned'),row('620','Portal 2',a,0,'owned',false)];
 const items=(id,app)=>[{asset_id:'900719925474099300',name:'Shared name',market_hash_name:'Shared name',amount:id===a?2:1,tradable:id===a,marketable:true,item_type:'Скин',tags:['Rare','Rifle'],tag_details:[{category:'rarity',category_name:'Редкость',value:'rare',name:'Rare'},{category:'type',category_name:'Тип',value:'rifle',name:'Rifle'}]}];
 await page.route('**/api/**',async route=>{
  const req=route.request(),u=new URL(req.url()),path=u.pathname;
  if(path==='/api/steam/avatar')return route.fulfill({status:404,body:''});
  if(req.method()==='POST'){const payload=req.postDataJSON();writes.push([path,payload]);if(path==='/api/steam/launch')active=payload.account_id;return route.fulfill({json:{status:'requested',message:'Запрос принят'}});}
  if(path==='/api/steam/library')return route.fulfill({json:{accounts:accounts(),games:games(),warnings:[]}});
  if(path==='/api/epic/accounts')return route.fulfill({json:{accounts:[]}});
  if(path==='/api/steam/downloads')return route.fulfill({json:{games:{730:{update:update(),is_installed:true}}}});
  if(path==='/api/steam/inventory-contexts')return route.fulfill({json:{contexts:[{app_id:'730',context_id:'2',name:'Counter-Strike 2'},{app_id:'570',context_id:'2',name:'Dota 2'}]}});
  if(path==='/api/steam/inventory')return route.fulfill({json:{items:items(u.searchParams.get('account_id'),u.searchParams.get('app_id')),total_count:1,next_cursor:null,updated_at:new Date().toISOString(),cached:false,warning:null}});
  if(path==='/api/steam/price'){const app=u.searchParams.get('app_id');prices.push(app);return route.fulfill({json:{unit_price_cents:app==='730'?100:500,currency:'USD',updated_at:new Date().toISOString()}});}
  return route.fulfill({json:{}});
 });
 await page.goto(process.env.SWCH_UI_URL||'http://127.0.0.1:4245/');await page.locator('.game-card').first().waitFor();
 assert.equal(await page.locator('[data-action="quick-launch"]').count(),3);
 assert(!await page.locator('[data-action="quick-launch"][data-game="steam:620"]').isDisabled());assert((await page.locator('.game-card').filter({hasText:'Portal 2'}).innerText()).includes('Не установлена'));assert((await page.locator('[data-action="quick-launch"][data-game="steam:620"]').innerText()).includes('Установить'));
 await page.locator('[data-action="quick-launch"][data-game="steam:730"]').click();await page.locator('#launch-account-dialog').waitFor();assert.equal(writes.length,0);assert((await page.locator('#launch-account-dialog').innerText()).includes('Семейный доступ'));
 await page.locator(`[data-action="launch-account"][data-account="${b}"]`).click();await page.waitForFunction(()=>!document.querySelector('#launch-account-dialog').open);assert.deepEqual(writes[0],['/api/steam/launch',{app_id:'730',account_id:b,update_only:false}]);assert.equal(await page.locator('.game-card').count(),3,'quick launch keeps card view');
 await page.locator('[data-action="styles"]').first().click();await page.locator('[data-style-choice="steam"]').click();assert.equal(await page.locator('.game-list').count(),1);assert.equal(await page.locator('.detail-panel').count(),1);
 await page.locator('[data-action="select-game"][data-game="steam:730"]').click();assert((await page.locator('.aggregate-metrics').first().innerText()).includes('3 ч'));assert((await page.locator('.download-status').innerText()).includes('25%'));
 progress=75;await page.waitForFunction(()=>document.querySelector('.download-status')?.textContent.includes('75%'),null,{timeout:10000});
 await page.locator('[data-action="update-game"]').click();await page.locator(`[data-action="launch-account"][data-account="${a}"]`).click();assert.equal(writes.at(-1)[1].update_only,true);
 await page.locator('[data-page="inventory"]').click();await page.locator('.inventory-item').nth(3).waitFor();assert.equal(await page.locator('#inventory-app').inputValue(),'all');assert.equal(await page.locator('.inventory-item').count(),4);
 await page.locator('[data-action="inventory-value"]').click();await page.getByRole('status').filter({hasText:'Оценка загруженных предметов завершена'}).waitFor();assert.equal(prices.length,2,'same item name priced separately for each app');assert((await page.locator('.global-valuation').innerText()).includes('18,00'));
 await page.locator('#inventory-account').selectOption(a);assert.equal(await page.locator('.inventory-item').count(),2);assert((await page.locator('#inventory-valuation').innerText()).includes('12,00'));assert((await page.locator('.global-valuation').innerText()).includes('18,00'));
 await page.locator('#inventory-account').selectOption('all');await page.locator('#inventory-app').selectOption('730');assert.equal(await page.locator('.inventory-item').count(),2);assert((await page.locator('#inventory-valuation').innerText()).includes('3,00'));
 await page.locator('#inventory-app').selectOption('all');await page.locator('#inventory-sort').selectOption('price_desc');assert((await page.locator('.inventory-item').first().innerText()).includes('Dota 2'));
 await page.locator('.inventory-facets summary').click();await page.locator('[data-inventory-facet="730:rarity"]').check();assert.equal(await page.locator('.inventory-item').count(),2);await page.locator('#inventory-availability').selectOption('tradable');assert.equal(await page.locator('.inventory-item').count(),1);
 await page.locator('[data-action="inventory-reset"]').click();assert.equal(await page.locator('.inventory-item').count(),4);
 await page.screenshot({path:'/tmp/swch-all-games-inventory.png',fullPage:true});
 await page.locator('[data-page="library"]').click();await page.locator('[data-action="select-game"][data-game="steam:730"]').click();assert((await page.locator('.aggregate-metrics').first().innerText()).includes('3,00'));await page.locator(`[data-action="select-account"][data-account="${b}"]`).click();assert((await page.locator('.aggregate-metrics').nth(1).innerText()).includes('1,00'));
 await page.screenshot({path:'/tmp/swch-game-totals-updates.png',fullPage:true});
 for(const width of [800,390]){await page.setViewportSize({width,height:900});await page.screenshot({path:`/tmp/swch-details-${width}.png`,fullPage:true});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,JSON.stringify(await page.evaluate(()=>[...document.querySelectorAll('body *')].filter(e=>e.getBoundingClientRect().right>innerWidth).map(e=>[e.tagName,e.className,e.getBoundingClientRect().width]).slice(-12))));}
 await page.reload();await page.locator('.game-list').waitFor();assert.equal(await page.locator('.game-card').count(),0,'Steam style retains left-list layout after reload');
 assert.deepEqual(errors,[]);console.log('PASS: six Steam workflows: cross-game/account totals, Steam facets and sort, account launch picker, atomic launch request, live update progress, family labels, style layouts.');await browser.close();
})().catch(e=>{console.error(e);process.exit(1)});
