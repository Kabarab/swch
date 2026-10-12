// Synthetic store pages in an isolated browser. No real accounts or orders.
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const source=fs.readFileSync(path.join(__dirname,'../../src/auto_claim/web_rules.rs'),'utf8').match(/const SCRIPT: &str = r#"([\s\S]*?)"#;/)[1];
function expression(platform='epic',title='Paid Game',identity='expected'){return `(async function(config){${source}})(${JSON.stringify({platform,title,identity})})`;}
function fixture({price='Free',original='$10.00',label='Get',extra='',title='Paid Game',account='expected'}={}) {return `<header><span data-account-id="${account}"></span></header><main><h1>${title}</h1><aside data-testid="purchase-area"><s data-testid="original-price">${original}</s><span data-testid="current-price">${price}</span><span>-100%</span><p>Sale ends tomorrow</p><button data-testid="purchase-cta-button">${label}</button></aside>${extra}</main>`;}
(async()=>{
 const browser=await chromium.launch({channel:'chrome',headless:true});
 try {
  const page=await browser.newPage();await page.route('https://www.google.com/recaptcha/**',r=>r.fulfill({contentType:'text/html',body:'<html><body></body></html>'}));const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(fixture());let s=await page.evaluate(expression());assert.equal(s.price_zero,true);assert.equal(s.original_price_positive,true);assert.equal(s.checkout_offer_matches,true);assert.equal(s.identity,'expected');
  await page.setContent(fixture({extra:'<iframe title="reCAPTCHA" src="https://www.google.com/recaptcha/api2/anchor?size=invisible" style="width:256px;height:60px"></iframe>'}));s=await page.evaluate(expression());assert.equal(s.challenge,false);assert.equal(s.claim_label,'Get');
  await page.setContent(fixture({extra:'<iframe title="reCAPTCHA challenge" src="https://www.google.com/recaptcha/api2/bframe" style="width:400px;height:400px"></iframe>'}));s=await page.evaluate(expression());assert.equal(s.challenge,true);
  await page.setContent(fixture({price:'$5.00',extra:'<span>Free</span>'}));s=await page.evaluate(expression());assert.equal(s.price_zero,false);
  await page.setContent(fixture({title:'Paid Game Deluxe'}));s=await page.evaluate(expression());assert.equal(s.checkout_offer_matches,false);
  await page.setContent(fixture({label:'In Library'}));s=await page.evaluate(expression());assert.equal(s.in_library,true);
  await page.setContent(fixture({extra:'<section>Other game is In Library</section>'}));s=await page.evaluate(expression());assert.equal(s.in_library,false);
  await page.setContent(fixture({extra:'<input name="dateOfBirth">'}));s=await page.evaluate(expression());assert.equal(s.age_gate,true);
  await page.setContent('<title>Just a moment...</title><main>Please complete a security check to continue</main>');s=await page.evaluate(expression());assert.equal(s.challenge,true);
  await page.setContent(fixture({extra:'<div role="dialog">This is free. Add it to your library to get started.<button>Add to library</button><p>By selecting Add to library, you certify that you are over 18.</p></div>'}));s=await page.evaluate(expression());assert.equal(s.claim_label,'Add to library');assert.equal(s.price_zero,true);assert.equal(s.requires_confirmation,false);assert.equal(s.age_gate,false);
  await page.setContent(fixture({label:'Place order',extra:'<div data-testid="order-item"><h3>Paid Game</h3></div><div data-testid="order-item"><h3>Other Game</h3></div><span data-testid="order-total">0 USD</span>'}));s=await page.evaluate(expression());assert.equal(s.checkout_item_count,2);assert.equal(s.checkout_offer_matches,false);
  await page.setContent(fixture({label:'Place order',extra:'<div data-testid="order-item"><h3>Paid Game</h3></div><span data-testid="order-total">0 USD</span>'}));s=await page.evaluate(expression());assert.equal(s.checkout_item_count,1);assert.equal(s.checkout_total_zero,true);
  // The marker moves with the selected button; an old Get button is not clicked.
  await page.setContent(fixture());await page.evaluate(expression());await page.evaluate(()=>{document.querySelector('main').insertAdjacentHTML('beforeend','<div role="dialog"><button>Add to library</button></div>');});s=await page.evaluate(expression());assert.equal(s.claim_label,'Add to library');assert.equal(await page.locator('[data-swch-claim-target="1"]').count(),1);
  await page.route('https://www.gog.com/**',r=>r.fulfill({contentType:'text/html',body:fixture({account:'misleading-markup'})}));
  let loggedIn=true;await page.route('https://menu.gog.com/v1/account/basic',r=>r.fulfill({json:{isLoggedIn:loggedIn,userId:123,username:'Gog User'},headers:{'access-control-allow-origin':'https://www.gog.com','access-control-allow-credentials':'true'}}));
  await page.goto('https://www.gog.com/en/game/paid-game');s=await page.evaluate(expression('gog','Paid Game','gog_123'));assert.equal(s.identity,'gog_123');assert.equal(s.signed_in,true);
  loggedIn=false;s=await page.evaluate(expression('gog','Paid Game','gog_123'));assert.equal(s.signed_in,false);
  await page.unroute('https://menu.gog.com/v1/account/basic');await page.route('https://menu.gog.com/v1/account/basic',r=>r.fulfill({contentType:'application/json',body:'{"isLoggedIn":true,"userId":60105607071443587,"username":"Gog User"}',headers:{'access-control-allow-origin':'https://www.gog.com','access-control-allow-credentials':'true'}}));s=await page.evaluate(expression('gog','Paid Game','gog_60105607071443587'));assert.equal(s.identity,'gog_60105607071443587');
  assert.deepEqual(errors,[]);
  console.log('PASS: store snapshots reject paid prices, different editions, extra cart items, false ownership, stale buttons, age gates and security challenges; GOG identity comes from the signed account API. No live orders.');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
