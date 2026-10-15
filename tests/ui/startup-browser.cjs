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
   window.chrome ??= {};
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
  const toggle=page.locator('#startup-toggle'),modes=page.locator('#startup-modes');
  const choice=mode=>modes.locator(`input[value="${mode}"]`);
  const ready=()=>page.waitForFunction(()=>!document.getElementById('startup-toggle').disabled);
  await open();await ready();
  assert.equal(await toggle.getAttribute('role'),'switch');
  assert.equal(await page.locator('#roblox-multi-instance-toggle').getAttribute('role'),'switch');
  assert.equal(await toggle.isChecked(),false);
  assert.equal(await modes.isVisible(),false);
  await toggle.focus();await page.keyboard.press('Space');await ready();
  assert.equal(await modes.isVisible(),true);
  assert.equal(await choice('open').isChecked(),true);
  assert.equal(await page.evaluate(()=>startupWrites.at(-1)),'startup:set:open');
  await choice('minimized').check();await ready();
  assert.equal(await page.evaluate(()=>startupWrites.at(-1)),'startup:set:minimized');
  await toggle.uncheck();await ready();
  assert.equal(await modes.isVisible(),false);
  assert.equal(await page.evaluate(()=>startupWrites.at(-1)),'startup:set:off');
  await toggle.check();await ready();
  assert.equal(await choice('minimized').isChecked(),true);
  assert.equal(await page.evaluate(()=>startupWrites.at(-1)),'startup:set:minimized');
  await choice('open').check();await ready();
  assert.equal(await page.evaluate(()=>startupWrites.at(-1)),'startup:set:open');
  await choice('minimized').check();await ready();
  await page.evaluate(()=>document.getElementById('styles-dialog').close());
  await open();await ready();
  assert.equal(await toggle.isChecked(),true);
  assert.equal(await choice('minimized').isChecked(),true);
  assert.match(await page.locator('#startup-note').innerText(),/После входа в Windows.*свёрнутым/);
  await page.evaluate(()=>window.startupFail=true);
  await choice('open').check();
  await page.locator('#startup-note').filter({hasText:'код Windows: 5'}).waitFor();
  assert.equal(await choice('minimized').isChecked(),true);
  assert.equal(await toggle.isDisabled(),false);
  await toggle.uncheck();await ready();
  assert.equal(await toggle.isChecked(),true);
  assert.equal(await modes.isVisible(),true);
  await page.setViewportSize({width:390,height:844});
  assert(await page.locator('#settings-startup-group').evaluate(e=>e.scrollWidth<=e.clientWidth+1));
  assert(await page.locator('#settings-startup-group').evaluate(e=>Array.from(e.querySelectorAll('label')).every(label=>label.scrollWidth<=label.clientWidth+1)));
  await page.evaluate(()=>{window.startupFail=false;document.getElementById('styles-dialog').close();});
  await open();await ready();
  await page.setViewportSize({width:1000,height:900});
  await page.locator('#settings-startup-group').scrollIntoViewIfNeeded();
  if(process.env.STARTUP_SCREENSHOT)await page.locator('#styles-dialog').screenshot({path:process.env.STARTUP_SCREENSHOT});
  await page.evaluate(()=>{window.startupFail=false;window.startupSilent=true;document.getElementById('styles-dialog').close();});
  await open();await page.locator('#startup-note').filter({hasText:'Не удалось получить ответ'}).waitFor({timeout:8000});
  assert.equal(await toggle.isDisabled(),true);
  await page.evaluate(()=>{window.startupSilent=false;document.getElementById('styles-dialog').close();});
  await open();await ready();
  // Reload reads the native setting instead of a browser preference.
  await page.reload();await open();await ready();
  assert.equal(await toggle.isChecked(),false);
  assert.equal(await modes.isVisible(),false);
  await page.evaluate(()=>window.startupFail=true);
  await toggle.check();await ready();
  assert.equal(await toggle.isChecked(),false);
  assert.equal(await modes.isVisible(),false);
  // Demo mode never calls the native bridge.
  await page.goto(server.origin+'/?demo=1');await open();
  assert.equal(await toggle.isDisabled(),true);
  assert.equal(await modes.isVisible(),false);
  assert.deepEqual(await page.evaluate(()=>startupWrites),[]);
  const web=await browser.newPage();
  await web.route('**/api/**',route=>route.fulfill({json:emptyApi(new URL(route.request().url()).pathname)}));
  await web.goto(server.origin);await web.locator('[data-action="settings"]').click();
  assert.equal(await web.locator('#startup-toggle').isDisabled(),true);
  assert.equal(await web.locator('#startup-modes').isVisible(),false);
  assert.match(await web.locator('#startup-note').innerText(),/приложении SWCH для Windows/);
  assert.deepEqual(errors,[]);
  console.log('Startup UI checks passed: modes, native persistence, errors, timeout, demo, browser and mobile.');
 } finally {await browser.close();await server.close();}
})().catch(e=>{console.error(e);process.exit(1);});
