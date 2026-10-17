const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const path=require('node:path');
const {emptyApi,staticServer,launchOptions}=require('./harness.cjs');

(async()=>{
 const server=await staticServer();
 const browser=await chromium.launch(launchOptions);
 try{
  const page=await browser.newPage({viewport:{width:1280,height:960}});
  const errors=[],writes=[];
  let steamAccounts=[{id:'76561197960265729',username:'nazar',display_name:'Основной',launcher:'steam',is_active:true}];
  let failScan=false;
  page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/api/**',async route=>{
   const req=route.request(),url=new URL(req.url());
   let json=emptyApi(url.pathname);
   if(req.method()==='POST'){
    writes.push([url.pathname,req.postDataJSON()]);
    if(url.pathname==='/api/steam/library/sync')return route.fulfill({status:502,json:{error:'Проверочная ошибка синхронизации'}});
    json={status:'success'};
   }else if(url.pathname==='/api/steam/library'){
    if(failScan)return route.fulfill({status:502,json:{error:'Не удалось прочитать Steam'}});
    json={accounts:steamAccounts,games:[],warnings:[]};
   }else if(url.pathname==='/api/epic/auth/url')json={url:'https://www.epicgames.com/id/login'};
   else if(url.pathname==='/api/epic/accounts')json={accounts:[{account_id:'epic_one',display_name:'Epic Main'}]};
   else if(url.pathname==='/api/riot/accounts')json={accounts:[{id:'riot_one',username:'Main',display_name:'Main#EU'}],profiles:[]};
   else if(url.pathname==='/api/roblox/accounts')json=[{account_id:'roblox_one',username:'Builder'}];
   else if(['/api/minecraft/library','/api/ubisoft/library','/api/gog/library','/api/hoyoverse/library'].includes(url.pathname)){
    const launcher=url.pathname.split('/')[2];
    json={games:[],accounts:[{id:launcher+'_one',username:launcher,launcher}]};
   }
   await route.fulfill({json});
  });
  await page.goto(server.origin);
  await page.locator('#accounts-count').filter({hasText:'8'}).waitFor();
  await page.locator('[data-page="accounts"]').click();
  // The Add button must follow every platform filter, including Riot.
  for(const platform of ['riot','epic','roblox','minecraft','ubisoft','gog','hoyoverse','steam']){
   await page.locator(`.account-platform-tabs [data-platform="${platform}"]`).click();
   await page.locator('#accounts-view .accounts-list-heading [data-action="connect"]').click();
   await page.locator(`#connect-dialog [data-pane="${platform}"].active`).waitFor();
   assert.equal(await page.locator('#connect-platform-select').inputValue(),platform);
   await page.locator('#connect-dialog .dialog-header [data-close]').click();
  }
  await page.locator('.account-platform-tabs [data-platform="all"]').click();
  await page.locator('#accounts-view .accounts-list-heading [data-action="connect"]').click();
  const dialog=page.locator('#connect-dialog');
  assert.equal(await dialog.locator('[role="tab"][aria-selected="true"]').getAttribute('data-tab'),'steam');
  assert.equal(await dialog.locator('.connect-advanced').getAttribute('open'),null);
  await dialog.locator('[data-tab="steam"]').focus();
  await page.keyboard.press('ArrowDown');
  assert.equal(await dialog.locator('[role="tab"][aria-selected="true"]').getAttribute('data-tab'),'epic');
  await page.keyboard.press('End');
  assert.equal(await dialog.locator('[role="tab"][aria-selected="true"]').getAttribute('data-tab'),'hoyoverse');
  await page.keyboard.press('Home');
  await fs.mkdir(path.join(server.root,'output/connect-redesign'),{recursive:true});
  await page.screenshot({path:path.join(server.root,'output/connect-redesign/desktop.png'),animations:'disabled'});
  for(const [width,height] of [[1000,760],[800,600],[545,746],[390,844],[320,568]]){
   await page.setViewportSize({width,height});
   for(const platform of ['steam','epic','riot','roblox','minecraft','ubisoft','gog','hoyoverse']){
    if(width<=640)await page.locator('#connect-platform-select').selectOption(platform);
    else await dialog.locator(`[data-tab="${platform}"]`).click();
    assert.ok(await dialog.locator(`[data-pane="${platform}"]`).isVisible());
    const bounds=await dialog.boundingBox();
    assert.ok(bounds.x>=0&&bounds.y>=0&&bounds.x+bounds.width<=width&&bounds.y+bounds.height<=height,'Dialog fits '+width);
    assert.ok(await dialog.evaluate(el=>el.scrollWidth<=el.clientWidth),'Dialog has no horizontal overflow '+platform+' '+width);
    assert.ok(await dialog.locator('.connect-tabs-content').evaluate(el=>el.scrollWidth<=el.clientWidth),'Content has no horizontal overflow '+platform+' '+width);
    await dialog.locator('.connect-tabs-content').evaluate(el=>el.scrollTop=el.scrollHeight);
    const close=await dialog.locator('.dialog-header [data-close]').boundingBox();
    const done=await dialog.getByRole('button',{name:'Готово',exact:true}).boundingBox();
    assert.ok(close.y>=0&&done.y+done.height<=height,'Close controls remain visible');
   }
  }
  await page.setViewportSize({width:545,height:746});
  await page.locator('#connect-platform-select').selectOption('steam');
  await page.screenshot({path:path.join(server.root,'output/connect-redesign/mobile.png'),animations:'disabled'});
  // Search reports backend failures and updates the account picker without closing.
  failScan=true;
  await dialog.getByRole('button',{name:'Найти аккаунты',exact:true}).click();
  await dialog.locator('#steam-scan-status').filter({hasText:'Не удалось прочитать Steam'}).waitFor();
  failScan=false;
  steamAccounts.push({id:'76561197960265730',username:'second',display_name:'Второй',launcher:'steam'});
  await dialog.getByRole('button',{name:'Найти аккаунты',exact:true}).click();
  await dialog.locator('#steam-scan-status').filter({hasText:'Поиск завершён'}).waitFor();
  assert.equal(await dialog.locator('#steam-sync-account option').count(),2);
  assert.equal(await dialog.locator('#steam-tab-badge').textContent(),'2');
  await dialog.locator('.connect-advanced summary').click();
  await dialog.locator('#steam-api-key').fill('test-secret');
  await dialog.locator('#steam-access-token').fill('test-token');
  await dialog.getByRole('button',{name:'Обновить игры и семейный доступ',exact:true}).click();
  await dialog.locator('#steam-sync-error').filter({hasText:'Проверочная ошибка синхронизации'}).waitFor();
  assert.equal(await dialog.locator('#steam-api-key').inputValue(),'');
  assert.equal(await dialog.locator('#steam-access-token').inputValue(),'');
  assert.deepEqual(writes.filter(([endpoint])=>/switch-account|open-launcher|library\/sync|accounts\/save/.test(endpoint)),[['/api/steam/library/sync',{account_id:steamAccounts[0].id,api_key:'test-secret',access_token:'test-token'}]],'Opening and browsing platforms must not switch accounts');
  steamAccounts=[];
  await dialog.getByRole('button',{name:'Найти аккаунты',exact:true}).click();
  await dialog.locator('#steam-scan-status').filter({hasText:'Аккаунты не найдены'}).waitFor();
  assert.ok(await dialog.getByRole('button',{name:'Обновить игры и семейный доступ',exact:true}).isDisabled());
  await dialog.getByRole('button',{name:'Готово',exact:true}).click();
  assert.equal(await dialog.isVisible(),false);
  assert.deepEqual(errors,[]);
  console.log('PASS: contextual Add for all 8 platforms, keyboard navigation, responsive dialog, visible close controls, Steam scan states and credential clearing.');
 }finally{await browser.close();await server.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
