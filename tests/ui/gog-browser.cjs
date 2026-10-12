const {emptyApi,launchOptions}=require('./harness.cjs');
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
const http=require('node:http');
const fs=require('node:fs');
const path=require('node:path');

(async()=>{
 const root=path.resolve(__dirname,'../..');
 const server=http.createServer((req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
  const file=path.join(root,pathname==='/'?'index.html':pathname);
  if(!file.startsWith(root+path.sep)){res.writeHead(403);res.end();return;}
  fs.readFile(file,(error,data)=>{
   if(error){res.writeHead(404);res.end();return;}
   const mime={'.html':'text/html','.js':'text/javascript','.mjs':'text/javascript','.css':'text/css'}[path.extname(file)]||'application/octet-stream';
   res.writeHead(200,{'Content-Type':mime});res.end(data);
  });
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 const browser=await chromium.launch(launchOptions);
 try{
  const page=await browser.newPage({viewport:{width:1280,height:900}});
  const errors=[],writes=[];page.on('pageerror',e=>errors.push(e.message));
  const accounts=[{id:'gog_1',launcher:'gog',username:'GOG Main',is_active:true,launcher_active:true},{id:'gog_2',launcher:'gog',username:'GOG Second',is_active:false,launcher_active:false}];
  const games=[{id:'gog_101',app_id:'101',launcher:'gog',name:'Shared GOG Game',is_installed:true,owner_ids:['gog_1','gog_2']},
   {id:'gog_102',app_id:'102',launcher:'gog',name:'Only Main Game',is_installed:false,owner_ids:['gog_1']},
   {id:'gog_103',app_id:'103',launcher:'gog',name:'Local GOG Game',is_installed:true,owner_ids:[],local_only:true}];
  let failLogin=false,clientInstalled=true,moduleStatus='not_detected';
  await page.route('**/api/**',async route=>{
   const req=route.request(),url=new URL(req.url());let json=emptyApi(url.pathname);
   if(req.method()==='POST'){
    const body=req.postDataJSON();writes.push([url.pathname,body]);json={message:'GOG action completed',warnings:[]};
    if(url.pathname==='/api/gog/switch-account')accounts.forEach(a=>{a.is_active=a.id===body.account_id;a.launcher_active=a.is_active;});
    if(url.pathname==='/api/gog/accounts/remove'){
     accounts.splice(accounts.findIndex(a=>a.id===body.account_id),1);
     games.forEach(g=>g.owner_ids=g.owner_ids.filter(id=>id!==body.account_id));
    }
    if(url.pathname==='/api/gog/auth/login'&&failLogin){await route.fulfill({status:401,json:{error:'Вход GOG истёк'}});return;}
   }else if(url.pathname==='/api/gog/library')json={games,accounts,client_installed:clientInstalled,launch_supported:true,message:'Библиотеки GOG; вход в GALAXY выполняется в клиенте.'};
   else if(url.pathname==='/api/steam/library')json={games:[],accounts:[],warnings:[]};
   else if(url.pathname==='/api/epic/accounts')json={accounts:[]};
   else if(['/api/riot/library','/api/roblox/library','/api/minecraft/library','/api/ubisoft/library'].includes(url.pathname))json={games:[],accounts:[]};
   else if(url.pathname==='/api/riot/accounts')json={accounts:[],profiles:[]};
   else if(url.pathname==='/api/riot/sessions')json={sessions:[]};
   else if(url.pathname==='/api/roblox/accounts')json=[];
   else if(url.pathname==='/api/accounts/restrictions')json={restrictions:[]};
   else if(url.pathname==='/api/modules')json={modules:[{id:'gog',name:'GOG GALAXY',active:moduleStatus==='active',status:moduleStatus,launcher_detected:moduleStatus==='active',module_installed:moduleStatus==='active',ram_bytes:0}]};
   await route.fulfill({json});
  });
  const ready=()=>page.waitForFunction(()=>document.querySelector('[data-action="refresh"]').disabled===false);
  const updated=async click=>{const refreshed=page.waitForResponse(r=>new URL(r.url()).pathname==='/api/gog/library'&&r.request().method()==='GET');await click();await refreshed;await ready();};
  await page.goto(`http://127.0.0.1:${server.address().port}/`);await ready();
  await page.waitForFunction(()=>document.querySelector('#modules-list')?.textContent.includes('Онлайн-библиотека доступна'));
  assert.equal(await page.locator('.platform-item[data-platform="gog"]').isVisible(),true);
  await page.locator('.platform-item[data-platform="gog"]').click();
  await page.locator('#library-advanced > summary').click();
  assert.equal(await page.locator('#gog-count').textContent(),'3');
  assert.equal(await page.locator('#steam-filters').isVisible(),false);
  assert.equal(await page.locator('#gog-filters').isVisible(),true);
  await page.selectOption('#gog-account-filter','gog_2');
  assert.equal(await page.locator('.game-card').count(),1);
  await updated(()=>page.locator('#gog-filters [data-action="sync-gog"]').click());
  assert.deepEqual(writes.at(-1),['/api/gog/library/sync',{account_id:'gog_2'}]);
  await page.selectOption('#gog-account-filter','all');
  await updated(()=>page.locator('.game-card').filter({hasText:'Local GOG Game'}).locator('[data-action="quick-launch"]').click());
  assert.deepEqual(writes.at(-1),['/api/gog/launch',{app_id:'103',account_id:null}]);
  clientInstalled=false;
  await updated(()=>page.locator('#gog-filters [data-action="sync-gog"]').click());
  assert.equal(await page.locator('.game-card').filter({hasText:'Local GOG Game'}).locator('[data-action="quick-launch"]').isDisabled(),true);
  await page.locator('[data-page="accounts"]').click();
  await updated(()=>page.locator('.gog-account').filter({hasText:'GOG Second'}).locator('[data-action="switch"]').click());
  assert.equal(writes.at(-1)[0],'/api/gog/switch-account');
  await page.locator('.gog-account').filter({hasText:'GOG Second'}).locator('.is-active').waitFor();
  assert.equal(await page.locator('.gog-account').filter({hasText:'GOG Second'}).locator('.is-active').count(),1);
  const main=page.locator('.gog-account').filter({hasText:'GOG Main'});
  await main.locator('summary').click();await updated(()=>main.locator('[data-action="sync-gog"]').click());
  assert.deepEqual(writes.at(-1),['/api/gog/library/sync',{account_id:'gog_1'}]);
  await page.locator('#accounts-view [data-platform="gog"]').click();
  await page.locator('#accounts-view [data-action="connect"]').click();
  assert.equal(await page.locator('[data-pane="gog"]').isVisible(),true);
  assert.equal(await page.locator('#gog-tab-badge').textContent(),'2');
  await page.locator('[data-action="gog-auth"]').click();assert.equal(writes.at(-1)[0],'/api/gog/auth/open');
  failLogin=true;await page.locator('#gog-code').fill('test-code');
  await page.locator('#gog-login-form button[type="submit"]').click();await page.locator('#gog-login-error').filter({hasText:'истёк'}).waitFor();await ready();
  assert.equal(await page.locator('#gog-code').inputValue(),'test-code');
  failLogin=false;await updated(()=>page.locator('#gog-login-form button[type="submit"]').click());
  assert.equal(await page.locator('#gog-code').inputValue(),'');assert.equal(await page.locator('#connect-dialog').isVisible(),false);
  const mainAfter=page.locator('.gog-account').filter({hasText:'GOG Main'});await mainAfter.locator('summary').click();
  await updated(()=>mainAfter.locator('[data-action="remove-gog"]').click());
  assert.equal(writes.at(-1)[0],'/api/gog/accounts/remove');assert.equal(await page.locator('.gog-account').count(),1);
  assert.equal(writes.some(([url])=>url.startsWith('/api/epic/')),false);
  assert.deepEqual(errors,[]);
  await page.locator('[data-page="library"]').click();
  await page.locator('.platform-item[data-platform="gog"]').click();
  await page.locator('#library-advanced > summary').click();
  fs.mkdirSync(path.join(root,'target_build/ui'),{recursive:true});
  await page.screenshot({path:path.join(root,'target_build/ui/gog-library.png'),fullPage:true});
  console.log('GOG browser: account filtering, local launch, missing client, sync, selection, login retry and removal passed.');
 }finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
})().catch(error=>{console.error(error);process.exitCode=1;});
