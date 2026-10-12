const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
const assert=require('node:assert/strict'),path=require('node:path'),fs=require('node:fs');
(async()=>{
 const server=await staticServer(),browser=await chromium.launch(launchOptions);
 try{
  const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[],writes=[];
  const one='76561197960265729',two='76561197960265730';
  page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/api/**',async route=>{
   const url=new URL(route.request().url());let json=emptyApi(url.pathname);if(route.request().method()!=='GET')writes.push(url.pathname);
   if(url.pathname==='/api/steam/library')json={games:[one,two].map(account_id=>({id:'steam_deadlock',app_id:'1422450',name:'Deadlock',launcher:'steam',is_installed:true,is_free:true,account_id,access:'owned',playtime_minutes:100})),accounts:[{id:one,launcher:'steam',display_name:'First',is_active:true},{id:two,launcher:'steam',display_name:'Second'}],warnings:[]};
   if(url.pathname==='/api/steam/game-stats'){
    const account_id=url.searchParams.get('account_id');
    json={account_id,app_id:'1422450',status:'available',source:'Deadlock API',metrics:[{label:'Победы',value:99,unit:''}],notes:[],deadlock:{requirements:{season:'Current',total_wins:60,hero_wins:15,heroes:3},standard_wins:account_id===one?80:null,heroes:account_id===one?[{hero_id:1,standard_wins:30,ranked_matches:4,last_rank:75,metrics:[{label:'Победы',value:22,unit:''},{label:'K/D',value:2,unit:''}]},{hero_id:2,standard_wins:20,ranked_matches:0,metrics:[{label:'Победы',value:7,unit:''}]}]:[],ranked_played:account_id===one}};
   }
   await route.fulfill({json});
  });
  await page.goto(server.origin);await page.getByRole('button',{name:'Открыть Deadlock',exact:true}).click();
  const panel=page.locator('#game-modal [data-game-stat-panel]');
  await panel.locator('[data-deadlock-status]').filter({hasText:'Есть рейтинговые матчи'}).waitFor();
  assert.match(await panel.innerText(),/Infernus/);
  await panel.locator('[data-deadlock-hero-select]').selectOption('1');
  assert.match(await panel.locator('.deadlock-selected-hero').innerText(),/Infernus.*Открыт по рейтинговым матчам.*Emissary V/s);
  assert.match(await panel.locator('.game-stat-grid').innerText(),/22/);assert.doesNotMatch(await panel.locator('.game-stat-grid').innerText(),/99/);
  await panel.locator('[data-deadlock-hero-select]').selectOption('2');assert.match(await panel.locator('.game-stat-grid').innerText(),/7/);
  assert.match(await panel.locator('[data-deadlock-hero-access]').innerText(),/Требование по победам выполнено/);
  await page.locator('#game-modal [data-action="select-account"][data-account="'+two+'"]').click();
  await panel.locator('[data-deadlock-status]').filter({hasText:'Доступ не подтверждён'}).waitFor();
  assert.equal(await panel.locator('[data-deadlock-hero-select]').inputValue(),'all');
  await panel.locator('[data-deadlock-hero-select]').selectOption('1');assert.match(await panel.innerText(),/Нет статистики этого персонажа/);
  await page.locator('#game-modal [data-action="select-account"][data-account="'+one+'"]').click();
  assert.equal(await panel.locator('[data-deadlock-hero-select]').inputValue(),'2');
  const output=path.join(server.root,'output/deadlock-stats');await fs.promises.mkdir(output,{recursive:true});
  await panel.locator('.deadlock-selected-hero').scrollIntoViewIfNeeded();
  await page.screenshot({path:path.join(output,'hero-desktop.png'),fullPage:true});
  for(const width of [1440,1000,800,650,390]){await page.setViewportSize({width,height:1000});assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Overflow '+width);}
  await panel.locator('.deadlock-selected-hero').scrollIntoViewIfNeeded();
  await page.screenshot({path:path.join(output,'hero-mobile.png'),fullPage:true});
  assert.deepEqual(errors,[]);assert.equal(writes.filter(url=>/switch|\/launch|sessions\/save/.test(url)).length,0);
  console.log('PASS: Deadlock ranked progress, specific heroes, rank names, unknown data, account selection, no session writes and five widths.');
 }finally{await browser.close();await server.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
