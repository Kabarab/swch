const {emptyApi,launchOptions}=require('./harness.cjs');
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),http=require('node:http');
(async()=>{
 const root=path.resolve(__dirname,'../..');
 const server=http.createServer((req,res)=>{
  const file=path.join(root,new URL(req.url,'http://localhost').pathname==='/'?'index.html':new URL(req.url,'http://localhost').pathname);
  res.setHeader('Content-Type',file.endsWith('.css')?'text/css':file.endsWith('.js')||file.endsWith('.mjs')?'text/javascript':'text/html');
  fs.createReadStream(file).on('error',()=>{res.statusCode=404;res.end();}).pipe(res);
 });
 await new Promise(r=>server.listen(0,'127.0.0.1',r));
 let browser;
 try{
  browser=await chromium.launch(launchOptions);
  const page=await browser.newPage({viewport:{width:1440,height:1100}});
  const errors=[],writes=[];
  page.on('pageerror',e=>errors.push(e.message));
  const sa='76561197960265729',sb='76561197960265730';
  const steam=[{id:sa,display_name:'Основной',username:'nazar',launcher:'steam',is_active:true},{id:sb,display_name:'Второй профиль',username:'second',launcher:'steam',is_active:false}];
  const section=value=>({value,error:null,updated_at:'2026-10-05T10:00:00Z'});
  const profiles=['one','two'].map((id,index)=>({account_id:'riot_'+id,app_id:'valorant',display_name:index?'Second#EU':'Main#EU',level:index?25:163,has_play_history:true,synced_at:'2026-10-05T10:00:00Z',skins:section([]),champions:section([]),playtime:section({seconds:3600,matches:5}),ranked:section({rank:index?'Gold 1':'Diamond 1',rating:index?10:42,eligibility:true})}));
  await page.route('**/api/**',async route=>{
   const req=route.request(),u=new URL(req.url());let json=emptyApi(u.pathname);
   if(req.method()==='POST'){writes.push([u.pathname,req.postDataJSON()]);json={status:'success',message:'Готово'};}
   else if(u.pathname==='/api/steam/library')json={accounts:steam,games:steam.map(a=>({id:'steam_570',app_id:'570',name:'Dota 2',launcher:'steam',is_installed:true,is_free:true,has_inventory:false,account_id:a.id,access:'owned',playtime_minutes:120})),warnings:[]};
   else if(u.pathname==='/api/epic/accounts')json={active_account_id:'epic_one',accounts:[{account_id:'epic_one',display_name:'Epic Main',username:'epic'}]};
   else if(u.pathname==='/api/epic/library')json=[{app_name:'Fortnite',title:'Fortnite',is_installed:true}];
   else if(u.pathname==='/api/accounts/restrictions')json={restrictions:[{account_id:sb,vac_banned:true,game_bans_count:1,game_bans:[]}]};
   else if(u.pathname==='/api/riot/accounts')json={profiles,accounts:[{id:'riot_one',username:'Main',display_name:'Main#EU',is_active:true},{id:'riot_two',username:'Second',display_name:'Second#EU',is_active:false}]};
   else if(u.pathname==='/api/riot/library')json={games:[{id:'riot_valorant',app_id:'valorant',name:'VALORANT',launcher:'riot',is_installed:true,banner_url:'/ui/artwork/valorant-keyart.jpg'}],client_installed:true,capabilities:{launch:true}};
   else if(u.pathname==='/api/roblox/library')json={games:[]};
   else if(u.pathname==='/api/roblox/accounts')json=[{account_id:'rb_one',display_name:'Builder',username:'builder',is_active:true,robux_balance:999,rap_value:1500}];
   else if(u.pathname==='/api/minecraft/library')json={games:[],accounts:[{id:'mc_one',username:'Explorer',is_active:true,account_type:'msa'}]};
   else if(u.pathname==='/api/steam/game-prices')json=[];
   else if(u.pathname==='/api/steam/game-stats')json={account_id:u.searchParams.get('account_id'),app_id:'570',status:'available',source:'OpenDota',metrics:[{label:'Победы',value:u.searchParams.get('account_id')===sa?123:45}],notes:[]};
   await route.fulfill({json});
  });
  await page.goto('http://127.0.0.1:'+server.address().port);
  await page.locator('#accounts-count').filter({hasText:'7'}).waitFor();
  await page.locator('[data-page="accounts"]').click();
  assert.equal(await page.locator('.account-row').count(),7);
  assert.equal(await page.locator('#accounts-view .metric').count(),0,'Game metrics must not be duplicated in the directory');
  assert.equal(await page.locator('.account-management[open]').count(),0);
  assert.ok((await page.locator('.account-row').first().boundingBox()).height<130,'Closed row must stay compact');
  assert.ok(await page.locator('.account-row').filter({hasText:'Второй профиль'}).locator('.account-warning').isVisible(),'Restriction summary must remain visible');
  await page.getByRole('searchbox',{name:'Найти аккаунт'}).fill('main');
  assert.equal(await page.locator('.account-row:visible').count(),2);
  await page.getByRole('searchbox',{name:'Найти аккаунт'}).fill('нет такого аккаунта');
  assert.ok(await page.locator('#accounts-no-results').isVisible());
  await page.getByRole('searchbox',{name:'Найти аккаунт'}).fill('');
  await page.locator('.account-platform-tabs [data-platform="riot"]').click();
  assert.equal(await page.locator('.account-row').count(),2);
  await page.locator('.account-platform-tabs [data-platform="all"]').click();
  await fs.promises.mkdir(path.join(root,'output/accounts-redesign'),{recursive:true});
  await page.screenshot({path:path.join(root,'output/accounts-redesign/accounts-desktop.png'),fullPage:true});
  for(const width of [1000,800,650,390]){
   await page.setViewportSize({width,height:1000});
   assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Directory overflow '+width);
   const management=page.locator('.steam-account .account-management').first();
   await management.locator('summary').first().click();
   assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Management overflow '+width);
   await management.locator('summary').first().click();
  }
  await page.screenshot({path:path.join(root,'output/accounts-redesign/accounts-mobile.png'),fullPage:true});
  await page.setViewportSize({width:1440,height:1100});
  await page.locator('.steam-account').filter({hasText:'Второй профиль'}).getByRole('button',{name:'Игры',exact:true}).click();
  await page.getByRole('button',{name:'Открыть Dota 2',exact:true}).click();
  const stats=page.locator('#game-modal [data-game-stat-panel]');
  await stats.getByText('45',{exact:true}).waitFor();
  assert.equal(await stats.getAttribute('data-account'),sb);
  await page.locator('#game-modal [data-action="select-account"][data-account="'+sa+'"]').click();
  await stats.getByText('123',{exact:true}).waitFor();
  assert.equal(writes.filter(([endpoint])=>/switch-account|open-launcher|\/launch|accounts\/save/.test(endpoint)).length,0,'Viewing statistics must never switch the actual launcher account');
  await page.evaluate(()=>document.getElementById('game-modal').close());
  await page.locator('[data-page="accounts"]').click();
  await page.locator('.riot-account').filter({hasText:'Second#EU'}).getByRole('button',{name:'Игры',exact:true}).click();
  assert.match(await page.locator('.detail-panel .riot-profile').innerText(),/Gold 1 · 10 RR/);
  await page.locator('[data-action="select-riot-game-account"][data-account="riot_one"]').click();
  assert.match(await page.locator('.detail-panel .riot-profile').innerText(),/Diamond 1 · 42 RR/);
  // Open the same Riot game as a card and ensure the selected account also updates the modal.
  await page.locator('[data-layout="cards"]').click();
  await page.getByRole('button',{name:'Открыть VALORANT',exact:true}).click();
  await page.locator('#game-modal [data-action="select-riot-game-account"][data-account="riot_two"]').click();
  assert.match(await page.locator('#game-modal .riot-profile').innerText(),/Gold 1 · 10 RR/);
  assert.ok(!(await page.locator('#game-modal .riot-profile').innerText()).includes('Diamond 1'));
  assert.equal(writes.filter(([endpoint])=>/switch-account|open-launcher|\/launch|accounts\/save/.test(endpoint)).length,0);
  await page.locator('#game-modal .riot-profile').scrollIntoViewIfNeeded();
  await page.screenshot({path:path.join(root,'output/accounts-redesign/game-statistics.png')});
  await page.evaluate(()=>document.getElementById('game-modal').close());
  await page.locator('[data-page="accounts"]').click();
  await Promise.all([
   page.waitForResponse(r=>r.url().endsWith('/api/riot/switch-account')&&r.request().method()==='POST'),
   page.locator('.riot-account').filter({hasText:'Second#EU'}).getByRole('button',{name:'Сделать активным',exact:true}).click()
  ]);
  const riotSwitches=writes.filter(([endpoint])=>endpoint==='/api/riot/switch-account');
  assert.equal(riotSwitches.length,1,'The directory button must switch Riot instead of displaying an informational placeholder');
  assert.equal(riotSwitches[0][1].account_id,'riot_two');
  assert.deepEqual(errors,[]);
  console.log('PASS: compact 5-platform directory, filters, responsive layout, selected statistics without launcher writes, and Riot account switch button.');
 }finally{if(browser)await browser.close();await new Promise(r=>server.close(r));}
})().catch(e=>{console.error(e);process.exitCode=1;});
