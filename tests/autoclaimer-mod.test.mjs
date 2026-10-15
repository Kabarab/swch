import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {buildUI,buildPackage} from '../scripts/build-autoclaimer-mod.mjs';
import {inspectZip} from '../mod-sdk/validation.mjs';

test('giveaways and automation form one self-contained mod interface',()=>{
  const html=buildUI();
  const script=html.match(/<script type="module">([\s\S]*)<\/script>/)[1];
  assert.doesNotThrow(()=>new Function(script));
  assert(!/^import\s/m.test(script));
  for(const id of ['claim-panel-games','claim-panel-accounts','claim-panel-history','claim-panel-options'])assert(html.includes(`id="${id}"`));
  assert(html.includes("swch.call('action'"));
  assert(!html.includes('settings-tab-giveaways'));
  assert(!fs.existsSync('ui/auto-claim.js'));
});

test('package validates with one manifest and one platform native service',()=>{
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'swch-claim-package-'));
  try {
    const binary=path.join(directory,'native.exe');fs.writeFileSync(binary,'package fixture');
    const zip=buildPackage('windows-x64',directory,binary);
    const {manifest}=inspectZip(fs.readFileSync(zip));
    assert.equal(manifest.id,'swch.autoclaimer');
    assert.equal(manifest.name,'Раздачи игр и автоклеймер');
    assert.deepEqual(manifest.platforms,['windows-x64']);
  } finally { fs.rmSync(directory,{recursive:true,force:true}); }
});

const binary=process.env.SWCH_AUTOCLAIMER_TEST_BINARY;
test('native mod stops on disable and restores settings on restart', {skip:!binary,timeout:30000},async()=>{
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'swch-claim-native-'));
  const target=process.platform==='win32'?'windows-x64':'macos-arm64';
  const native=path.join(directory,'native',target);fs.mkdirSync(native,{recursive:true});
  fs.copyFileSync(binary,path.join(native,'swch-autoclaimer'+(process.platform==='win32'?'.exe':'')));
  fs.copyFileSync('mods/autoclaimer/backend.mjs',path.join(directory,'backend.mjs'));
  const previous={SWCH_DB_PATH:process.env.SWCH_DB_PATH,SWCH_AUTO_CLAIM_NO_SCHEDULER:process.env.SWCH_AUTO_CLAIM_NO_SCHEDULER};
  process.env.SWCH_DB_PATH=path.join(directory,'test.db');
  process.env.SWCH_AUTO_CLAIM_NO_SCHEDULER='1';
  let dispose;
  try {
    const {default:activate}=await import(pathToFileURL(path.join(directory,'backend.mjs')).href);
    let control;const tabs=[];
    const sdk={registerTab:tab=>tabs.push(tab),registerAction:(action,fn)=>{assert.equal(action.id,'control');control=fn;},log:()=>{}};
    dispose=await activate(sdk);
    assert.deepEqual(tabs,[{id:'giveaways',name:'Раздачи игр'}]);
    const api=async(path,body)=>{
      const {job}=control({operation:'request',path,...(body!==undefined?{body}:{})});
      for(let i=0;i<200;i++){
        const result=control({operation:'poll',job});
        if(result.done){if(result.error)throw new Error(result.error);return result.result;}
        await new Promise(resolve=>setTimeout(resolve,25));
      }
      throw new Error('Native mod request timed out');
    };
    const initial=await api('/api/free-games');
    await api('/api/free-games/settings',{...initial.settings,enabled:false,dry_run:true,interval_minutes:180});
    await dispose();dispose=null;
    assert.throws(()=>control({operation:'request',path:'/api/free-games'}),/остановлен/);
    dispose=await activate(sdk);
    const restored=await api('/api/free-games');
    assert.equal(restored.settings.enabled,false);
    assert.equal(restored.settings.dry_run,true);
    assert.equal(restored.settings.interval_minutes,180);
    assert.equal(restored.run.running,false);
  } finally {
    if(dispose)await dispose();
    for(const [key,value] of Object.entries(previous))value===undefined?delete process.env[key]:process.env[key]=value;
    fs.rmSync(directory,{recursive:true,force:true});
  }
});
