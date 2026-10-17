import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtemp, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {createInterface} from 'node:readline';
import {fileURLToPath} from 'node:url';

const runtime=fileURLToPath(new URL('../mod-sdk/runtime.mjs',import.meta.url));

async function fixture(t,source,onHost){
  const directory=await mkdtemp(path.join(tmpdir(),'swch-mod-runtime-'));
  const backend=path.join(directory,'backend.mjs');
  await writeFile(backend,source);
  const child=spawn(process.execPath,[runtime,backend],{cwd:directory,windowsHide:true,stdio:['pipe','pipe','pipe']});
  const pending=new Map();let sequence=0,stderr='';
  child.stderr.on('data',data=>{stderr=(stderr+data).slice(-4000);});
  const closed=new Promise(resolve=>child.once('close',resolve));
  const send=message=>child.stdin.write(JSON.stringify({jsonrpc:'2.0',...message})+'\n');
  const input=createInterface({input:child.stdout});
  input.on('line',line=>{
    const message=JSON.parse(line);
    if(message.method){
      void Promise.resolve().then(()=>message.method==='jobs.recover'?[]:onHost(message)).then(
        result=>send({id:message.id,result:result??null}),
        error=>send({id:message.id,error:{code:-32000,message:String(error.message)}}),
      );
      return;
    }
    const waiter=pending.get(message.id);pending.delete(message.id);
    if(waiter)message.error?waiter.reject(new Error(message.error.message)):waiter.resolve(message.result);
  });
  const fail=error=>{for(const waiter of pending.values())waiter.reject(error);pending.clear();};
  child.once('error',fail);
  child.once('close',code=>fail(new Error(`Runtime exited (${code}): ${stderr}`)));
  t.after(async()=>{child.kill();await closed;input.close();await rm(directory,{recursive:true,force:true});});
  return (method,params={})=>new Promise((resolve,reject)=>{
    const id=++sequence;pending.set(id,{resolve,reject});send({id,method,params});
  });
}

test('runtime carries SDK calls and action payloads larger than one MiB',{timeout:10000},async t=>{
  const payload='x'.repeat(2*1024*1024);
  let stored;
  const request=await fixture(t,`export default async s=>{
    await s.storage.set('payload','x'.repeat(2*1024*1024));
    s.registerAction({id:'echo',name:'Echo'},input=>input);
  }`,message=>{assert.equal(message.method,'storage.set');stored=message.params.value;});
  await request('initialize',{id:'large.mod'});
  assert.equal(stored,payload);
  assert.equal(await request('action',{id:'echo',input:payload}),payload);
});

test('runtime keeps SDK calls pending while the host takes longer than 25 seconds',{timeout:40000},async t=>{
  const request=await fixture(t,`export default async s=>{
    const value=await s.storage.get('slow');
    s.registerAction({id:'result',name:'Result'},()=>value);
  }`,async message=>{
    assert.equal(message.method,'storage.get');
    await new Promise(resolve=>setTimeout(resolve,26000));
    return 'complete';
  });
  await request('initialize',{id:'slow.mod'});
  assert.equal(await request('action',{id:'result'}),'complete');
});
