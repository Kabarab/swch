import test from 'node:test';import assert from 'node:assert/strict';import {accountState,runSummary,indexedResults} from '../../ui/claim-state.mjs';
const account={id:'one',launcher:'epic',connected:true,automatic:true};const data={settings:{steam:true,epic:true,platforms:['gog']},run:{results:[]}};
test('saved Epic cookies do not prove that checkout is ready',()=>{assert.equal(accountState(account,data).kind,'saved');const blocked={...data,run:{results:[{launcher:'epic',account_id:'one',status:'attention',message:'Проверка браузера или CAPTCHA',updated_at:'2026-10-08T10:00:00Z'}]}};assert.equal(accountState(account,blocked).kind,'attention');assert.equal(accountState(account,blocked).action,'manual');assert.equal(accountState({...account,id:'two'},blocked).kind,'saved');});
test('missing GOG store login differs from a library/network error',()=>{assert.equal(accountState({id:'g',launcher:'gog',connected:false},data).kind,'login');const d={...data,run:{results:[{launcher:'epic',account_id:'one',status:'attention',message:'Не удалось проверить библиотеку Epic.'}]}};assert.equal(accountState(account,d).action,'retry');});
test('successful browser verification clears older issues for only that account',()=>{const d={...data,account_results:[{launcher:'epic',account_id:'one',status:'attention',message:'Проверка безопасности',updated_at:'2026-10-08T10:00:00Z'}],verified_accounts:[{launcher:'epic',account_id:'one',at:'2026-10-08T11:00:00Z'}]};assert.equal(accountState(account,d).kind,'ready');d.run.results=[{launcher:'epic',account_id:'one',status:'attention',message:'Проверка безопасности',updated_at:'2026-10-08T12:00:00Z'}];assert.equal(accountState(account,d).kind,'attention');});
test('scoped runs preserve other accounts last results',()=>{const d={...data,account_results:[{launcher:'epic',account_id:'one',status:'attention',message:'CAPTCHA'},{launcher:'epic',account_id:'two',status:'owned',message:'Owned'}],run:{results:[{launcher:'epic',account_id:'two',status:'claimed',message:'Игра добавлена в библиотеку.'}]}};assert.equal(indexedResults(d).get('epic:one')[0].status,'attention');assert.equal(indexedResults(d).get('epic:two')[0].status,'claimed');});
test('new licenses differ from old ownership and saved keys',()=>{const counts=runSummary({run:{results:[{launcher:'steam',account_id:'s',status:'no_offers'},{launcher:'steam',account_id:'s',status:'no_offers'},{status:'claimed',message:'Игра добавлена в библиотеку.'},{status:'claimed',message:'Уже получена ранее.'},{status:'key_saved'},{status:'code_received'},{status:'attention'}]}});assert.deepEqual(counts,{added:1,owned:1,keys:1,issues:1,noOffers:1});});

test('native Epic mode uses saved account and never requests browser repair',()=>{
 const account={id:'e1',launcher:'epic',connected:false,automatic:true};const data={settings:{epic:true,epic_launcher_claims:true},run:{results:[{launcher:'epic',account_id:'e1',status:'attention',message:'Проверка безопасности браузера'}]}};
 const state=accountState(account,data);assert.equal(state.action,'check');assert.equal(state.kind,'attention');assert.match(state.detail,/Epic Launcher/);
});

test('unverified Epic store session cannot inherit an old browser verification',()=>{
 const d={...data,run:{results:[]},verified_accounts:[{launcher:'epic',account_id:'one',at:'2026-10-08T11:00:00Z'}]};
 const state=accountState({...account,store_verified:false},d);
 assert.equal(state.kind,'saved');assert.equal(state.action,'verify');assert.match(state.detail,/ещё не подтверждён/);
 assert.equal(accountState({...account,store_verified:true},d).kind,'ready');
 d.run.results=[{launcher:'epic',account_id:'one',status:'owned',message:'Owned'}];
 assert.equal(accountState({...account,store_verified:false},d).kind,'saved');
});
