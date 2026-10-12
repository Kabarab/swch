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
  const page=await browser.newPage({viewport:{width:1280,height:960}});
  const errors=[],writes=[];
  page.on('pageerror',e=>errors.push(e.message));
  const accounts=[{id:'ubisoft_a',launcher:'ubisoft',username:'a',display_name:'Ubisoft Main',is_active:true},{id:'ubisoft_b',launcher:'ubisoft',username:'b',display_name:'Ubisoft Second',is_active:false}];
  let automatic=null,automaticCounter=0,previousActive=null,failStatusOnce=false;
  const game={id:'ubisoft_635',launcher:'ubisoft',app_id:'635',name:'Rainbow Six Siege',is_installed:true,launch_uri:'uplay://launch/635/0'};
  await page.route('**/api/**',async route=>{
   const req=route.request(),url=new URL(req.url());
   let json=emptyApi(url.pathname);
   if(req.method()==='POST'){
    writes.push([url.pathname,req.postDataJSON()]);
    json={status:'success',message:'Ubisoft action completed'};
    if(url.pathname==='/api/ubisoft/switch-account'){
     accounts.forEach(a=>a.is_active=false);
     setTimeout(()=>accounts.forEach(a=>a.is_active=a.id===req.postDataJSON().account_id),700);
    }
    if(url.pathname==='/api/ubisoft/accounts/prepare'){
     previousActive=accounts.find(a=>a.is_active)?.id;
     accounts.forEach(a=>a.is_active=false);
     automatic={session_id:`automatic-${++automaticCounter}`,status:'waiting',account:null};
     json={...json,automatic_session_id:automatic.session_id};
    }
    if(url.pathname==='/api/ubisoft/auth/cancel'){
     automatic.status='cancelled';accounts.forEach(a=>a.is_active=a.id===previousActive);json=automatic;
    }
    if(url.pathname==='/api/ubisoft/accounts/remove')accounts.splice(accounts.findIndex(a=>a.id===req.postDataJSON().account_id),1);
   }else if(url.pathname==='/api/ubisoft/library')json={games:[game],accounts,client_installed:true,message:'Installed Ubisoft games'};
   else if(url.pathname==='/api/ubisoft/auth/status'){
    if(failStatusOnce){failStatusOnce=false;await route.fulfill({status:503,json:{error:'Temporary connection failure'}});return;}
    json=automatic;
   }
   else if(url.pathname==='/api/ubisoft/accounts')json={accounts};
   else if(url.pathname==='/api/ubisoft/current-account'){
    const active=accounts.find(a=>a.is_active);json={account_id:active?.id||null,display_name:active?'Auto Ubisoft':null};
   }
   else if(url.pathname==='/api/steam/library')json={games:[],accounts:[],warnings:[]};
   else if(url.pathname==='/api/epic/accounts')json={accounts:[]};
   else if(url.pathname==='/api/riot/library'||url.pathname==='/api/roblox/library'||url.pathname==='/api/minecraft/library')json={games:[],accounts:[]};
   else if(url.pathname==='/api/riot/accounts')json={accounts:[],profiles:[]};
   else if(url.pathname==='/api/riot/sessions')json={sessions:[]};
   else if(url.pathname==='/api/roblox/accounts')json=[];
   else if(url.pathname==='/api/accounts/restrictions')json={restrictions:[]};
   else if(url.pathname==='/api/modules')json={modules:[{id:'ubisoft',name:'Ubisoft Connect',active:true,status:'active',launcher_detected:true,module_installed:true,ram_bytes:0}]};
   await route.fulfill({json});
  });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.locator('.game-card').first().waitFor();
  await page.locator('.platform-item[data-platform="ubisoft"]').click();
  assert.equal(await page.locator('#ubisoft-count').textContent(),'1');
  await page.locator('[data-page="accounts"]').click();
  await page.locator('.ubisoft-account').first().waitFor();
  assert.equal(await page.locator('.ubisoft-account').count(),2);
  assert.equal(await page.locator('.ubisoft-account').filter({hasText:'Ubisoft Main'}).locator('.account-row-actions button').last().textContent(),'Открыть в лаунчере');
  // A login performed directly in Ubisoft updates cards without manual refresh.
  accounts.forEach(a=>a.is_active=a.id==='ubisoft_b');
  await page.locator('.ubisoft-account').filter({hasText:'Ubisoft Second'}).locator('.is-active').waitFor();
  // The server can discover the nickname after the account has already appeared.
  accounts[1].display_name='Discovered Ubisoft';accounts[1].username='Discovered Ubisoft';
  await page.locator('.ubisoft-account').filter({hasText:'Discovered Ubisoft'}).waitFor();
  assert.equal(await page.locator('.ubisoft-account').filter({hasText:'Ubisoft Second'}).count(),0);
  accounts[1].display_name='Ubisoft Second';accounts[1].username='b';
  await page.locator('.ubisoft-account').filter({hasText:'Ubisoft Second'}).waitFor();
  assert.equal(await page.locator('.ubisoft-account').filter({hasText:'Ubisoft Second'}).locator('.account-row-actions button').last().textContent(),'Открыть в лаунчере');
  assert.equal(await page.locator('.ubisoft-account').filter({hasText:'Ubisoft Main'}).locator('.account-row-actions button').last().textContent(),'Сделать активным');
  await page.locator('[data-action="switch"][data-account="ubisoft_a"]').click();
  await page.waitForFunction(()=>document.querySelector('[data-action="refresh"]').disabled===false);
  assert.equal(writes.at(-1)[0],'/api/ubisoft/switch-account');
  await page.locator('.ubisoft-account').filter({hasText:'Ubisoft Main'}).locator('.is-active').waitFor();
  await page.locator('.ubisoft-account').filter({hasText:'Ubisoft Main'}).locator('[data-action="open-launcher"]').click();
  await page.waitForFunction(()=>document.querySelector('[data-action="refresh"]').disabled===false);
  assert.equal(writes.at(-1)[0],'/api/ubisoft/open-launcher');
  await page.locator('#accounts-view [data-platform="ubisoft"]').click();
  await page.locator('#accounts-view [data-action="connect"]').click();
  assert.equal(await page.locator('[data-pane="ubisoft"]').evaluate(el=>el.classList.contains('active')),true);
  await page.waitForFunction(()=>document.querySelector('#ubisoft-account-name').value==='Auto Ubisoft');
  await page.locator('[data-action="prepare-ubisoft"]').click();
  await page.locator('[data-action="cancel-ubisoft-login"]').waitFor({state:'visible'});
  assert.deepEqual(writes.at(-1),['/api/ubisoft/accounts/prepare',{automatic:true}]);
  assert.equal(await page.locator('#ubisoft-account-name').inputValue(),'');
  assert.equal(await page.locator('.ubisoft-account .is-active').count(),0);
  failStatusOnce=true;
  await page.waitForFunction(()=>document.querySelector('#ubisoft-session-error').textContent.includes('Повторяем проверку автоматически'));
  assert.equal(await page.locator('[data-action="prepare-ubisoft"]').isDisabled(),true);
  accounts.forEach(a=>a.is_active=a.id==='ubisoft_b');
  automatic.status='connected';automatic.account=accounts[1];
  await page.waitForFunction(()=>!document.querySelector('#connect-dialog').open);
  assert.equal(writes.some(([url])=>url==='/api/ubisoft/sessions/save'),false,'Automatic login needs no manual save');
  await page.locator('.ubisoft-account').filter({hasText:'Ubisoft Second'}).locator('.is-active').waitFor();
  await page.locator('#accounts-view [data-action="connect"]').click();
  await page.locator('[data-action="prepare-ubisoft"]').click();
  await page.locator('[data-action="cancel-ubisoft-login"]').waitFor({state:'visible'});
  await page.locator('[data-action="cancel-ubisoft-login"]').click();
  await page.waitForFunction(()=>document.querySelector('[data-action="prepare-ubisoft"]').disabled===false);
  assert.equal(writes.at(-1)[0],'/api/ubisoft/auth/cancel');
  assert.equal(accounts.find(a=>a.is_active)?.id,'ubisoft_b');
  await page.locator('[data-pane="ubisoft"] details > summary').click();
  await page.waitForFunction(()=>document.querySelector('#ubisoft-account-name').value==='Auto Ubisoft');
  await page.locator('#ubisoft-session-form button[type="submit"]').click();
  await page.waitForFunction(()=>document.querySelector('#ubisoft-session-form button').disabled===false);
  assert.deepEqual(writes.at(-1),['/api/ubisoft/sessions/save',{display_name:null}]);
  await page.locator('#ubisoft-account-name').fill('My Ubisoft');
  await page.locator('#ubisoft-session-form button[type="submit"]').click();
  await page.waitForFunction(()=>document.querySelector('#ubisoft-session-form button').disabled===false);
  assert.deepEqual(writes.at(-1),['/api/ubisoft/sessions/save',{display_name:'My Ubisoft'}]);
  await page.screenshot({path:path.join(root,'output/ubisoft-connect.png')});
  await page.locator('#connect-dialog [data-close]').click();
  await page.locator('[data-page="library"]').click();
  await page.locator('[data-action="open-game"][data-game="ubisoft:635"]').first().click();
  await page.locator('#game-modal [data-action="quick-launch"]').click();
  await page.locator('[data-action="launch-account"][data-account="ubisoft_b"]').click();
  await page.waitForFunction(()=>document.querySelector('[data-action="refresh"]').disabled===false);
  assert.deepEqual(writes.at(-1),['/api/ubisoft/launch',{app_id:'635',account_id:'ubisoft_b'}]);
  assert.equal(writes.some(([url])=>url.startsWith('/api/epic/')||url.startsWith('/api/steam/')),false);
  assert.deepEqual(errors,[]);
  console.log('PASS: one-click Ubisoft login adds an account automatically; cancellation and live activity work.');
 }finally{await browser.close();server.close();}
})().catch(error=>{console.error(error);process.exit(1);});
