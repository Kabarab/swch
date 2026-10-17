const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
const revision=require('../../ui/lang/source.json').revision;
(async()=>{
 const server=await staticServer(),browser=await chromium.launch(launchOptions);
 try {
  async function scenario({locale='en-US',saved,stored,missing=[],expected='en',blockedStorage=false,systemLanguages}){
   const context=await browser.newContext({locale,viewport:{width:1280,height:850}}),page=await context.newPage();
   const errors=[],writes=[];page.on('pageerror',e=>errors.push(e.message));
   let selection=saved?{revision,...saved}:{locale:'en',preference:'system',saved:false};
   let currentSystem=systemLanguages;
   await page.route('**/api/**',async route=>{
    const request=route.request(),endpoint=new URL(request.url()).pathname;
    if(endpoint==='/api/mods/events')return route.fulfill({contentType:'text/event-stream',body:': heartbeat\n\n'});
    let json=emptyApi(endpoint);
    if(endpoint==='/api/i18n/selection'){
     if(request.method()==='POST'){const body=request.postDataJSON();writes.push(body);selection={locale:body.locale,preference:body.preference,revision:body.revision,saved:true};}
     json={...selection,...(currentSystem?{systemLanguages:currentSystem}:{})};
    }
    return route.fulfill({json});
   });
   await page.route('https://raw.githubusercontent.com/**',route=>route.abort());
   await page.route('**/ui/lang/packs/*.json',route=>missing.some(id=>route.request().url().endsWith('/'+id+'.json'))?route.abort():route.continue());
   if(stored)await page.addInitScript(value=>localStorage.setItem('swch-language',value),stored);
   if(blockedStorage)await page.addInitScript(()=>{Storage.prototype.getItem=()=>{throw new Error('Blocked');};Storage.prototype.setItem=()=>{throw new Error('Blocked');};});
   const automaticSave=(!saved||saved.preference==='system')?page.waitForResponse(response=>response.url().endsWith('/api/i18n/selection')&&response.request().method()==='POST').then(()=>null,error=>error):null;
   await page.goto(server.origin+'/?demo=1');
   await page.waitForFunction(expected=>document.documentElement.lang===expected&&!document.documentElement.classList.contains('i18n-loading'),expected);
   if(automaticSave){const saveError=await automaticSave;if(saveError)throw saveError;}
   await page.locator('[data-action="settings"]').click();
   assert.deepEqual(errors,[]);
   return {page,writes,context,selection:()=>selection,setSystem:value=>{currentSystem=value;}};
  }
  for(const [locale,expected] of [['en-US','en'],['ru-RU','ru'],['de-DE','de'],['fr-CA','fr'],['zh-SG','zh-Hans'],['he-IL','he'],['xx-XX','en']]){
   const item=await scenario({locale,expected});
   assert.equal(await item.page.locator('#language-select').inputValue(),'system',locale);
   assert.equal(item.selection().preference,'system');
   if(locale==='en-US'){
    assert.equal(await item.page.locator('[data-page="library"]').getAttribute('aria-label'),'Library');
    fs.mkdirSync(path.join(server.root,'output/i18n'),{recursive:true});
    await item.page.screenshot({path:path.join(server.root,'output/i18n/system-language-en.png')});
   }
   await item.context.close();
  }
  for(const options of [
   {locale:'de-DE',saved:{locale:'ru',preference:'ru',saved:true},expected:'ru'},
   {locale:'ru-RU',saved:{locale:'en',preference:'en',saved:true},expected:'en'},
   {locale:'en-US',saved:{locale:'ru',saved:true},stored:'en',expected:'ru'},
   {locale:'en-US',systemLanguages:['ru-RU'],expected:'ru'},
   {locale:'de-DE',missing:['de'],expected:'en'},
   {locale:'en-US',blockedStorage:true,expected:'en'},
  ]){const item=await scenario(options);await item.context.close();}
  const item=await scenario({locale:'de-DE',saved:{locale:'en',preference:'en',saved:true},expected:'en'});
  await item.page.locator('#language-select').selectOption('system');await item.page.locator('#language-apply').click();
  await item.page.waitForFunction(()=>document.documentElement.lang==='de'&&!document.querySelector('#language-apply').disabled);
  assert.equal(item.selection().preference,'system');assert.equal(item.writes.at(-1).locale,'de');
  item.setSystem(['ru-RU']);await item.page.reload();await item.page.waitForFunction(()=>document.documentElement.lang==='ru');
  await item.context.close();
  console.log('System defaults, regional locales, English fallback, saved overrides, storage failures, and OS changes passed.');
 }finally{await browser.close();await server.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
