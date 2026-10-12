const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync(require('node:path').join(__dirname,'../../ui/launcher.js'),'utf8');
const batch=source.slice(source.indexOf('async function syncAllRiotProfiles()'),source.indexOf('function renderPlayedGames('));
(async()=>{
 const accounts=[{id:'one',launcher:'riot',display_name:'One',is_active:true},{id:'two',launcher:'riot',display_name:'Two'},{id:'three',launcher:'riot',display_name:'Three'}];
 let active='one';const calls=[],saved=[],messages=[];
 const errors=new Map();
 const context=vm.createContext({demo:false,busy:false,riotSyncBusy:false,riotSyncProgress:'',accounts,riotProfileErrors:errors,render(){},toast:m=>messages.push(m),applyRiotProfiles:data=>{if(data.profile)saved.push(data.profile);},api:async(path,body)=>{
  calls.push([path,body]);
  if(path==='/api/riot/switch-account'){active=body.account_id;return {};}
  if(path==='/api/riot/accounts/sync'){
   assert.equal(body.app_id,'valorant');assert.equal(body.account_id,active);
   if(active==='two')throw new Error('Riot requires confirmation');
   return {profile:active};
  }
  assert.equal(path,'/api/riot/accounts');return {};
 }});
 vm.runInContext(batch,context);
 await context.syncAllRiotProfiles();
 assert.deepEqual(saved,['one','three'],'A failed account must not prevent loading the rest');
 assert.equal(active,'one','Original login must be restored after partial failure');
 assert.equal(errors.get('two:valorant'),'Riot requires confirmation');
 assert.ok(messages.at(-1).includes('2/3'));
 assert.equal(context.busy,false);assert.equal(context.riotSyncBusy,false);
 assert.ok(!calls.some(([path])=>['/api/riot/launch','/api/riot/open-launcher'].includes(path)),'Never launch or install a game');
 context.api=async(path,body)=>{
  if(path==='/api/riot/switch-account'&&body.account_id==='one')throw new Error('Restore failed');
  return {};
 };
 await context.syncAllRiotProfiles();
 assert.ok(messages.at(-1).includes('Restore failed'),'Failed restoration must be reported');
 assert.equal(context.busy,false);
 console.log('Riot inventory batch: no game launch, account isolation, partial failure, original login restoration passed');
})().catch(error=>{console.error(error);process.exitCode=1;});
