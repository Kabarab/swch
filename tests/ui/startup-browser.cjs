const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
(async()=>{
 const server=await staticServer(),browser=await chromium.launch(launchOptions);
 try {
  const page=await browser.newPage({viewport:{width:1000,height:900}}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/api/**',route=>route.fulfill({json:emptyApi(new URL(route.request().url()).pathname)}));
  await page.addInitScript(()=>{
   let mode='off',listener;
   window.startupWrites=[];
   window.startupFail=false;
   window.chrome.webview={
    addEventListener(type,callback){if(type==='message')listener=callback;},
    postMessage(message){
     window.startupWrites.push(message);
     if(window.startupSilent)return;
     if(message.startsWith('startup:set:')&&!window.startupFail)mode=message.slice(12);
     setTimeout(()=>listener({data:{type:'swch-startup',mode,errorCode:window.startupFail?5:0}}),10);
    }
   };
  });
  await page.goto(server.origin);
  const open=()=>page.locator('[data-action="settings"]').click();
  const select=page.locator('#startup-mode');
  await open();await page.waitForFunction(()=>!document.getElementById('startup-mode').disabled);
  assert.equal(await select.inputValue(),'off');
  for(const mode of ['open','minimized','off']) {
   await select.selectOption(mode);
   await page.waitForFunction(()=>!document.getElementById('startup-mode').disabled);
   assert.equal(await select.inputValue(),mode);
   assert.equal(await page.evaluate(()=>startupWrites.at(-1)),`startup:set:${mode}`);
  }
  await select.selectOption('minimized');
  await page.waitForFunction(()=>!document.getElementById('startup-mode').disabled);
  await page.evaluate(()=>document.getElementById('styles-dialog').close());
  await open();await page.waitForFunction(()=>!document.getElementById('startup-mode').disabled);
  assert.equal(await select.inputValue(),'minimized');
  assert.match(await page.locator('#startup-note').innerText(),/После входа в Windows.*свёрнутым/);
  await page.evaluate(()=>window.startupFail=true);
  await select.selectOption('open');
  await page.locator('#startup-note').filter({hasText:'код Windows: 5'}).waitFor();
  assert.equal(await select.inputValue(),'minimized');
  assert.equal(await select.isDisabled(),false);
  await page.setViewportSize({width:390,height:844});
  assert(await page.locator('#settings-startup-group').evaluate(e=>e.scrollWidth<=e.clientWidth+1));
  await page.evaluate(()=>{window.startupFail=false;window.startupSilent=true;document.getElementById('styles-dialog').close();});
  await open();await page.locator('#startup-note').filter({hasText:'Не удалось получить ответ'}).waitFor({timeout:8000});
  assert.equal(await select.isDisabled(),true);
  await page.evaluate(()=>{window.startupSilent=false;document.getElementById('styles-dialog').close();});
  await open();await page.waitForFunction(()=>!document.getElementById('startup-mode').disabled);
  // Reload reads the native setting instead of a browser preference.
  await page.reload();await open();await page.waitForFunction(()=>!document.getElementById('startup-mode').disabled);
  assert.equal(await select.inputValue(),'off');
  // Demo mode never calls the native bridge.
  await page.goto(server.origin+'/?demo=1');await open();
  assert.equal(await select.isDisabled(),true);
  assert.deepEqual(await page.evaluate(()=>startupWrites),[]);
  const web=await browser.newPage();
  await web.route('**/api/**',route=>route.fulfill({json:emptyApi(new URL(route.request().url()).pathname)}));
  await web.goto(server.origin);await web.locator('[data-action="settings"]').click();
  assert.equal(await web.locator('#startup-mode').isDisabled(),true);
  assert.match(await web.locator('#startup-note').innerText(),/приложении SWCH для Windows/);
  assert.deepEqual(errors,[]);
  console.log('Startup UI checks passed: modes, native persistence, errors, timeout, demo, browser and mobile.');
 } finally {await browser.close();await server.close();}
})().catch(e=>{console.error(e);process.exit(1);});
