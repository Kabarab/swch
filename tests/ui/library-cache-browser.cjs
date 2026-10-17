const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
(async()=>{
 const server=await staticServer(),browser=await chromium.launch(launchOptions);
 try {
  const context=await browser.newContext(),errors=[];
  // This cache regression deliberately suspends every API, including language
  // selection. Pin its assertions' locale independently of the system default.
  await context.addInitScript(()=>localStorage.setItem('swch-language','ru'));
  let mode='fresh',release;
  let gate=Promise.resolve();
  const steam={games:[{id:'steam_730',launcher:'steam',app_id:'730',name:'Cached Counter-Strike',is_installed:true,account_id:'alice',access:'owned'}],accounts:[{id:'alice',launcher:'steam',username:'Alice',display_name:'Alice',access_token:'do-not-cache'}],warnings:[]};
  const epicAccounts={accounts:[{account_id:'epic-alice',display_name:'Epic Alice'}],active_account_id:'epic-alice'};
  await context.route('**/api/**',async route=>{
   const path=new URL(route.request().url()).pathname;
   if(mode==='waiting')await gate;
   if(mode==='offline')return route.fulfill({status:503,json:{error:'offline'}});
   if(path==='/api/epic/library'&&mode==='epic-failure')return route.fulfill({status:503,json:{error:'offline'}});
   if(path==='/api/epic/accounts'&&mode!=='empty')return route.fulfill({json:epicAccounts});
   if(path==='/api/epic/library'&&mode!=='empty')return route.fulfill({json:[{id:'epic_game',app_name:'epic-game',title:'Cached Epic Game',namespace:'test',is_installed:false}]});
   return route.fulfill({json:path==='/api/steam/library'?(mode==='empty'?{games:[],accounts:[],warnings:[]}:steam):emptyApi(path)});
  });
  let page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
  await page.goto(server.origin);
  await page.waitForFunction(()=>localStorage.getItem('swch-library-cache-v1'));
  assert(!(await page.evaluate(()=>localStorage.getItem('swch-library-cache-v1'))).includes('do-not-cache'));
  // New page, same persistent storage: the card appears while every API is held.
  await page.close();mode='waiting';gate=new Promise(resolve=>release=resolve);
  page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
  await page.goto(server.origin,{waitUntil:'domcontentloaded'});
  await page.locator('#library-content [data-game="steam:730"]').first().waitFor({timeout:5000});
  assert.equal(await page.locator('[data-action="refresh"]').isDisabled(),true);
  assert.match(await page.locator('#footer-status').innerText(),/сохранено в кэше/);
  assert.equal(await page.locator('#accounts-count').innerText(),'2');
  // Failed refresh preserves the displayed cache and its timestamp.
  const saved=await page.evaluate(()=>localStorage.getItem('swch-library-cache-v1'));
  mode='offline';release();
  await page.waitForFunction(()=>!document.querySelector('[data-action="refresh"]').disabled);
  assert(await page.locator('#library-content').innerText().then(text=>text.includes('Cached Counter-Strike')));
  assert.equal(await page.evaluate(()=>localStorage.getItem('swch-library-cache-v1')),saved);
  // An individual Epic library failure must not erase that account's saved games.
  mode='epic-failure';await page.locator('[data-action="refresh"]').click();
  await page.waitForFunction(()=>!document.querySelector('[data-action="refresh"]').disabled);
  assert((await page.locator('#library-content').innerText()).includes('Cached Epic Game'));
  // A successful empty response removes cached games/accounts, including on reload.
  mode='empty';await page.locator('[data-action="refresh"]').click();
  await page.waitForFunction(()=>JSON.parse(localStorage.getItem('swch-library-cache-v1')).games.length===0);
  await page.reload();await page.waitForFunction(()=>!document.querySelector('[data-action="refresh"]').disabled);
  assert.equal(await page.locator('#library-content').innerText().then(text=>text.includes('Cached Counter-Strike')),false);
  // Demo mode cannot overwrite the real snapshot.
  const emptySaved=await page.evaluate(()=>localStorage.getItem('swch-library-cache-v1'));
  await page.goto(server.origin+'/?demo=1');
  await page.waitForFunction(()=>!document.querySelector('[data-action="refresh"]').disabled);
  assert.equal(await page.evaluate(()=>localStorage.getItem('swch-library-cache-v1')),emptySaved);
  assert.deepEqual(errors,[]);
  console.log('Library cache browser checks passed: immediate restore, offline, removal, demo isolation.');
 }finally{await browser.close();await server.close();}
})().catch(e=>{console.error(e);process.exit(1);});
