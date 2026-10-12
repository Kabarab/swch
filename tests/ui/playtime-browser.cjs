const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
const assert=require('node:assert/strict'),path=require('node:path'),fs=require('node:fs');
(async()=>{
 const server=await staticServer(),browser=await chromium.launch(launchOptions);
 try{
  const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[],writes=[],timeQueries=[];
  page.on('pageerror',error=>errors.push(error.message));
  const section=value=>({value,error:null});
  let seconds=3600, nickname='Main';
  await page.route('**/api/**',async route=>{
   const request=route.request(),url=new URL(request.url());let json=emptyApi(url.pathname);
   if(request.method()!=='GET')writes.push(url.pathname);
   if(url.pathname==='/api/riot/library')json={games:[{id:'riot_valorant',launcher:'riot',app_id:'valorant',name:'VALORANT',is_installed:true,banner_url:'/ui/artwork/valorant-keyart.jpg'},{id:'riot_lol',launcher:'riot',app_id:'league_of_legends',name:'League of Legends',is_installed:false}],capabilities:{launch:true},client_installed:true};
   if(url.pathname==='/api/riot/accounts')json={accounts:[{id:'one',display_name:nickname,is_active:true},{id:'two',display_name:'Second'},{id:'unknown',display_name:'Unknown'}],profiles:[{account_id:'one',display_name:nickname,app_id:'valorant',has_play_history:true,playtime:section({seconds,matches:2}),skins:section([]),champions:section([])},{account_id:'two',app_id:'valorant',has_play_history:true,playtime:section({seconds:7200,matches:5}),skins:section([]),champions:section([])}]};
   if(url.pathname==='/api/epic/accounts')json={accounts:[{account_id:'epic',display_name:'Epic'}],active_account_id:'epic'};
   if(url.pathname==='/api/epic/library')json=[{app_name:'Fortnite',namespace:'fn',title:'Fortnite',is_installed:true},{app_name:'NoTime',title:'No Time',is_installed:false}];
   if(url.pathname==='/api/playtime'){timeQueries.push(url.searchParams.toString());json={records:url.searchParams.get('launcher')==='epic'?[{launcher:'epic',app_id:'Fortnite',account_id:'epic',seconds:18000,scope:'lifetime',source:'Epic Games'}]:[]};}
   await route.fulfill({json});
  });
  await page.goto(server.origin);await page.locator('.game-card').filter({hasText:'VALORANT'}).waitFor();
  const valorant=page.locator('.game-card').filter({hasText:'VALORANT'});
  assert.equal(await valorant.locator('.game-time').getAttribute('data-playtime-seconds'),'10800');
  assert.match(await valorant.locator('.game-time').innerText(),/3 ч.*по матчам/s);
  await page.locator('.game-card').filter({hasText:'Fortnite'}).locator('.game-time[data-playtime-seconds="18000"]').waitFor();
  assert.equal(await page.locator('.game-card').filter({hasText:'No Time'}).locator('.game-time').count(),0);
  assert.equal(await page.locator('.game-card').filter({hasText:'League of Legends'}).locator('.game-time').count(),0);
  const output=path.join(server.root,'output/playtime');await fs.promises.mkdir(output,{recursive:true});
  await page.screenshot({path:path.join(output,'cards.png')});
  await valorant.getByRole('button',{name:'Открыть VALORANT',exact:true}).click();
  const modal=page.locator('#game-modal');
  assert.equal(await modal.locator('.game-time').getAttribute('data-playtime-seconds'),'3600');
  assert.match(await modal.locator('.riot-playtime').innerText(),/1 ч/);
  await modal.locator('[data-action="select-riot-game-account"][data-account="two"]').click();
  assert.equal(await modal.locator('.game-time').getAttribute('data-playtime-seconds'),'7200');
  assert.match(await modal.locator('.riot-playtime').innerText(),/2 ч/);
  await modal.locator('[data-action="select-riot-game-account"][data-account="unknown"]').click();
  assert.equal(await modal.locator('.game-time').count(),0);
  assert.match(await modal.innerText(),/Профиль.*не загружен|Нет данных|не синхронизирован|Обнови профиль/s);
  await page.evaluate(()=>document.getElementById('game-modal').close());
  await page.locator('[data-layout="detail"]').click();
  assert.equal(await page.locator('.game-list [data-game="riot:valorant"] .game-time').getAttribute('data-playtime-seconds'),'10800');
  await page.locator('.game-list [data-game="riot:valorant"]').click();
  await page.locator('.detail-panel [data-action="select-riot-game-account"][data-account="one"]').click();
  for(const width of [1440,1000,800,650,390]){
   await page.setViewportSize({width,height:1000});assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Time layout overflow '+width);
  }
  await page.screenshot({path:path.join(output,'detail-mobile.png'),fullPage:true});
  await page.setViewportSize({width:1440,height:1000});
  seconds=5400;nickname='Renamed#TAG';await page.locator('.page-heading [data-action="refresh"]').click();
  await page.locator('.game-list [data-game="riot:valorant"] .game-time[data-playtime-seconds="12600"]').waitFor();
  assert.match(await page.locator('.detail-panel .account-chip[data-account="one"]').innerText(),/Renamed#TAG/);
  assert.match(await page.locator('.riot-profile').innerText(),/Renamed#TAG/);
  await page.locator('[data-page="accounts"]').click();
  assert.equal(await page.locator('.riot-account').filter({hasText:'Renamed#TAG'}).count(),1);
  assert.ok(timeQueries.some(query=>query.includes('launcher=epic')&&query.includes('refresh=true')));
  assert.equal(writes.filter(url=>/switch|\/launch|sessions\/save/.test(url)).length,0);
  assert.deepEqual(errors,[]);console.log('PASS: Riot/Epic time in cards, lists, modal and selected profiles; unknown time, refresh, scope and five widths.');
 }finally{await browser.close();await server.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
