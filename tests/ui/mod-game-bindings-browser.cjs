const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
(async()=>{
  const server=await staticServer(),browser=await chromium.launch(launchOptions);
  try{
    const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[],writes=[];
    page.on('pageerror',error=>errors.push(error.message));
    const genshin={id:'hoyo_genshin',launcher:'hoyoverse',app_id:'genshin',name:'Genshin Impact',is_installed:true,size_bytes:0};
    const other={...genshin,id:'hoyo_other',app_id:'other',name:'Other Application'};
    const account={id:'hoyo-one',launcher:'hoyoverse',username:'one',display_name:'Player One',is_active:true,access_token:'do-not-forward'};
    const target={launcher:genshin.launcher,app_id:genshin.app_id,name:genshin.name};
    const manifest={id:'author.inventory',name:'Genshin Inventory',version:'1.0.0',api:'^1.0.0',platforms:['windows-x64','macos-arm64'],games:[target],ui:'index.html'};
    let bindings=null,enabled=true,failSave=false;
    const state=()=>({manifest,games:bindings??manifest.games,enabled,status:enabled?'active':'disabled',source:{downloads:55},contributions:{tabs:enabled?[{id:'main',name:'Inventory'}]:[],actions:enabled?[{id:'inspect',name:'Inspect context'}]:[]}});
    await page.route('**/api/**',async route=>{
      const request=route.request(),url=new URL(request.url());let json=emptyApi(url.pathname);
      if(url.pathname==='/api/mods/events')return route.fulfill({contentType:'text/event-stream',body:': heartbeat\n\n'});
      if(request.method()==='POST')writes.push([url.pathname,request.postDataJSON()]);
      if(url.pathname==='/api/hoyoverse/library')json={games:[genshin,other],accounts:[account]};
      if(url.pathname==='/api/mods')json={mods:[state(),{manifest:{...manifest,id:'author.general',name:'General Mod',games:[],ui:null,theme:'theme.json'},games:[],enabled:true,status:'active',contributions:{tabs:[]}}]};
      if(url.pathname==='/api/platforms/library')json={games:[],accounts:[],platforms:[]};
      if(url.pathname==='/api/mods/catalog')json={catalog:{mods:[
        {manifest:{...manifest,id:'author.remote',name:'Catalog Genshin'},author:'author',repository:'author/remote',downloads:1234,downloads_updated_at:'2026-10-15T10:00:00Z'},
        {manifest:{...manifest,id:'author.other',name:'Unrelated Catalog',games:[{launcher:'steam',app_id:'genshin'}],platforms:['macos-arm64']},author:'author',repository:'author/other',downloads:0}
      ]},target:'windows-x64',compatibility:{'author.other':false}};
      if(url.pathname==='/api/mods/author.inventory/games'){
        if(failSave)return route.fulfill({status:400,json:{error:'Cannot save bindings'}});
        bindings=request.postDataJSON().games;json={status:'ok'};
      }
      if(url.pathname.endsWith('/toggle')){enabled=request.postDataJSON().enabled;json={status:'ok'};}
      if(url.pathname.endsWith('/ui'))json={html:'<h1>Inventory view</h1><pre id="context"></pre><script>document.getElementById("context").textContent=JSON.stringify({game:swch.game,account:swch.account,tab:swch.tab});</script>'};
      if(url.pathname.endsWith('/sdk'))json={status:'ok'};
      return route.fulfill({json});
    });
    const sidebarMods=()=>page.locator('.sidebar [data-page="mods"]');
    const openGame=async name=>{
      await page.locator('.sidebar [data-page="library"]').click();
      await page.getByRole('button',{name:`Открыть ${name}`,exact:true}).click();
      await page.locator('#game-modal[open]').waitFor();
      await page.locator('#game-modal [data-mod-action="game-mods"]').click();
      await page.locator('#game-modal').waitFor({state:'hidden'});
      await page.locator('#mod-game-scope strong').getByText(name,{exact:true}).waitFor();
    };
    await page.goto(server.origin);await page.waitForFunction(()=>document.querySelectorAll('.game-card').length===2);
    await openGame(genshin.name);
    await page.locator('#catalog-mods').getByRole('heading',{name:'Catalog Genshin',exact:true}).waitFor();
    assert.equal(await page.locator('#installed-mods-section').isVisible(),false);
    await page.getByRole('tab',{name:'Установленные',exact:true}).click();
    await page.locator('#installed-mods').getByRole('heading',{name:manifest.name,exact:true}).waitFor();
    assert.equal(await page.locator('#installed-mods .mod-card').count(),1);
    assert.equal(await page.locator('#catalog-mods .mod-card').count(),1);
    assert.equal(await page.locator('#page-title').textContent(),'Моды.');
    assert.equal(await page.locator('#mod-game-filter').inputValue(),JSON.stringify(['hoyoverse','genshin']));
    assert.match(await page.locator('#catalog-mods .mod-downloads').innerText(),/1\D?234/);
    assert.equal(await page.locator('#installed-mods .mod-downloads').innerText(),'Скачивания: 55');
    await page.locator('#mod-game-filter').selectOption('general');
    assert.deepEqual(await page.locator('#installed-mods h3').allTextContents(),['General Mod']);
    assert.equal(await page.locator('#installed-mods .mod-downloads').innerText(),'Скачивания: нет данных');
    await page.getByRole('button',{name:'Сбросить фильтры',exact:true}).click();
    await page.getByRole('tab',{name:'Каталог',exact:true}).click();
    await page.locator('#mod-compatibility-filter').selectOption('incompatible');
    assert.deepEqual(await page.locator('#catalog-mods h3').allTextContents(),['Unrelated Catalog']);
    assert.equal(await page.locator('#catalog-mods .mod-downloads').innerText(),'Скачивания: 0');
    await page.locator('#mod-compatibility-filter').selectOption('all');await page.locator('#mod-sort').selectOption('downloads');
    assert.deepEqual(await page.locator('#catalog-mods h3').allTextContents(),['Catalog Genshin','Unrelated Catalog']);
    await page.getByRole('tab',{name:'Каталог',exact:true}).click();assert.equal(await page.locator('#installed-mods-section').isVisible(),false);
    await page.getByRole('tab',{name:'Установленные',exact:true}).click();await page.locator('#mod-kind-filter').selectOption('theme');
    assert.deepEqual(await page.locator('#installed-mods h3').allTextContents(),['General Mod']);assert.equal(await page.locator('#catalog-mods-section').isVisible(),false);
    await page.getByRole('button',{name:'Сбросить фильтры',exact:true}).click();
    await page.locator('#mod-search').fill('no such mod');assert.equal(await page.locator('.mods-grid .mod-card').count(),0);
    await page.getByRole('button',{name:'Сбросить фильтры',exact:true}).click();
    await page.getByRole('tab',{name:'Установленные',exact:true}).click();
    await page.locator('#mod-game-filter').selectOption(JSON.stringify(['hoyoverse','genshin']));
    await page.getByRole('button',{name:'Inspect context',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('#toast')?.textContent==='Готово');
    const action=writes.find(([url])=>url.endsWith('/sdk'))[1];
    assert.equal(action.params.input.game.app_id,'genshin');
    assert.equal(action.params.input.account.id,account.id);
    assert.equal(action.params.input.account.access_token,undefined);
    await page.getByRole('button',{name:'Открыть · Inventory',exact:true}).click();
    const context=async()=>JSON.parse(await page.frameLocator('.mod-frame').locator('#context').textContent());
    await page.frameLocator('.mod-frame').getByRole('heading',{name:'Inventory view'}).waitFor();
    assert.equal((await context()).game.app_id,'genshin');assert.equal((await context()).account.id,account.id);
    await page.getByRole('button',{name:'Моды этой игры',exact:true}).click();
    await page.getByRole('button',{name:'Вернуться к игре',exact:true}).click();
    await page.locator('#game-modal[open]').waitFor();await page.locator('#game-modal [data-mod-action="game-mods"]').click();
    await page.getByRole('button',{name:'Все моды',exact:true}).click();
    await page.locator('#installed-mods').getByRole('heading',{name:'General Mod',exact:true}).waitFor();
    await page.locator('[data-mod-action="bindings"][data-mod-id="author.inventory"]').click();
    const dialog=page.locator('#mod-bindings');
    await dialog.getByRole('checkbox',{name:/Genshin Impact/}).uncheck();
    await dialog.getByRole('checkbox',{name:/Other Application/}).check();
    failSave=true;await dialog.getByRole('button',{name:'Сохранить привязку',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('#toast')?.textContent==='Cannot save bindings');assert.equal(await dialog.isVisible(),true);
    failSave=false;await dialog.getByRole('button',{name:'Сохранить привязку',exact:true}).click();await dialog.waitFor({state:'hidden'});
    await page.reload();await page.waitForFunction(()=>document.querySelectorAll('.game-card').length===2);
    await openGame(genshin.name);assert.equal(await page.locator('#installed-mods .mod-card').count(),0);
    await openGame(other.name);await page.getByRole('tab',{name:'Установленные',exact:true}).click();await page.getByRole('button',{name:'Открыть · Inventory',exact:true}).waitFor();
    await page.getByRole('button',{name:'Открыть · Inventory',exact:true}).click();await page.frameLocator('.mod-frame').locator('#context').waitFor();
    assert.equal((await context()).game.app_id,'other');
    await page.locator('.sidebar [data-page="library"]').click();assert.equal(await page.locator('.mod-frame').count(),0);
    await openGame(other.name);await page.getByRole('button',{name:'Открыть · Inventory',exact:true}).click();await page.frameLocator('.mod-frame').locator('#context').waitFor();
    await sidebarMods().click();assert.equal(await page.locator('#page-title').textContent(),'Моды.');
    await page.getByRole('button',{name:'Открыть · Inventory',exact:true}).click();await page.frameLocator('.mod-frame').locator('#context').waitFor();
    assert.equal((await context()).game,null);assert.equal((await context()).account,null);
    await sidebarMods().click();await page.locator('[data-mod-action="bindings"][data-mod-id="author.inventory"]').click();
    await dialog.getByRole('button',{name:'Как указал автор',exact:true}).click();await dialog.waitFor({state:'hidden'});assert.equal(bindings,null);
    await openGame(genshin.name);await page.getByRole('button',{name:'Выключить',exact:true}).click();
    await page.getByRole('button',{name:'Включить',exact:true}).waitFor();assert.equal(await page.locator('[data-mod-action="open"]').count(),0);
    fs.mkdirSync(path.join(server.root,'output/mods'),{recursive:true});
    await page.screenshot({path:path.join(server.root,'output/mods/game-bindings.png'),fullPage:true});
    await page.setViewportSize({width:390,height:844});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
    await page.screenshot({path:path.join(server.root,'output/mods/game-bindings-mobile.png'),fullPage:true});
    assert.deepEqual(errors,[]);console.log('Game bindings, scoped catalog, account context, save errors, persistence, reset and disabled mods passed.');
  }finally{await browser.close();await server.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
