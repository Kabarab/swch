const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
(async()=>{
  const server=await staticServer(),browser=await chromium.launch(launchOptions);
  try{
    const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[],writes=[];
    page.on('pageerror',e=>errors.push(e.message));page.on('dialog',d=>d.accept());
    const platform='mod:author.launcher:demo';
    const metadata={id:platform,name:'Demo Launcher',mod_id:'author.launcher',capabilities:['library','sync','accounts','connect','disconnect','switch','launch']};
    const game={id:platform+':game:demo',launcher:platform,app_id:'demo',name:'SWCH Mod Demo',is_installed:true,size_bytes:0,owner_ids:[platform+':account:alice',platform+':account:bob']};
    const accounts=['alice','bob'].map(id=>({id:platform+':account:'+id,launcher:platform,username:id,display_name:id==='alice'?'Первый игрок':'Второй игрок',is_active:id==='alice'}));
    let enabled=true,count=0,settings={},theme={};
    const manifest={id:'author.launcher',name:'Демонстрационный лаунчер',version:'1.0.0',api:'^1.0.0',platforms:['windows-x64','macos-arm64'],description:'Библиотека и аккаунты',backend:'backend.mjs',ui:'index.html',theme:'theme.json'};
    await page.route('**/api/**',async route=>{
      const request=route.request(),url=new URL(request.url());let json=emptyApi(url.pathname);
      if(url.pathname==='/api/mods/events'){await route.fulfill({contentType:'text/event-stream',body:': heartbeat\n\n'});return;}
      if(request.method()==='POST')writes.push([url.pathname,request.postDataJSON()]);
      if(url.pathname==='/api/mods')json={mods:[{manifest,enabled,status:enabled?'active':'disabled',contributions:{launchers:[{...metadata,id:'demo'}],tabs:[{id:'main',name:'Демо-вкладка'}],actions:[{id:'count',name:'Увеличить счётчик'}]}}],theme};
      if(url.pathname==='/api/platforms/library')json={platforms:enabled?[metadata]:[],games:enabled?[game]:[],accounts:enabled?accounts:[]};
      if(url.pathname==='/api/mods/catalog')json={catalog:{schema:1,mods:[{manifest:{...manifest,id:'other.mod',name:'Мод из каталога'},author:'other',repository:'other/mod'}],blocked:[]},target:'windows-x64'};
      if(url.pathname.endsWith('/toggle')){enabled=request.postDataJSON().enabled;json={status:'ok'};}
      if(url.pathname==='/api/mods/theme'){theme={bg:'#123456',accent:'#abcdef'};json=theme;}
      if(url.pathname.endsWith('/settings')){if(request.method()==='POST')settings=request.postDataJSON();json=settings;}
      if(url.pathname.endsWith('/logs'))json={logs:['activation log']};
      if(url.pathname.endsWith('/ui'))json={html:'<h1>SDK iframe</h1><button id="increment">Count</button><p id="value"></p><script>document.getElementById("increment").onclick=async()=>{document.getElementById("value").textContent=await swch.call("action",{id:"count"});};</script>'};
      if(url.pathname.endsWith('/sdk')){count++;json=count;}
      if(url.pathname.includes('/api/platforms/')&&request.method()==='POST'){json={message:'Готово'};if(url.pathname.endsWith('/switch'))for(const a of accounts)a.is_active=a.id===request.postDataJSON().account_id;}
      await route.fulfill({json});
    });
    await page.goto(server.origin);await page.waitForFunction(()=>document.querySelector('#connection-state')?.textContent==='Подключено');
    assert.equal(await page.locator('.game-card').count(),1);
    await page.locator('.card-play').click();await page.locator('#launch-account-dialog [data-account$=":bob"]').click();
    await page.waitForFunction(()=>document.querySelector('#toast')?.textContent==='Готово');assert.ok(writes.some(([url,body])=>url.endsWith('/launch')&&body.game_id===game.id&&body.account_id===accounts[1].id));
    await page.getByRole('button',{name:'Аккаунты',exact:true}).click();await page.getByRole('button',{name:'Сделать активным',exact:true}).click();
    await page.waitForTimeout(150);assert.ok(writes.some(([url])=>url.endsWith('/switch')));
    await page.getByRole('button',{name:'Моды',exact:true}).click();await page.getByRole('heading',{name:'Мод из каталога'}).waitFor();
    await page.getByRole('button',{name:'Настройки',exact:true}).last().click();await page.locator('#mod-settings-json').fill('{"greeting":"hello"}');await page.getByRole('button',{name:'Сохранить',exact:true}).click();await page.locator('#mod-details').waitFor({state:'hidden'});assert.equal(settings.greeting,'hello');
    await page.getByRole('button',{name:'Применить тему'}).click();await page.waitForFunction(()=>document.documentElement.style.getPropertyValue('--bg')==='#123456');
    fs.mkdirSync(path.join(server.root,'output/mods'),{recursive:true});await page.screenshot({path:path.join(server.root,'output/mods/catalog.png'),fullPage:true});
    await page.setViewportSize({width:390,height:844});assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));await page.screenshot({path:path.join(server.root,'output/mods/mobile.png'),fullPage:true});await page.setViewportSize({width:1440,height:1000});
    await page.getByRole('button',{name:'Демо-вкладка',exact:true}).click();const frame=page.frameLocator('.mod-frame');await frame.getByRole('heading',{name:'SDK iframe'}).waitFor();await frame.getByRole('button',{name:'Count'}).click();await frame.locator('#value').getByText('1',{exact:true}).waitFor();
    await page.getByRole('button',{name:'Моды',exact:true}).click();await page.getByRole('button',{name:'Выключить',exact:true}).click();await page.getByRole('button',{name:'Включить',exact:true}).waitFor();await page.waitForFunction(()=>document.querySelectorAll('[data-mod-platform-nav]').length===0);assert.equal(await page.locator('[data-mod-platform-nav]').count(),0);
    assert.deepEqual(errors,[]);console.log('Mods UI, launch, accounts, theme, settings and iframe SDK passed.');
  }finally{await browser.close();await server.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
