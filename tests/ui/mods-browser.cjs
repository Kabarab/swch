const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
(async()=>{
  const resourcePaths=[],image=fs.readFileSync(path.resolve(__dirname,'../../ui/artwork/mod-platform.svg'));
  const assets={
    'web/style.css':['text/css','h1 { color: rgb(1, 2, 3); }'],
    'web/app.mjs':['text/javascript',`import {label} from './shared.mjs';await WebAssembly.instantiateStreaming(fetch(swch.assets.url('models/sample.wasm')));document.querySelector('h1').dataset.resources=label;document.getElementById('increment').onclick=async()=>{document.getElementById('value').textContent=await swch.call('action',{id:'count'});};`],
    'web/shared.mjs':['text/javascript',"export const label='loaded';"],
    'models/sample.wasm':['application/wasm',Buffer.from([0,97,115,109,1,0,0,0])],
    'images/icon.svg':['image/svg+xml',image],
  };
  const server=await staticServer((request,response,pathname)=>{
    if(!pathname.startsWith('/api/mods/author.launcher/assets/'))return false;
    const asset=pathname.split('/assets/')[1];resourcePaths.push(asset);
    const [contentType,body]=assets[asset]||['text/plain','missing'];
    response.writeHead(assets[asset]?200:404,{'Content-Type':contentType,'Access-Control-Allow-Origin':'null'});response.end(body);return true;
  }),browser=await chromium.launch(launchOptions);
  try{
    const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[],writes=[];
    page.setDefaultTimeout(15000);
    page.on('pageerror',e=>errors.push(e.message));page.on('dialog',d=>d.accept());
    await page.addInitScript(()=>{
      const originalFetch=window.fetch;
      window.modRequests=[];
      window.fetch=(url,options={})=>{
        const pathname=new URL(typeof url==='string'?url:url.url,location.href).pathname;
        if(/^\/api\/(?:mods|platforms)(?:\/|$)/.test(pathname))window.modRequests.push({pathname,hasSignal:!!options.signal,isFile:options.body instanceof File});
        return originalFetch(url,options);
      };
      File.prototype.arrayBuffer=()=>{throw new Error('The ZIP must be uploaded directly without a full memory copy');};
      if(window!==window.top){
        window.modTimerProbe=true;
        const originalTimeout=window.setTimeout;
        window.setTimeout=(fn,delay,...args)=>originalTimeout(fn,delay===35000?1:delay,...args);
      }
    });
    await page.route('https://covers.example.test/**',route=>route.request().url().endsWith('/broken.png')?route.abort():route.fulfill({contentType:'image/svg+xml',body:image}));
    await page.route('https://avatars.githubusercontent.com/**',route=>route.request().url().includes('/u/99?')?route.abort():route.fulfill({contentType:'image/svg+xml',body:image}));
    const platform='mod:author.launcher:demo';
    const metadata={id:platform,name:'Demo Launcher',mod_id:'author.launcher',capabilities:['library','sync','accounts','connect','disconnect','switch','launch']};
    const game={id:platform+':game:demo',launcher:platform,app_id:'demo',name:'SWCH Mod Demo',is_installed:true,size_bytes:0,owner_ids:[platform+':account:alice',platform+':account:bob']};
    const accounts=['alice','bob'].map(id=>({id:platform+':account:'+id,launcher:platform,username:id,display_name:id==='alice'?'Первый игрок':'Второй игрок',is_active:id==='alice'}));
    let enabled=true,count=0,settings={},theme={};let folderChoice='E:/Mod storage';
    const filePaths={data:'C:/SWCH/files/author.launcher/data',cache:'C:/SWCH/files/author.launcher/cache',temp:'C:/SWCH/files/author.launcher/temp',tools:'C:/SWCH/files/author.launcher/tools'};
    const fileUsage={data:1000,cache:1048576,temp:2000,tools:3000};
    const manifest={id:'author.launcher',name:'Демонстрационный лаунчер',version:'1.0.0',api:'^1.0.0',platforms:['windows-x64','macos-arm64'],description:'Библиотека и аккаунты',backend:'backend.mjs',ui:'web/index.html',theme:'theme.json',cover_url:'https://covers.example.test/cover.png'};
    await page.route('**/api/**',async route=>{
      const request=route.request(),url=new URL(request.url());let json=emptyApi(url.pathname);
      if(url.pathname.includes('/assets/'))return route.continue();
      if(url.pathname==='/api/mods/events'){await route.fulfill({contentType:'text/event-stream',body:': heartbeat\n\n'});return;}
      if(request.method()==='POST')writes.push([url.pathname,url.pathname==='/api/mods/install-file'?request.postDataBuffer():request.postDataJSON()]);
      if(url.pathname==='/api/mods/install-file'){
        assert.equal(url.searchParams.get('trusted'),'true');assert.equal(url.searchParams.has('sha256'),false);json={id:manifest.id};
      }
      if(url.pathname==='/api/mods')json={mods:[{manifest,source:{repository:'author/mod',owner_id:10},enabled,status:enabled?'active':'disabled',contributions:{launchers:[{...metadata,id:'demo'}],tabs:[{id:'main',name:'Демо-вкладка'}],actions:[{id:'count',name:'Увеличить счётчик'}]}}],theme};
      if(url.pathname==='/api/platforms/library')json={platforms:enabled?[metadata]:[],games:enabled?[game]:[],accounts:enabled?accounts:[]};
      if(url.pathname==='/api/mods/catalog')json={catalog:{schema:1,mods:[
        {manifest:{...manifest,id:'other.mod',name:'Мод из каталога',cover_url:null},author:'other',repository:'other/mod',owner_id:22},
        {manifest:{...manifest,id:'cover.mod',name:'Своя обложка'},author:'author',repository:'author/mod',owner_id:10},
        {manifest:{...manifest,id:'broken.mod',name:'Недоступная обложка',cover_url:'https://covers.example.test/broken.png'},author:'author',repository:'author/mod',owner_id:10},
        {manifest:{...manifest,id:'offline.mod',name:'Недоступный аватар',cover_url:null},author:'offline',repository:'offline/mod',owner_id:99}
      ],blocked:[]},target:'windows-x64'};
      if(url.pathname.endsWith('/toggle')){enabled=request.postDataJSON().enabled;json={status:'ok'};}
      if(url.pathname==='/api/mods/theme'){theme={bg:'#123456',accent:'#abcdef'};json=theme;}
      if(url.pathname.endsWith('/settings')){if(request.method()==='POST')settings=request.postDataJSON();json=settings;}
      if(url.pathname.endsWith('/logs'))json={logs:['activation log']};
      if(url.pathname.endsWith('/ui'))json={html:'<link rel="stylesheet" href="./style.css"><h1>SDK iframe</h1><img id="resource-image" src="../images/icon.svg"><button id="increment">Count</button><p id="value"></p><script type="module" src="./app.mjs"></script>'};
      if(url.pathname.endsWith('/sdk')){
        const {method,params={}}=request.postDataJSON();
        if(method==='files.paths')json=filePaths;
        else if(method==='files.usage')json=fileUsage;
        else if(method==='dialogs.openFolder')json=folderChoice;
        else if(method==='files.configure'){filePaths[params.kind]=params.directory+'/SWCH-Mods/author.launcher/'+params.kind;json=filePaths;}
        else if(method==='files.clearCache'){fileUsage.cache=0;json=null;}
        else if(method==='shell.openFolder')json=null;
        else{await new Promise(resolve=>setTimeout(resolve,50));count++;json=count;}
      }
      if(url.pathname.includes('/api/platforms/')&&request.method()==='POST'){json={message:'Готово'};if(url.pathname.endsWith('/switch'))for(const a of accounts)a.is_active=a.id===request.postDataJSON().account_id;}
      await route.fulfill({contentType:'application/json',body:JSON.stringify(json)});
    });
    await page.goto(server.origin);await page.waitForFunction(()=>document.querySelector('#connection-state')?.textContent==='Подключено');
    assert.equal(await page.locator('.game-card').count(),1);
    await page.locator('.card-play').click();await page.locator('#launch-account-dialog [data-account$=":bob"]').click();
    await page.waitForFunction(()=>document.querySelector('#toast')?.textContent==='Готово');assert.ok(writes.some(([url,body])=>url.endsWith('/launch')&&body.game_id===game.id&&body.account_id===accounts[1].id));
    await page.getByRole('button',{name:'Аккаунты',exact:true}).click();await page.getByRole('button',{name:'Сделать активным',exact:true}).click();
    await page.waitForTimeout(150);assert.ok(writes.some(([url])=>url.endsWith('/switch')));
    await page.getByRole('button',{name:'Моды',exact:true}).click();await page.getByRole('heading',{name:'Мод из каталога'}).waitFor();
    assert.equal(await page.locator('#mods-tab-catalog').getAttribute('aria-selected'),'true');
    assert.equal(await page.locator('#installed-mods-section').isVisible(),false);
    assert.equal(await page.locator('#mod-status-filter').count(),0);
    const card=name=>page.locator('#catalog-mods .mod-card').filter({has:page.getByRole('heading',{name,exact:true})}).locator('.mod-card-artwork');
    for(const img of await page.locator('#catalog-mods .mod-card-artwork').all()){
      await img.scrollIntoViewIfNeeded();
      await page.waitForFunction(image=>image.complete&&image.naturalWidth>0,await img.elementHandle());
    }
    assert.equal(await card('Мод из каталога').getAttribute('src'),'https://avatars.githubusercontent.com/u/22?s=256');
    assert.equal(await card('Своя обложка').getAttribute('src'),'https://covers.example.test/cover.png');
    assert.equal(await card('Своя обложка').evaluate(img=>getComputedStyle(img).objectFit),'cover');
    assert.equal(await card('Недоступная обложка').getAttribute('src'),'https://avatars.githubusercontent.com/u/10?s=256');
    assert.equal(await card('Недоступный аватар').getAttribute('src'),'/ui/artwork/mod-platform.svg');
    assert.equal(await card('Мод из каталога').evaluate(img=>getComputedStyle(img).objectFit),'contain');
    const tile=page.locator('#catalog-mods .mod-library-card').first(),tileWidth=await tile.evaluate(el=>el.getBoundingClientRect().width);
    assert.ok(tileWidth<350);
    const poster=await tile.locator('.mod-poster').boundingBox();assert.ok(Math.abs(poster.width/poster.height-2/3)<0.01);
    assert.ok(await page.locator('#catalog-mods').evaluate(el=>getComputedStyle(el).gridTemplateColumns.split(' ').length)>=3);
    await page.locator('#mod-search').fill('Своя обложка');assert.equal(await page.locator('#catalog-mods .mod-library-card').count(),1);
    assert.ok(Math.abs(await page.locator('#catalog-mods .mod-library-card').evaluate(el=>el.getBoundingClientRect().width)-tileWidth)<1);
    await page.locator('#mod-search').fill('');
    fs.mkdirSync(path.join(server.root,'output/mods'),{recursive:true});await page.screenshot({path:path.join(server.root,'output/mods/covers.png'),fullPage:true});
    await page.setViewportSize({width:390,height:844});
    assert.equal(await page.locator('#catalog-mods').evaluate(el=>getComputedStyle(el).gridTemplateColumns.split(' ').length),2);
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.screenshot({path:path.join(server.root,'output/mods/catalog-mobile.png'),fullPage:true});await page.setViewportSize({width:1440,height:1000});
    await page.getByRole('tab',{name:'Установленные',exact:true}).click();
    assert.equal(await page.locator('#catalog-mods-section').isVisible(),false);
    assert.equal(await page.locator('#installed-mods .mod-card-artwork').getAttribute('src'),manifest.cover_url);
    const zipBytes=Buffer.from('ZIP bytes are streamed unchanged');
    await page.locator('#mod-zip').setInputFiles({name:'large-mod.zip',mimeType:'application/zip',buffer:zipBytes});
    await page.waitForFunction(()=>document.querySelector('#toast')?.textContent==='Мод установлен');
    assert.deepEqual(writes.find(([url])=>url==='/api/mods/install-file')[1],zipBytes);
    await page.getByRole('button',{name:'Настройки',exact:true}).last().click();await page.locator('#mod-settings-json').fill('{"greeting":"hello"}');await page.getByRole('button',{name:'Сохранить',exact:true}).click();await page.locator('#mod-details').waitFor({state:'hidden'});assert.equal(settings.greeting,'hello');
    await page.getByRole('button',{name:'Файлы и место на диске',exact:true}).click();
    const fileDialog=page.locator('#mod-files'),cacheSection=()=>fileDialog.locator('.mod-file-location').filter({has:page.getByRole('heading',{name:'Кэш',exact:true})});
    await fileDialog.locator('.mod-file-location').first().waitFor();
    assert.equal(await fileDialog.locator('.mod-file-location').count(),4);
    assert.ok((await cacheSection().innerText()).includes(new Intl.NumberFormat('ru').format(1048576)));
    await cacheSection().getByRole('button',{name:'Выбрать папку',exact:true}).click();
    await cacheSection().locator('.mod-file-path').getByText('E:/Mod storage/SWCH-Mods/author.launcher/cache',{exact:true}).waitFor();
    assert.ok(writes.some(([url,body])=>url.endsWith('/sdk')&&body.method==='files.configure'&&body.params.kind==='cache'&&body.params.directory==='E:/Mod storage'));
    await cacheSection().getByRole('button',{name:'Открыть папку',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('#mod-files [data-mod-action="open-file-location"]:disabled')===null);
    assert.ok(writes.some(([url,body])=>url.endsWith('/sdk')&&body.method==='shell.openFolder'&&body.params.path===filePaths.cache));
    folderChoice=null;const configured=writes.filter(([,body])=>body?.method==='files.configure').length;
    await cacheSection().getByRole('button',{name:'Выбрать папку',exact:true}).click();
    await page.waitForTimeout(50);assert.equal(writes.filter(([,body])=>body?.method==='files.configure').length,configured);
    await fileDialog.getByRole('button',{name:'Очистить кэш',exact:true}).click();
    await cacheSection().getByText('0 байт',{exact:true}).waitFor();assert.equal(fileUsage.data,1000);
    await page.setViewportSize({width:390,height:844});assert.ok((await fileDialog.boundingBox()).width<=390);await page.screenshot({path:path.join(server.root,'output/mods/files-mobile.png'),fullPage:true});await page.setViewportSize({width:1440,height:1000});
    await fileDialog.getByRole('button',{name:'Закрыть',exact:true}).click();
    await page.getByRole('button',{name:'Применить тему'}).click();await page.waitForFunction(()=>document.documentElement.style.getPropertyValue('--bg')==='#123456');
    fs.mkdirSync(path.join(server.root,'output/mods'),{recursive:true});await page.screenshot({path:path.join(server.root,'output/mods/catalog.png'),fullPage:true});
    await page.setViewportSize({width:390,height:844});assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));await page.screenshot({path:path.join(server.root,'output/mods/mobile.png'),fullPage:true});await page.setViewportSize({width:1440,height:1000});
    await page.getByRole('button',{name:'Демо-вкладка',exact:true}).click();const frame=page.frameLocator('.mod-frame');await frame.locator('h1[data-resources="loaded"]').waitFor();await frame.getByRole('button',{name:'Count'}).click();await frame.locator('#value').getByText('1',{exact:true}).waitFor();
    assert.equal(await frame.locator('h1').evaluate(el=>getComputedStyle(el).color),'rgb(1, 2, 3)');
    assert.equal(await frame.locator('#resource-image').evaluate(image=>image.complete&&image.naturalWidth>0),true);
    assert.deepEqual(new Set(resourcePaths),new Set(['web/style.css','web/app.mjs','web/shared.mjs','models/sample.wasm','images/icon.svg']));
    assert.equal(await frame.locator('h1').evaluate(()=>{try{return !!parent.document.body;}catch{return false;}}),false,'mod iframe must not access the main DOM');
    assert.equal(await frame.locator('h1').evaluate(()=>{try{swch.assets.url('../outside');return false;}catch{return true;}}),true);
    assert.equal(await frame.locator('#value').evaluate(()=>window.modTimerProbe),true);
    const requests=await page.evaluate(()=>window.modRequests);
    assert.ok(requests.some(r=>r.pathname.endsWith('/sdk')));
    assert.ok(requests.some(r=>r.pathname==='/api/mods/install-file'&&r.isFile));
    assert.ok(requests.every(r=>!r.hasSignal),'mod API operations must have no default browser deadline');
    delete manifest.cover_url;
    await page.getByRole('button',{name:'Моды',exact:true}).click();await page.getByRole('button',{name:'Выключить',exact:true}).click();await page.getByRole('button',{name:'Включить',exact:true}).waitFor();await page.waitForFunction(()=>document.querySelectorAll('[data-mod-platform-nav]').length===0);assert.equal(await page.locator('[data-mod-platform-nav]').count(),0);
    assert.equal(await page.locator('#installed-mods .mod-card-artwork').getAttribute('src'),'https://avatars.githubusercontent.com/u/10?s=256');
    await page.getByRole('button',{name:'Файлы и место на диске',exact:true}).click();await fileDialog.locator('.mod-file-location').first().waitFor();assert.equal(await fileDialog.isVisible(),true);await fileDialog.getByRole('button',{name:'Закрыть',exact:true}).click();
    assert.deepEqual(errors,[]);console.log('Mods UI, accounts, theme, settings, ZIP streaming, resources, isolated iframe, disk selection and cache controls passed.');
  }finally{await browser.close();await server.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
