import test from 'node:test';
import assert from 'node:assert/strict';
import {gamePlaytime,timeLabel,timeScopeLabel} from '../../ui/playtime.mjs';
import {metricKey} from '../../ui/model.mjs';
const riot={launcher:'riot',app_id:'valorant'};
const profile=(id,seconds,error=null)=>({account_id:id,app_id:'valorant',playtime:{value:seconds===null?null:{seconds},error}});
test('Riot time is visible, history scoped, and isolated by selected account',()=>{
 const riotProfiles=[profile('one',3600),profile('two',7200),profile('missing',null)];
 assert.equal(gamePlaytime(riot,{riotProfiles}).seconds,10800);
 const time=gamePlaytime(riot,{riotProfiles,accountId:'two'});
 assert.equal(timeLabel(time),'2 ч');assert.equal(timeScopeLabel(time),'по матчам');
 assert.equal(gamePlaytime(riot,{riotProfiles,accountId:'missing'}),null);
 assert.equal(gamePlaytime(riot,{riotProfiles:[profile('one',3600,'Offline')]}).cached,true);
});
test('unknown, negative, non-finite and null time are not displayed as zero',()=>{
 for(const seconds of [null,undefined,-1,NaN,Infinity,'3600'])assert.equal(gamePlaytime(riot,{riotProfiles:[profile('one',seconds)]}),null);
 assert.equal(timeLabel(gamePlaytime(riot,{riotProfiles:[profile('one',0)]})),'0 ч');
 assert.equal(timeLabel(gamePlaytime(riot,{riotProfiles:[profile('one',20)]})),'< 1 мин');
 assert.equal(timeLabel(gamePlaytime(riot,{riotProfiles:[profile('one',120)]})),'2 мин');
});
test('Steam snapshots deduplicate per account and preserve the existing minutes conversion',()=>{
 const game={launcher:'steam',app_id:'730'},steamRows=[{app_id:'730',account_id:'a',playtime_minutes:60},{app_id:'730',account_id:'a',playtime_minutes:120},{app_id:'730',account_id:'b',playtime_minutes:null}];
 assert.equal(gamePlaytime(game,{steamRows}).seconds,7200);
 assert.equal(gamePlaytime(game,{steamRows,accountId:'b'}),null);
});
test('every supported library uses imported time; native totals take priority over Galaxy and manual entries',()=>{
 for(const launcher of ['epic','gog','ubisoft','minecraft','roblox','hoyoverse']) {
  const game={launcher,app_id:'game'},accounts=[{id:'a',launcher}];
  const records=[{launcher,app_id:'game',account_id:'a',seconds:3600,scope:'lifetime',source:launcher},{launcher,app_id:'game',account_id:null,seconds:10800,scope:'galaxy_local',source:'Galaxy'}];
  assert.equal(gamePlaytime(game,{accounts,records,metrics:{[metricKey(game,accounts[0])]:{hours:99}}}).seconds,3600);
  assert.equal(gamePlaytime(game,{accounts,records,accountId:'missing'}),null);
  assert.equal(gamePlaytime({...game,playtime_minutes:60},{accounts}).seconds,3600);
 }
});
test('manual fallback and local UUID aliases do not duplicate imported totals',()=>{
 const game={launcher:'epic',app_id:'one',owner_ids:['a']},accounts=[{id:'a',launcher:'epic'},{id:'b',launcher:'epic'}];
 assert.equal(gamePlaytime(game,{accounts,metrics:{[metricKey(game,accounts[0])]:{hours:2},[metricKey(game,accounts[1])]:{hours:10}}}).seconds,7200);
 const records=['a','alias'].map(account_id=>({launcher:'minecraft',app_id:'minecraft_java',account_id,seconds:3600,identity:'uuid-1',source:'Minecraft',scope:'local_worlds'}));
 assert.equal(gamePlaytime({launcher:'minecraft',app_id:'minecraft_java'},{records}).seconds,3600);
 assert.equal(gamePlaytime({launcher:'minecraft',app_id:'minecraft_java'},{records,accountId:'alias'}).seconds,3600);
});
