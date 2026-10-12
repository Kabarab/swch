import test from 'node:test';
import assert from 'node:assert/strict';
import {VALORANT_WEAPONS} from '../../ui/valorant-weapons.mjs';
import {weaponForSkin,riotInventoryData} from '../../ui/riot-inventory.mjs';
const section=value=>({value,error:null});
const vandal=VALORANT_WEAPONS.find(w=>w.name==='Vandal'),ghost=VALORANT_WEAPONS.find(w=>w.name==='Ghost');
const profiles=[{app_id:'valorant',account_id:'a',display_name:'First',skins:section([{id:vandal.skins[0],name:'Translated skin',image_url:'image'}]),buddies:section([{id:'buddy',name:'Buddy'}]),champions:section([{id:'jett',name:'Jett'}])},{app_id:'valorant',account_id:'b',display_name:'Second',skins:section([{id:ghost.skins[0],name:'Ghost skin'}]),champions:section([{id:'jett',name:'Jett'},{id:'sage',name:'Sage'}]),buddies:{value:null,error:'Offline'}}];
test('default collection includes all kinds and merges shared heroes by identity',()=>{
 const data=riotInventoryData(profiles);
 assert.equal(data.rows.length,5);
 assert.deepEqual(data.sections.map(s=>s.kind),['skins','buddies','champions']);
 assert.deepEqual(data.rows.map(i=>i.kind),['skins','skins','buddies','champions','champions']);
 assert.deepEqual(data.sections[2].items.map(i=>i.name),['Jett','Sage']);
 const jett=data.rows.find(i=>i.id==='jett');assert.deepEqual(jett.owners,[{id:'a',name:'First'},{id:'b',name:'Second'}]);
 assert.equal(riotInventoryData(profiles,{account:'a'}).rows.length,3);
 assert.equal(riotInventoryData(profiles,{account:'b',kind:'champions'}).rows.length,2);
 assert.deepEqual(profiles[0].champions.value,[{id:'jett',name:'Jett'}]);
});
test('weapon UUID mapping works offline, filters ignore buddies and heroes and retain unknown skins',()=>{
 assert.equal(weaponForSkin({id:vandal.skins[0],name:'Translated skin'}).name,'Vandal');
 assert.equal(weaponForSkin({id:'old',name:'Vendetta Ghost'}).name,'Ghost');
 assert.equal(weaponForSkin({id:'unknown',name:'Unknown'}),null);
 const data=riotInventoryData(profiles,{category:'rifle',weapon:vandal.id});
 assert.equal(data.filtered.length,1);assert.equal(data.filtered[0].kind,'skins');
 assert.deepEqual(data.categories.map(c=>[c.id,c.count]),[['sidearm',1],['rifle',1]]);
 assert.equal(riotInventoryData(profiles,{search:'Jett'}).filtered.length,1);
 assert.equal(riotInventoryData(profiles,{search:'missing'}).filtered.length,0);
});
test('partial collection preserves cached sections and identifies missing categories per account',()=>{
 const accounts=['a','b','missing'].map(id=>({id,launcher:'riot'}));
 const data=riotInventoryData(profiles,{accounts});
 assert.equal(data.pending.length,4);assert.equal(data.errors[0].kind,'buddies');assert.equal(data.errors[0].cached,false);
 assert.equal(riotInventoryData(profiles,{accounts,account:'missing'}).known,false);
 assert.equal(riotInventoryData([{...profiles[0],champions:section([]),skins:section([]),buddies:section([])}],{accounts:accounts.slice(0,1)}).pending.length,0);
});
