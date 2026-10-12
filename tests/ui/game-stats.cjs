const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const path=require('node:path');
const {emptyApi,launchOptions}=require('./harness.cjs');
(async()=>{
 const browser=await chromium.launch(launchOptions);
 try {
 const page=await browser.newPage({viewport:{width:1440,height:1100}}), errors=[], writes=[], reads=[];
 page.on('pageerror',e=>errors.push(e.message));
 const a='76561197960265729',b='76561197960265730';
 const accounts=[{id:a,username:'one',display_name:'Основной',launcher:'steam',is_active:true},{id:b,username:'two',display_name:'Второй',launcher:'steam',is_active:false}];
 const games=[['570','Dota 2'],['730','Counter-Strike 2'],['1422450','Deadlock']].flatMap(([app_id,name])=>accounts.map(acc=>({id:'steam_'+app_id,app_id,name,launcher:'steam',is_installed:true,is_free:true,has_inventory:app_id!=='1422450',account_id:acc.id,access:'owned',playtime_minutes:54320,owner_steamids:[acc.id]})));
 let releaseSlow; const slow=new Promise(r=>releaseSlow=r);
 await page.route('**/*',async route=>{
  const req=route.request(),u=new URL(req.url());
  if(u.origin!=='http://swch.test')return route.abort();
  if(!u.pathname.startsWith('/api/')){
   const file=u.pathname==='/'?'index.html':u.pathname.slice(1);
   if(file!=='index.html'&&!file.startsWith('ui/'))return route.fulfill({status:404,body:''});
   return route.fulfill({body:await fs.readFile(path.resolve(__dirname,'../..',file)),contentType:file.endsWith('css')?'text/css':file.endsWith('html')?'text/html':'text/javascript'});
  }
  if(req.method()==='POST')writes.push([u.pathname,req.postDataJSON()]);
  let json=emptyApi(u.pathname);
  if(u.pathname==='/api/steam/library')json={accounts,games,warnings:[]};
  if(u.pathname==='/api/epic/accounts')json={accounts:[]};
  if(u.pathname==='/api/steam/game-prices')json=[];
  if(u.pathname==='/api/steam/game-stats'){
   const account_id=u.searchParams.get('account_id'),app_id=u.searchParams.get('app_id');reads.push([account_id,app_id]);
   if(app_id==='570'&&account_id===a)await slow;
   json={account_id,app_id,status:'available',source:app_id==='570'?'OpenDota':app_id==='730'?'Steam Web API':'Deadlock API',fetched_at:'2026-09-21T12:00:00Z',notes:['История источника может быть неполной.'],metrics:[{label:'Победы',value:account_id===a?123:45,unit:''},{label:'Поражения',value:82,unit:''},{label:'Процент побед',value:60,unit:'%'},{label:'Матчи в источнике',value:205,unit:''},{label:'Ранг',value:'Легенда 4',unit:''},{label:'K/D',value:null,unit:''}]};
   if(app_id==='730'&&req.method()!=='POST')json={...json,status:'needs_key',metrics:[],notes:['Для статистики нужен ключ Steam Web API.']};
   if(app_id==='1422450')json={...json,stale:true,cached:true,notes:['Обновить не удалось. Показана сохранённая статистика.']};
  }
  return route.fulfill({json});
 });
 await page.goto('http://swch.test/');
 await page.getByRole('button',{name:'Открыть Dota 2',exact:true}).click();
 const panel=page.locator('#game-modal [data-game-stat-panel]');
 await panel.getByText('Загружаем статистику выбранного аккаунта…').waitFor();
 await page.locator(`#game-modal [data-action="select-account"][data-account="${b}"]`).click();
 await panel.getByText('45',{exact:true}).waitFor();
 releaseSlow();
 await page.waitForTimeout(250);
 assert.equal(await panel.getAttribute('data-account'),b);assert(!(await panel.innerText()).includes('123'));
 await page.locator(`#game-modal [data-action="select-account"][data-account="${a}"]`).click();
 await panel.getByText('123',{exact:true}).waitFor();
 assert.equal(reads.filter(([id,app])=>id===a&&app==='570').length,1);
 assert((await panel.innerText()).includes('—'));
 await panel.scrollIntoViewIfNeeded();
 const output=path.resolve(__dirname,'../../output/game-stats');await fs.mkdir(output,{recursive:true});
 await page.screenshot({path:path.join(output,'desktop.png')});
 for(const width of [800,390]){
  await page.setViewportSize({width,height:950});
  assert.equal(await panel.evaluate(el=>el.scrollWidth>el.clientWidth+1),false,`stats overflow ${width}`);
  const overflow=await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth);
  if(overflow){await page.screenshot({path:path.join(output,'overflow-'+width+'.png')});console.log(await page.evaluate(()=>({inner:innerWidth,root:document.documentElement.scrollWidth,body:document.body.scrollWidth,modal:document.getElementById('game-modal').getBoundingClientRect().toJSON(),elements:[...document.querySelectorAll('body *')].map(el=>({tag:el.tagName,id:el.id,cls:el.className,right:el.getBoundingClientRect().right,width:el.getBoundingClientRect().width,scroll:el.scrollWidth})).filter(el=>el.width>0&&(el.right>innerWidth+1||el.scroll>innerWidth)).slice(0,15)})));}
  assert.equal(overflow,false,`page overflow ${width}`);
 }
 await panel.scrollIntoViewIfNeeded();
 await page.screenshot({path:path.join(output,'mobile.png')});
 await page.evaluate(()=>document.getElementById('game-modal').close());
 await page.setViewportSize({width:1440,height:1100});
 await page.getByRole('button',{name:'Открыть Counter-Strike 2',exact:true}).click();
 await panel.locator('[data-game-stats-key]').fill('test-only-secret');
 await panel.getByRole('button',{name:'Загрузить статистику',exact:true}).click();
 await panel.getByText('123',{exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>JSON.stringify(localStorage).includes('test-only-secret')),false);
 assert.equal(await panel.locator('[data-game-stats-key]').inputValue(),'');
 assert.deepEqual(writes.filter(([endpoint])=>endpoint!=='/api/steam/check-restrictions'),[['/api/steam/game-stats',{api_key:'test-only-secret'}]]);
 assert.ok(!writes.some(([endpoint])=>/switch-account|open-launcher|\/launch/.test(endpoint)),'Statistics must not change launcher state');
 await page.evaluate(()=>document.getElementById('game-modal').close());
 await page.getByRole('button',{name:'Открыть Deadlock',exact:true}).click();
 await panel.getByText('Обновить не удалось. Показана сохранённая статистика.').waitFor();
 assert.deepEqual(errors,[]);
 await page.goto('http://swch.test/?demo=1');
 const count=reads.length;
 await page.getByRole('button',{name:'Открыть Dota 2',exact:true}).click();
 assert.equal(reads.length,count);
 console.log('PASS: stats placement, delayed-account isolation, unknown values, cached/stale data, key clearing, no account switching, mobile widths, demo isolation.');
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
