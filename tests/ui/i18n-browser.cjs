const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');
(async()=>{
 const server=await staticServer(),browser=await chromium.launch(launchOptions);
 try {
  const context=await browser.newContext({viewport:{width:1280,height:850},permissions:['clipboard-read','clipboard-write']});
  const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
  let selected='ru',corrupt=false,offline=false;
  await page.route('**/api/**',async route=>{
   const request=route.request(),url=new URL(request.url());let json=emptyApi(url.pathname);
   if(url.pathname==='/api/i18n/selection'){
    if(request.method()==='POST')selected=request.postDataJSON().locale;
    json={locale:selected,status:'ok'};
   }
   if(url.pathname==='/api/mods/events'){await route.fulfill({contentType:'text/event-stream',body:': heartbeat\n\n'});return;}
   await route.fulfill({json});
  });
  await page.route('**/ui/lang/packs/*.json',async route=>{
   if(offline){await route.abort();return;}
   if(corrupt&&route.request().url().endsWith('/de.json')){await route.fulfill({json:{schema:1,locale:'de',messages:{}}});return;}
   await route.continue();
  });
  await page.route('https://raw.githubusercontent.com/**',route=>route.abort());
  await page.goto(server.origin+'/?demo=1');
  await page.waitForFunction(()=>document.documentElement.lang==='ru'&&!document.documentElement.classList.contains('i18n-loading'));
  await page.evaluate(()=>{const alert=document.createElement('div');alert.id='diagnostic-probe';alert.textContent="Installation rolled back: Mod operation failed: EISDIR: illegal operation on a directory, lstat 'C:'";document.body.append(alert);});
  const russianDiagnostic="Установка отменена: Ошибка при выполнении мода: EISDIR: недопустимая операция с папкой, lstat 'C:'";
  await page.waitForFunction(expected=>document.getElementById('diagnostic-probe').textContent===expected,russianDiagnostic);
  await page.locator('[data-action="settings"]').click();
  await page.locator('#language-select').selectOption('en');await page.locator('#language-apply').click();
  await page.waitForFunction(()=>document.documentElement.lang==='en');
  assert.equal(await page.locator('#diagnostic-probe').textContent(),"Installation rolled back: Mod operation failed: EISDIR: illegal operation on a directory, lstat 'C:'");
  assert.equal(selected,'en');assert.equal(await page.locator('#language-machine').isVisible(),true);
  assert.equal(await page.locator('[data-page="library"]').getAttribute('aria-label'),'Library');
  const residual=await page.locator('#styles-dialog').evaluate(el=>{
   const walker=document.createTreeWalker(el,NodeFilter.SHOW_TEXT),texts=[];
   while(walker.nextNode()){const n=walker.currentNode;if(!n.parentElement.closest('[translate=no],code,pre,textarea')&&/[А-Яа-яЁё]/.test(n.nodeValue))texts.push(n.nodeValue.trim());}
   return texts.filter(Boolean);
  });
  assert.deepEqual(residual,[],'Untranslated settings text');
  corrupt=true;await page.locator('#language-select').selectOption('de');await page.locator('#language-apply').click();
  await page.waitForFunction(()=>!document.querySelector('#language-apply').disabled);
  assert.equal(await page.locator('html').getAttribute('lang'),'en');assert.equal(selected,'en');corrupt=false;
  await page.locator('#language-select').selectOption('de');await page.locator('#language-apply').click();
  await page.waitForFunction(()=>document.documentElement.lang==='de');
  assert.doesNotMatch(await page.locator('#diagnostic-probe').textContent(),/Installation rolled back|Mod operation failed|illegal operation/);
  assert.ok((await page.locator('#diagnostic-probe').textContent()).endsWith("lstat 'C:'"));
  await page.setViewportSize({width:390,height:850});
  assert.ok(await page.locator('#styles-dialog').evaluate(el=>el.scrollWidth<=el.clientWidth+1),'German dialog overflow');
  await page.setViewportSize({width:1280,height:850});
  await page.locator('#language-select').selectOption('ar');await page.locator('#language-apply').click();
  await page.waitForFunction(()=>document.documentElement.dir==='rtl');
  assert.equal(selected,'ar');
  fs.mkdirSync(path.join(server.root,'output/i18n'),{recursive:true});
  await page.screenshot({path:path.join(server.root,'output/i18n/settings-ar.png'),fullPage:true});
  for(const width of [390,768,1280]){
   await page.setViewportSize({width,height:850});assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'RTL overflow '+width);
   assert.ok(await page.locator('#styles-dialog').evaluate(el=>el.scrollWidth<=el.clientWidth+1),'Dialog overflow '+width);
   assert.ok(await page.evaluate(()=>document.querySelector('.main-shell').getBoundingClientRect().right<=document.querySelector('.sidebar').getBoundingClientRect().left+1),'Sidebar overlaps RTL content '+width);
  }
  offline=true;await page.reload();await page.waitForFunction(()=>document.documentElement.dir==='rtl'); // IndexedDB cache survives reload.
  await page.evaluate(()=>{const alert=document.createElement('div');alert.id='diagnostic-probe';alert.textContent="Installation rolled back: Mod operation failed: EISDIR: illegal operation on a directory, lstat 'C:'";document.body.append(alert);});
  await page.waitForFunction(()=>!document.getElementById('diagnostic-probe').textContent.startsWith('Installation rolled back'));
  await page.locator('[data-page="mods"]').click();
  await page.locator('#mod-copy-prompt').waitFor({state:'visible'});
  assert.equal(await page.locator('#mod-author-guide').getAttribute('open'),null,'Copy is available with the guide closed');
  const prompt=await page.locator('#mod-ai-prompt').inputValue();assert.match(prompt,/Build a complete, working mod for SWCH/);assert.doesNotMatch(prompt,/[А-Яа-яЁё]/);
  await page.locator('#mod-copy-prompt').click();assert.equal((await page.evaluate(()=>navigator.clipboard.readText())).replace(/\r\n/g,'\n'),prompt);
  await page.evaluate(()=>{navigator.clipboard.writeText=()=>Promise.reject(new Error('Clipboard permission denied'));});
  await page.locator('#mod-copy-prompt').click();
  assert.equal((await page.evaluate(()=>navigator.clipboard.readText())).replace(/\r\n/g,'\n'),prompt,'Fallback copies the entire prompt');
  assert.equal(await page.locator('#mod-prompt-preview').getAttribute('open'),null,'Successful copy does not expand the long prompt');
  await page.evaluate(()=>{document.execCommand=()=>false;});
  await page.locator('#mod-copy-prompt').click();
  assert.equal(await page.locator('#mod-ai-prompt').evaluate(el=>el.selectionEnd-el.selectionStart),prompt.length,'Manual fallback selects all text');
  assert.equal(await page.locator('#mod-prompt-preview').getAttribute('open'),'','Manual fallback makes selected text visible');
  for(const width of [390,768,1280]){
   await page.setViewportSize({width,height:850});
   assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'RTL prompt overflow '+width);
   assert.ok(await page.locator('.mod-prompt-card').evaluate(el=>el.scrollWidth<=el.clientWidth+1),'Prompt card overflow '+width);
  }
  await page.locator('#mod-author-guide summary').click();
  const untranslatedGuide=await page.locator('#mods-view').evaluate(el=>{
   const walker=document.createTreeWalker(el,NodeFilter.SHOW_TEXT),texts=[];
   while(walker.nextNode()){const n=walker.currentNode;if(!n.parentElement.closest('[translate=no],code,pre,textarea')&&/[А-Яа-яЁё]/.test(n.nodeValue))texts.push(n.nodeValue.trim());}
   return texts.filter(Boolean);
  });assert.deepEqual(untranslatedGuide,[]);
  await page.screenshot({path:path.join(server.root,'output/i18n/mod-guide-ar.png'),fullPage:true});
  await page.locator('[data-action="settings"]').click();await page.locator('#language-select').selectOption('ru');await page.locator('#language-apply').click();await page.waitForFunction(()=>document.documentElement.lang==='ru');
  assert.equal(await page.locator('#diagnostic-probe').textContent(),russianDiagnostic);
  assert.equal(await page.locator('[data-page="library"]').getAttribute('aria-label'),'Библиотека');
  await page.locator('#styles-dialog [data-close]').click();
  await page.locator('#mod-author-guide').scrollIntoViewIfNeeded();
  await page.screenshot({path:path.join(server.root,'output/i18n/mod-guide-ru.png')});
  assert.deepEqual(errors,[]);console.log('Language switching, full settings/guide, clipboard, RTL, cache and failed download passed.');
 }finally{await browser.close();await server.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
