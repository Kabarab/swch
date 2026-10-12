const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
const assert=require('node:assert/strict');
(async()=>{
 const server=await staticServer(),browser=await chromium.launch(launchOptions);
 try{
  const page=await browser.newPage(),errors=[];let roblox=false,steam=true,riot=true,releaseInventory,inventoryStarted;
  const started=new Promise(resolve=>inventoryStarted=resolve),released=new Promise(resolve=>releaseInventory=resolve);
  page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(()=>localStorage.setItem('swch-inventory-launcher',JSON.stringify('roblox')));
  await page.route('**/api/**',async route=>{
   const url=new URL(route.request().url());let json=emptyApi(url.pathname);
   if(url.pathname==='/api/steam/library')json={games:[],accounts:steam?[{id:'steam',launcher:'steam',display_name:'Steam'}]:[],warnings:[]};
   if(url.pathname==='/api/riot/accounts')json={accounts:riot?[{id:'riot',display_name:'Riot'}]:[],profiles:[]};
   if(url.pathname==='/api/roblox/library')json={games:[{id:'roblox',launcher:'roblox',app_id:'roblox',name:'Roblox'}]};
   if(url.pathname==='/api/roblox/accounts')json=roblox?[{account_id:'roblox',display_name:'Roblox',username:'Player'}]:[];
   if(url.pathname==='/api/roblox/inventory'){inventoryStarted();await released;json={collectibles:[],wearing_asset_ids:[],total_rap:0};}
   await route.fulfill({json});
  });
  await page.goto(server.origin);await page.waitForFunction(()=>!document.querySelector('.page-heading [data-action="refresh"]').disabled);
  await page.locator('[data-page="inventory"]').click();
  assert.deepEqual(await page.locator('[data-inventory-launcher]').evaluateAll(nodes=>nodes.map(n=>n.dataset.inventoryLauncher)),['steam','riot']);
  assert.equal(await page.locator('[data-inventory-launcher="steam"]').getAttribute('aria-pressed'),'true');
  roblox=true;await page.locator('.page-heading [data-action="refresh"]').click();
  await page.locator('[data-inventory-launcher="roblox"]').waitFor();
  await page.locator('[data-inventory-launcher="roblox"]').click();await started;
  roblox=false;await page.locator('.page-heading [data-action="refresh"]').click();
  await page.locator('[data-inventory-launcher="roblox"]').waitFor({state:'detached'});
  releaseInventory();await page.waitForTimeout(100);
  assert.equal(await page.locator('[data-inventory-launcher="roblox"]').count(),0);
  assert.equal(await page.locator('[data-inventory-launcher="steam"]').getAttribute('aria-pressed'),'true');
  steam=false;await page.locator('.page-heading [data-action="refresh"]').click();
  await page.locator('[data-inventory-launcher="steam"]').waitFor({state:'detached'});
  assert.equal(await page.locator('[data-inventory-launcher="riot"]').getAttribute('aria-pressed'),'true');
  riot=false;await page.locator('.page-heading [data-action="refresh"]').click();
  await page.getByRole('heading',{name:'Подключи игровой аккаунт',exact:true}).waitFor();
  assert.equal(await page.locator('[data-inventory-launcher]').count(),0);assert.deepEqual(errors,[]);
  console.log('PASS: inventory platforms follow accounts, additions/removals, stale selection, delayed Roblox response and empty state.');
 }finally{await browser.close();await server.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
