const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
const assert=require('node:assert/strict'),path=require('node:path'),fs=require('node:fs');
(async()=>{
 const {VALORANT_WEAPONS}=await import('../../ui/valorant-weapons.mjs');
 const vandal=VALORANT_WEAPONS.find(w=>w.name==='Vandal'),ghost=VALORANT_WEAPONS.find(w=>w.name==='Ghost');
 const server=await staticServer(),browser=await chromium.launch(launchOptions);
 try{
  const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[],writes=[];
  const section=value=>({value,error:null}),image_url='/ui/artwork/valorant-keyart.jpg';
  await page.route('**/api/**',async route=>{
   const url=new URL(route.request().url());let json=emptyApi(url.pathname);if(route.request().method()!=='GET')writes.push(url.pathname);
   if(url.pathname==='/api/riot/accounts')json={accounts:[{id:'one',display_name:'First#TAG',is_active:true},{id:'two',display_name:'Second#TAG'},{id:'missing',display_name:'Missing#TAG'}],profiles:[
    {account_id:'one',app_id:'valorant',display_name:'First#TAG',skins:section([{id:vandal.skins[0],name:'Vandal skin',image_url}]),buddies:section([{id:'buddy',name:'Buddy',image_url}]),champions:section([{id:'jett',name:'Jett',image_url}])},
    {account_id:'two',app_id:'valorant',display_name:'Second#TAG',skins:section([{id:ghost.skins[0],name:'Ghost skin',image_url}]),buddies:{value:null,error:'Offline'},champions:section([{id:'jett',name:'Jett',image_url},{id:'sage',name:'Sage',image_url}])}
   ]};
   await route.fulfill({json});
  });
  await page.goto(server.origin);await page.waitForFunction(()=>!document.querySelector('.page-heading [data-action="refresh"]').disabled);
  await page.locator('[data-page="inventory"]').click();
  assert.equal(await page.locator('#riot-inventory-kind').inputValue(),'all');
  assert.equal(await page.locator('[data-riot-item]').count(),5);
  assert.deepEqual(await page.locator('[data-riot-section]').evaluateAll(nodes=>nodes.map(n=>n.dataset.riotSection)),['skins','buddies','champions']);
  for(const kind of ['skins','buddies','champions'])assert.equal(await page.locator(`[data-riot-section="${kind}"] [data-riot-kind]:not([data-riot-kind="${kind}"])`).count(),0);
  const palette=await page.locator('#riot-inventory-weapon').evaluate(select=>({scheme:getComputedStyle(select).colorScheme,color:getComputedStyle(select.options[0]).color,background:getComputedStyle(select.options[0]).backgroundColor}));
  assert.equal(palette.scheme,'dark');
  const brightness=color=>color.match(/\d+/g).slice(0,3).map(Number).reduce((a,b)=>a+b)/3;
  assert.ok(brightness(palette.color)>180&&brightness(palette.background)<100,'Options must have readable light text on a dark background');
  const jett=page.locator('[data-riot-item="jett"]');assert.match(await jett.innerText(),/First#TAG.*Second#TAG/s);
  assert.match(await page.locator('#riot-inventory-content').innerText(),/Брелоки · 2 акк\.|Брелоки.*Offline/s);
  await page.locator('[data-action="riot-weapon-category"][data-value="rifle"]').click();
  assert.equal(await page.locator('#riot-inventory-kind').inputValue(),'skins');
  assert.equal(await page.locator('[data-riot-item]').count(),1);assert.match(await page.locator('[data-riot-item]').innerText(),/Vandal/);
  assert.equal(await page.locator('#riot-inventory-weapon option').count(),2);
  await page.locator('#riot-inventory-weapon').selectOption(vandal.id);
  await page.locator('#riot-inventory-search').fill('Missing');assert.equal(await page.locator('[data-riot-item]').count(),0);
  await page.locator('#riot-inventory-search').fill('');
  await page.locator('#riot-inventory-kind').selectOption('buddies');assert.equal(await page.locator('[data-riot-item]').count(),1);
  assert.equal(await page.locator('#riot-inventory-weapon').count(),0);
  await page.locator('#riot-inventory-kind').selectOption('champions');assert.equal(await page.locator('[data-riot-item]').count(),2);
  await page.locator('#riot-inventory-account').selectOption('one');assert.equal(await page.locator('[data-riot-item]').count(),1);
  assert.doesNotMatch(await page.locator('#riot-inventory-content').innerText(),/Second#TAG/);
  await page.locator('#riot-inventory-account').selectOption('missing');assert.match(await page.locator('#riot-inventory-content').innerText(),/ещё не загружена/);
  await page.locator('#riot-inventory-account').selectOption('all');await page.locator('#riot-inventory-kind').selectOption('all');
  const output=path.join(server.root,'output/riot-inventory');await fs.promises.mkdir(output,{recursive:true});
  await page.screenshot({path:path.join(output,'all-desktop.png'),fullPage:true});
  for(const width of [1440,1000,800,650,390]){await page.setViewportSize({width,height:1000});assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Overflow '+width);}
  await page.screenshot({path:path.join(output,'all-mobile.png'),fullPage:true});
  assert.deepEqual(errors,[]);assert.equal(writes.filter(url=>/switch|\/launch|sessions\/save/.test(url)).length,0);
  console.log('PASS: all Riot items by default, shared heroes, weapon type/exact selection, search, partial data, account isolation and five widths.');
 }finally{await browser.close();await server.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
