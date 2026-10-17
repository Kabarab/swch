const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
const {staticServer,emptyApi,launchOptions}=require('./harness.cjs');
(async()=>{
 const server=await staticServer(),browser=await chromium.launch(launchOptions);
 try {
  const page=await browser.newPage({viewport:{width:1100,height:900}}),errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  const initial=()=>({phase:'downloading',progress_percent:12,download_percent:12.5,downloaded_bytes:1310720,total_bytes:10485760,bytes_per_second:262144,eta_seconds:35,message:'Скачивание обновления с GitHub…'});
  let status={phase:'idle'},offline=false,applies=0;
  await page.route('**/api/**',async route=>{
   const endpoint=new URL(route.request().url()).pathname;
   if(endpoint==='/api/update/status'){
    if(offline)return route.abort();
    return route.fulfill({json:status});
   }
   if(endpoint==='/api/update/check')return route.fulfill({json:{current_version:'1.3.3',latest_version:'1.3.4',update_available:true,release_notes:'Тест обновления'}});
   if(endpoint==='/api/update/apply'){
    applies++;status=initial();return route.fulfill({json:{status:'started'}});
   }
   return route.fulfill({json:emptyApi(endpoint)});
  });
  await page.goto(server.origin);
  await page.locator('[data-action="settings"]').click();
  await page.locator('#btn-check-update').click();
  await page.locator('#btn-apply-update').click();
  const waitLabel=text=>page.waitForFunction(value=>document.getElementById('update-progress-label').textContent.includes(value),text);
  await waitLabel('Скачивание');
  const track=page.locator('#update-progress-track'),percent=page.locator('#update-progress-percent');
  assert.equal(await percent.innerText(),'12,5%');
  assert.equal(await track.getAttribute('aria-valuenow'),'12.5');
  assert.match(await page.locator('#update-progress-details').innerText(),/1,3 МиБ из 10 МиБ.*256 КиБ\/с.*35 с/);
  assert.equal(await page.locator('#update-available-panel').isVisible(),false);
  assert.equal(await page.locator('#update-progress-bar').evaluate(e=>getComputedStyle(e).transitionDuration),'0.5s');
  status={...initial(),download_percent:37.5,downloaded_bytes:3932160,message:'Получение следующей части'};
  await waitLabel('следующей');
  assert.equal(await percent.innerText(),'37,5%');
  assert.equal(await page.locator('#update-progress-bar').evaluate(e=>e.style.width),'37.5%');
  if(process.env.UPDATE_SCREENSHOT)await page.locator('#styles-dialog').screenshot({path:process.env.UPDATE_SCREENSHOT});
  status={...status,total_bytes:null,download_percent:null,eta_seconds:null,message:'Размер неизвестен'};
  await waitLabel('неизвестен');
  assert.equal(await percent.innerText(),'Загрузка');
  assert.equal(await track.getAttribute('aria-valuenow'),null);
  assert.match(await track.getAttribute('class'),/indeterminate/);
  assert.match(await page.locator('#update-progress-details').innerText(),/скачано.*Размер файла не сообщён/);
  status={...status,bytes_per_second:0,message:'Ожидание данных от сервера…'};
  await waitLabel('Ожидание данных');
  assert.doesNotMatch(await page.locator('#update-progress-details').innerText(),/\/с|Осталось/);
  offline=true;await waitLabel('Нет связи');offline=false;await waitLabel('Ожидание данных');
  status={...initial(),phase:'error',message:'Ошибка обновления: обрыв связи'};
  await waitLabel('обрыв');
  assert.equal(await percent.innerText(),'Ошибка');
  assert.equal(await page.locator('#btn-apply-update').isEnabled(),true);
  assert.equal(await page.locator('#update-available-panel').isVisible(),true);
  await page.locator('#btn-apply-update').click();await waitLabel('Скачивание');
  assert.equal(applies,2);
  // Reload reconnects to an ongoing update and keeps the apply button disabled.
  await page.reload();await page.locator('[data-action="settings"]').click();await waitLabel('Скачивание');
  await page.locator('#btn-check-update').click();
  assert.equal(await page.locator('#update-available-panel').isVisible(),false);
  assert.equal(await page.locator('#btn-apply-update').isDisabled(),true);
  for(const [phase,name] of [['extracting','Распаковка'],['installing','Установка']]){
   status={...initial(),phase,message:name};await waitLabel(name);
   assert.equal(await percent.innerText(),name);
   assert.doesNotMatch(await page.locator('#update-progress-details').innerText(),/\/с|Осталось/);
  }
  await page.setViewportSize({width:390,height:844});
  assert(await page.locator('#update-progress-panel').evaluate(e=>e.scrollWidth<=e.clientWidth+1));
  status={phase:'idle'};await waitLabel('Обновление прервано');
  assert.equal(await page.locator('#btn-apply-update').isEnabled(),true);
  assert.deepEqual(errors,[]);
  console.log('Update UI checks passed: real bytes, smooth progress, unknown size, waiting, connection loss, retry, reload, installation stages and mobile.');
 } finally {await browser.close();await server.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
