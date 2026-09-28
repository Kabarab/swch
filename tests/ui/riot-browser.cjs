const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const http=require('node:http');
(async()=>{
 const root=path.resolve(__dirname,'../..');
 const server=http.createServer((req,res)=>{const file=path.join(root,req.url.split('?')[0]==='/'?'index.html':req.url.split('?')[0]);res.setHeader('Content-Type',file.endsWith('.js')||file.endsWith('.mjs')?'text/javascript':file.endsWith('.css')?'text/css':'text/html');fs.createReadStream(file).on('error',()=>res.end()).pipe(res);});
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 let browser;
 try {
 browser=await chromium.launch({channel:'chrome',headless:true});
 const page=await browser.newPage();let installed=true;const writes=[],errors=[];
 const section=value=>({value,updated_at:'2026-09-21T12:00:00Z',error:null});
 const profiles=[
 {account_id:'riot_one',app_id:'league_of_legends',display_name:'One#EU',level:10,has_play_history:true,synced_at:'2026-09-21T12:00:00Z',skins:section([{id:'1001',name:'One skin'}]),champions:section([{id:'1',name:'Annie'}]),playtime:section({seconds:3600,matches:2})},
 {account_id:'riot_two',app_id:'league_of_legends',display_name:'Two#EU',level:1,has_play_history:false,synced_at:'2026-09-21T12:00:00Z',skins:section([{id:'2001',name:'Two skin'}]),champions:section([]),playtime:section({seconds:0,matches:0})}
 ];
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/api/**',async route=>{
  const req=route.request(),url=new URL(req.url());let data={};
  if(url.pathname==='/api/riot/accounts')data={profiles};
  else if(req.method()==='POST'){writes.push([url.pathname,req.postDataJSON()]);data=url.pathname==='/api/riot/accounts/sync'?{profiles}:{message:'Запрос передан Riot Client.'};}
  else if(url.pathname==='/api/riot/library')data={games:[{id:'riot_league_of_legends',app_id:'league_of_legends',name:'League of Legends',launcher:'riot',is_installed:false}],client_installed:installed,capabilities:{launch:installed},message:installed?'Вход в Riot Client':'Riot Client не найден'};
  else if(url.pathname==='/api/steam/library')data={games:[],accounts:[],warnings:[]};
  else if(url.pathname==='/api/epic/accounts')data={accounts:[]};
  else if(url.pathname==='/api/accounts/restrictions')data={restrictions:[]};
  await route.fulfill({json:data});
 });
 await page.goto(`http://127.0.0.1:${server.address().port}/`);
 await page.locator('#riot-count').filter({hasText:'1'}).waitFor();
 await page.locator('[data-platform="riot"]').click();
 await page.getByRole('button',{name:'Установить / открыть',exact:true}).click();
 await page.waitForFunction(()=>!document.querySelector('.hero button').disabled);
 assert.deepEqual(writes,[['/api/riot/launch',{app_id:'league_of_legends'}]]);
 await page.getByRole('button',{name:'Открыть Riot Client',exact:true}).click();
 await page.waitForFunction(()=>!document.querySelector('.hero button').disabled);
 assert.equal(writes[1][0],'/api/riot/open-launcher');
 installed=false;
 await page.locator('.page-heading-actions [data-action="refresh"]').click();
 await page.getByRole('button',{name:'Нужен Riot Client',exact:true}).waitFor();
 assert.equal(await page.getByRole('button',{name:'Нужен Riot Client',exact:true}).isDisabled(),true);
 assert.deepEqual(errors,[]);
 // A positive history associates only the correct account with the game.
 assert.equal(await page.locator('.riot-profile').count(),1);
 assert.match(await page.locator('.riot-profile').innerText(),/One#EU/);
 assert.ok(!(await page.locator('.riot-profile').innerText()).includes('Two#EU'));
 await page.locator('[data-page="accounts"]').click();
 assert.equal(await page.locator('.riot-account').count(),2);
 await page.locator('#accounts-view > .riot-profile [data-app="league_of_legends"]').click();
 await page.waitForFunction(()=>!document.querySelector('#accounts-view > .riot-profile [data-app="league_of_legends"]').disabled);
 assert.deepEqual(writes.at(-1),['/api/riot/accounts/sync',{app_id:'league_of_legends'}]);
 assert.equal(await page.locator('.riot-account').count(),2);
 await page.locator('.riot-account').filter({hasText:'One#EU'}).getByRole('button',{name:'Скины и персонажи'}).click();
 assert.match(await page.locator('#riot-collection-dialog').innerText(),/One skin/);
 assert.ok(!(await page.locator('#riot-collection-dialog').innerText()).includes('Two skin'));
 await page.locator('#riot-collection-dialog [data-close]').click();
 await page.locator('.riot-account').filter({hasText:'Two#EU'}).getByRole('button',{name:'Скины и персонажи'}).click();
 assert.match(await page.locator('#riot-collection-dialog').innerText(),/Two skin/);
 await page.setViewportSize({width:390,height:844});
 assert.ok(await page.evaluate(()=>document.querySelector('#riot-collection-dialog').getBoundingClientRect().width<=innerWidth));
 await page.screenshot({path:'/tmp/swch-riot-collection.png',fullPage:true});
 assert.deepEqual(errors,[]);
 console.log('Riot UI: launch, missing client, positive-playtime association and isolated account collections passed');
 } finally {if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));}
})().catch(e=>{console.error(e);process.exitCode=1;});
