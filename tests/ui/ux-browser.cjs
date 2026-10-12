const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict'),path=require('node:path'),fs=require('node:fs');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
(async()=>{
 const server=await staticServer(),browser=await chromium.launch(launchOptions);
 try{
  const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[],writes=[];
  page.on('pageerror',e=>errors.push(e.message));
  let removed=false;
  const account=id=>({id,launcher:id,username:id,display_name:id.toUpperCase(),is_active:true});
  const game=id=>({id:id+'_1',app_id:'1',launcher:id,name:id+' game',is_installed:true});
  const item={asset_id:'a',name:'AK-47',market_hash_name:'AK-47',amount:1,tradable:true,marketable:true,icon_url:'/ui/artwork/valorant-keyart.jpg',tags:['Rifle'],tag_details:[{category:'Type',value:'Rifle',name:'Rifle'}]};
  await page.route('**/api/**',async route=>{
   const req=route.request(),url=new URL(req.url());let json=emptyApi(url.pathname);
   if(req.method()==='POST'){writes.push([url.pathname,req.postDataJSON()]);json={message:'Готово'};}
   if(url.pathname==='/api/steam/library')json={games:removed?[]:[{...game('steam'),app_id:'730',name:'Counter-Strike 2'}],accounts:[account('steam')],warnings:[]};
   if(url.pathname==='/api/epic/accounts')json={accounts:[{account_id:'epic',display_name:'Empty Epic'}],active_account_id:'epic'};
   if(url.pathname==='/api/riot/accounts')json={accounts:[account('riot')],profiles:[]};
   if(url.pathname==='/api/riot/library')json={games:[{...game('riot'),app_id:'valorant',name:'VALORANT'}],capabilities:{launch:true}};
   if(url.pathname==='/api/roblox/accounts')json=[{account_id:'roblox',display_name:'ROBLOX',is_active:true}];
   if(url.pathname==='/api/minecraft/library')json={games:[game('minecraft')],accounts:[account('minecraft')]};
   if(url.pathname==='/api/ubisoft/library')json={games:[],accounts:[account('ubisoft')]};
   if(url.pathname==='/api/gog/library')json={games:[game('gog')],accounts:[account('gog')],client_installed:true,launch_supported:true};
   if(url.pathname==='/api/hoyoverse/library')json={games:[],accounts:[account('hoyoverse')]};
   if(url.pathname==='/api/steam/inventory-cache')json={pages:[{account_id:'steam',app_id:'730',context_id:'2',page:{items:[item],total_count:1,cached:true,updated_at:'2026-10-11T10:00:00Z'}}],contexts:[{account_id:'steam',data:{contexts:[{app_id:'730',context_id:'2',name:'Counter-Strike 2'}]}}]};
   await route.fulfill({json});
  });
  await page.goto(server.origin);
  await page.locator('#accounts-count').filter({hasText:'8'}).waitFor();
  assert.equal(await page.locator('.sidebar .platform-item:visible').count(),5);
  for(const id of ['epic','roblox','ubisoft','hoyoverse'])assert.equal(await page.locator(`.sidebar [data-platform="${id}"]`).isVisible(),false);
  assert.equal(await page.locator('#library-advanced').getAttribute('open'),null);
  await page.locator('[data-page="accounts"]').click();
  assert.equal(await page.locator('.account-row').count(),8);
  assert.equal(await page.locator('.sidebar .platform-item:visible').count(),9);
  assert.equal(await page.locator('.account-management[open]').count(),0);
  const emptyEpic=page.locator('.epic-account');await emptyEpic.getByRole('button',{name:'Игры',exact:true}).click();
  assert.equal(await page.locator('#accounts-view').isVisible(),true);
  assert.match(await page.locator('#toast').innerText(),/пока нет игр/);
  await page.locator('.sidebar [data-platform="hoyoverse"]').click();
  assert.equal(await page.locator('.account-row').count(),1);
  assert.equal(await page.locator('#accounts-view').isVisible(),true);
  await page.locator('.sidebar [data-platform="all"]').click();
  await page.evaluate(()=>document.getElementById('toast').hidden=true);
  const output=path.join(server.root,'output/ux-redesign');await fs.promises.mkdir(output,{recursive:true});
  await page.screenshot({path:path.join(output,'accounts-desktop.png'),fullPage:true});
  await page.locator('[data-action="connect"]').filter({visible:true}).first().click();
  assert.equal(await page.locator('#connect-dialog .platform-logo').count(),8);
  await page.evaluate(()=>document.getElementById('connect-dialog').close());
  for(const width of [1440,1000,800,650,390]){
   await page.setViewportSize({width,height:1000});
   if(width<=900)assert.equal(await page.locator('#mobile-platform').isVisible(),true);
   assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Account overflow '+width);
   await page.locator('.account-row').first().locator('summary').first().click();
   assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Management overflow '+width);
   await page.locator('.account-row').first().locator('summary').first().click();
  }
  await page.screenshot({path:path.join(output,'accounts-mobile.png'),fullPage:true});
  await page.locator('#mobile-platform').selectOption('epic');assert.equal(await page.locator('.account-row').count(),1);
  await page.locator('[data-page="library"]').click();
  assert.equal(await page.locator('#mobile-platform option[value="epic"]').count(),0);
  await page.setViewportSize({width:1440,height:1000});
  await page.locator('.sidebar [data-platform="steam"]').click();removed=true;
  await page.locator('.page-heading [data-action="refresh"]').click();
  await page.waitForFunction(()=>document.querySelector('.sidebar [data-platform="steam"]').hidden);
  assert.equal(await page.locator('.sidebar [data-platform="all"]').getAttribute('aria-pressed'),'true');
  removed=false;await page.locator('.page-heading [data-action="refresh"]').click();
  await page.waitForFunction(()=>!document.querySelector('.sidebar [data-platform="steam"]').hidden);
  await page.screenshot({path:path.join(output,'library-desktop.png'),fullPage:true});
  await page.locator('[data-page="inventory"]').click();await page.locator('#inventory-app').selectOption('730');
  await page.locator('.cs-inventory-item').waitFor();
  assert.equal(await page.locator('.cs-advanced-filters').getAttribute('open'),null);
  assert.equal(await page.locator('.inventory-setting-box').isVisible(),false);
  await page.evaluate(()=>scrollTo(0,0));
  await page.screenshot({path:path.join(output,'inventory-desktop.png'),fullPage:true});
  assert.ok((await page.locator('.cs-inventory-item').boundingBox()).y<850,'Items should be visible in the first viewport');
  await page.screenshot({path:path.join(output,'inventory-desktop.png'),fullPage:true});
  for(const width of [1440,1000,800,650,390]){
   await page.setViewportSize({width,height:1000});
   assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Inventory overflow '+width);
   await page.locator('.cs-advanced-filters > summary').click();
   assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Expanded filters overflow '+width);
   await page.locator('.cs-advanced-filters > summary').click();
  }
  await page.screenshot({path:path.join(output,'inventory-mobile.png'),fullPage:true});
  assert.equal(writes.filter(([endpoint])=>/switch|\/launch|open-launcher|sessions\/save|accounts\/save/.test(endpoint)).length,0,'Browsing never changes a launcher session');
  assert.deepEqual(errors,[]);
  console.log('PASS: eight-platform directory, context navigation, empty account, update fallback, local logos, disclosures and five viewport widths.');
 }finally{await browser.close();await server.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
