const {chromium}=require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert=require('node:assert/strict');
(async()=>{
const browser=await chromium.launch({channel:'chrome',headless:true});const page=await browser.newPage({viewport:{width:1280,height:960}});const errors=[];page.on('pageerror',e=>errors.push(e.message));let writes=[];
await page.route('**/api/**',async route=>{
const req=route.request(),url=new URL(req.url());if(req.method()==='POST'){writes.push([url.pathname,req.postDataJSON()]);return route.fulfill({status:503,json:{error:'Проверочная ошибка: попробуй ещё раз'}});}
let json=[];
if(url.pathname==='/api/games'){await new Promise(r=>setTimeout(r,250));json=[{id:'730',app_id:'730',launcher:'steam',name:'Counter-Strike 2',is_installed:true}];}
if(url.pathname==='/api/steam/library')json={accounts:[{id:'s1',username:'steam1',display_name:'Steam 1',launcher:'steam',is_active:true},{id:'s2',username:'steam2',display_name:'Steam 2',launcher:'steam',is_active:false}],games:[{id:'730',app_id:'730',launcher:'steam',name:'Counter-Strike 2',is_installed:true,account_id:'s1',access:'local'}],warnings:[]};
if(url.pathname==='/api/epic/accounts')json={active_account_id:'e1',accounts:[{account_id:'e1',display_name:'Epic One'},{account_id:'e2',display_name:'Epic Two'}]};
if(url.pathname==='/api/epic/library')json=[{app_name:'shared',title:'Epic shared game',is_installed:true}];
if(url.pathname==='/api/epic/auth/url')json={url:'https://www.epicgames.com/id/login'};
await route.fulfill({json});
});
await page.goto((process.env.SWCH_UI_URL || 'http://127.0.0.1:4243/'));await page.locator('.game-card').first().waitFor();assert.equal(await page.locator('.game-card').count(),2,'async Steam load must not erase Epic games or duplicate shared games');
await page.getByRole('button',{name:'Открыть Epic shared game',exact:true}).click();assert.equal(await page.locator('.account-chip').count(),2,'shared game exposes both owners');
assert((await page.locator('.metrics').innerText()).includes('Данные не добавлены'),'unknown real metrics must not be invented');
await page.locator('[data-account="e2"][data-action="select-account"]').click();await page.getByRole('button',{name:'Сделать активным',exact:true}).click();await page.getByRole('status').filter({hasText:'Проверочная ошибка'}).waitFor();assert.deepEqual(writes.at(-1),['/api/epic/switch',{account_id:'e2'}]);
await page.locator('[data-action="select-account"][data-account="e1"]').click();await page.getByRole('button',{name:'Играть',exact:true}).click();assert.deepEqual(writes.at(-1),['/api/epic/launch',{app_name:'shared',account_id:'e1'}]);
await page.locator('.connect-link').click();await page.locator('#epic-auth-link[href]').waitFor();await page.locator('#oauth-code-input').fill('test-code');await page.getByRole('button',{name:'Подключить Epic Games',exact:true}).click();await page.locator('#login-error').filter({hasText:'Проверочная ошибка'}).waitFor();assert.deepEqual(writes.at(-1),['/api/epic/auth/login',{code:'test-code'}]);assert.equal(await page.locator('#connect-dialog').isVisible(),true);
assert.deepEqual(errors,[]);console.log('PASS: delayed API aggregation, ownership, unknown metrics, switch/launch/login payloads and visible errors. No live writes.');await browser.close();
})();
