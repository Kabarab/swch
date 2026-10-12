const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const http=require('node:http');
(async()=>{
 const root=path.resolve(__dirname,'../..');
 const server=http.createServer((req,res)=>{
  const file=path.join(root,req.url.split('?')[0]==='/'?'index.html':req.url.split('?')[0]);
  res.setHeader('Content-Type',/\.m?js$/.test(file)?'text/javascript':file.endsWith('.css')?'text/css':'text/html');
  fs.createReadStream(file).on('error',()=>res.end()).pipe(res);
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 let browser;
 try{
  browser=await chromium.launch({channel:'chrome',headless:true});
  const page=await browser.newPage({viewport:{width:1280,height:960}}),errors=[],writes=[];
  page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/api/**',async route=>{
   const url=new URL(route.request().url());let data={};
   if(url.pathname==='/api/steam/artwork'){
    if(url.searchParams.get('kind')==='poster')return route.fulfill({status:404,body:''});
    return route.fulfill({contentType:'image/jpeg',body:fs.readFileSync(path.join(root,'ui/artwork/valorant-keyart.jpg'))});
   }
   if(route.request().method()==='POST')writes.push([url.pathname,route.request().postDataJSON()]);
   if(url.pathname==='/api/steam/library')data={accounts:[{id:'s1',launcher:'steam',username:'one',display_name:'One',is_active:true},{id:'s2',launcher:'steam',username:'two',display_name:'Two'}],games:[{app_id:'730',launcher:'steam',name:'Counter-Strike 2',account_id:'s1',access:'owned',is_installed:true},{app_id:'570',launcher:'steam',name:'Dota 2',account_id:'s2',access:'owned',is_installed:true}],warnings:[]};
   if(url.pathname==='/api/epic/accounts')data={accounts:[]};
   if(url.pathname==='/api/riot/library')data={games:[]};
   if(url.pathname==='/api/riot/accounts')data={profiles:[]};
   if(url.pathname==='/api/accounts/restrictions')data={restrictions:[]};
   if(url.pathname==='/api/steam/check-restrictions')data={restrictions:[],checked_account_ids:[]};
   await route.fulfill({json:data});
  });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.locator('.game-card').first().waitFor();
  assert.equal(await page.locator('[data-page="favorites"]').count(),0);
  await page.waitForFunction(()=>[...document.querySelectorAll('.poster-art')].every(img=>img.naturalWidth>0&&img.src.includes('kind=hero')));
  await page.locator('[data-action="favorite"][data-game="steam:570"]').click();
  assert.match(await page.locator('.game-card').first().innerText(),/Dota 2/);
  await page.locator('[data-page="accounts"]').click();
  await page.locator('.account-tile:has([data-account="s1"]) .account-management > summary').click();
  await page.locator('[data-action="manage-restrictions"][data-account="s1"]').first().click();
  assert.deepEqual(await page.locator('#restr-account-games option').evaluateAll(options=>options.map(o=>o.value)),['Counter-Strike 2']);
  assert.equal(await page.locator('#restr-add-game-select option[value="570"]').count(),0);
  await page.locator('#restr-custom-name').fill('Counter-Strike 2');
  assert.equal(await page.locator('#restr-custom-appid').inputValue(),'730');
  await page.locator('#restr-custom-name').fill('Another game');
  assert.equal(await page.locator('#restr-custom-appid').inputValue(),'');
  await page.locator('#restr-custom-name').fill('Counter-Strike 2');
  await page.locator('#restr-add-custom-btn').click();
  assert.match(await page.locator('#restr-game-bans-list').innerText(),/Counter-Strike 2/);
  assert.match(await page.locator('#restr-game-bans-list').innerText(),/730/);
  await page.keyboard.press('Escape');
  await page.locator('.account-tile:has([data-account="s2"]) .account-management > summary').click();
  await page.locator('[data-action="manage-restrictions"][data-account="s2"]').first().click();
  assert.deepEqual(await page.locator('#restr-account-games option').evaluateAll(options=>options.map(o=>o.value)),['Dota 2']);
  assert.deepEqual(writes.find(([url])=>url==='/api/steam/check-restrictions')[1],{account_ids:['s1','s2']});
  await page.screenshot({path:path.join(root,'output/restrictions-autocomplete.png')});
  assert.deepEqual(errors,[]);
  console.log('Account-specific game suggestions, exact AppID, favorites and artwork recovery passed.');
 }finally{await browser?.close();await new Promise(resolve=>server.close(resolve));}
})().catch(error=>{console.error(error);process.exitCode=1;});
