import test from 'node:test';
import assert from 'node:assert/strict';
import {createSDK} from '../mod-sdk/index.mjs';

const clone=value=>structuredClone(value);
const terminal=job=>['completed','failed','cancelled'].includes(job.status);
async function until(condition){
  const deadline=Date.now()+2000;
  while(!condition()){
    assert.ok(Date.now()<deadline,'Job did not advance');
    await new Promise(resolve=>setImmediate(resolve));
  }
}
function host(records=new Map()){
  let bridge;
  const changed=job=>queueMicrotask(()=>void bridge.onEvent('jobs.changed',clone(job)));
  const registrations={launchers:[],tabs:[],actions:[]};
  bridge=createSDK(async(method,params)=>{
    if(method==='jobs.recover'){
      for(const job of records.values())if(job.status==='running')job.status='queued';
      return clone([...records.values()].filter(job=>job.status==='queued'));
    }
    if(method==='jobs.list')return clone([...records.values()]);
    if(method==='jobs.start'){
      const job={id:`job-${records.size+1}`,type:params.type,input:params.input,status:'queued',progress:null,checkpoint:null,result:null,error:null,attempt:0};
      records.set(job.id,job);changed(job);return clone(job);
    }
    const job=records.get(params.id);assert.ok(job,`Missing job ${params.id}`);
    if(method==='jobs.get')return clone(job);
    if(method==='jobs.cancel'){
      if(!terminal(job)){job.status='cancelled';changed(job);}
      return clone(job);
    }
    if(method==='jobs.update'){
      if(!terminal(job)){
        if(params.status==='running'&&job.status!=='running')job.attempt++;
        for(const [key,value] of Object.entries(params))if(key!=='id')job[key]=clone(value);
        changed(job);
      }
      return clone(job);
    }
    throw new Error(`Unexpected SDK method ${method}`);
  },()=>{},registrations);
  return {bridge,records,registrations};
}

test('jobs validate declarations and reject unregistered starts before persistence',async()=>{
  const {bridge,records,registrations}=host();
  for(const id of ['','../other','UPPER','con','aux.mod','trailing.'])assert.throws(()=>bridge.sdk.jobs.register(id,()=>{}),/Invalid job definition/);
  bridge.sdk.jobs.register('download',()=>{});
  assert.deepEqual(registrations.job_types,['download']);
  assert.throws(()=>bridge.sdk.jobs.register('download',()=>{}),/Duplicate/);
  await assert.rejects(bridge.sdk.jobs.start('missing',{}),/Unknown job type/);
  assert.equal(records.size,0);
  registrations.frozen=true;
  assert.throws(()=>bridge.sdk.jobs.register('late',()=>{}),/during activation/);
  bridge.shutdown();
});

test('duplicate queued notifications run once while internal UI calls and cancellation remain live',async()=>{
  const {bridge,records}=host();let calls=0,context,release;
  const finished=new Promise(resolve=>{release=resolve;});
  bridge.sdk.jobs.register('download',async(input,job)=>{calls++;context=job;await job.progress({bytes:42});await finished;return input;});
  bridge.sdk.registerMethod('ping',input=>({pong:input}));
  await bridge.recoverJobs();
  const job=await bridge.sdk.jobs.start('download',{url:'file'});
  await bridge.onEvent('jobs.changed',job);
  await bridge.onEvent('jobs.changed',job);
  await until(()=>records.get(job.id).progress?.bytes===42);
  assert.deepEqual(await bridge.dispatch('method',{id:'ping',input:42}),{pong:42});
  assert.equal(calls,1);
  await bridge.sdk.jobs.cancel(job.id);
  await until(()=>context.signal.aborted);
  release();
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(records.get(job.id).status,'cancelled');
  assert.equal(records.get(job.id).result,null);
  await bridge.onEvent('jobs.changed',job);
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(calls,1);
  bridge.shutdown();
});

test('recovery keeps checkpoints and fails removed job types instead of stranding them',async()=>{
  const records=new Map([
    ['resume',{id:'resume',type:'download',status:'running',input:{url:'file'},progress:{bytes:42},checkpoint:{offset:42},attempt:1}],
    ['obsolete',{id:'obsolete',type:'removed',status:'queued',input:{},checkpoint:null,attempt:0}],
  ]);
  const {bridge}=host(records);
  bridge.sdk.jobs.register('download',async(input,job)=>({url:input.url,offset:job.savedCheckpoint.offset,attempt:job.attempt}));
  await bridge.recoverJobs();
  await until(()=>records.get('resume').status==='completed'&&records.get('obsolete').status==='failed');
  assert.deepEqual(records.get('resume').result,{url:'file',offset:42,attempt:2});
  assert.match(records.get('obsolete').error,/no longer registered: removed/);
  bridge.shutdown();
});

test('runtime shutdown aborts workers without converting recoverable jobs to failures',async()=>{
  const {bridge,records}=host();let signal;
  bridge.sdk.jobs.register('download',async(_input,job)=>{
    signal=job.signal;
    await job.checkpoint({offset:42});
    await new Promise((resolve,reject)=>job.signal.addEventListener('abort',()=>reject(job.signal.reason),{once:true}));
  });
  await bridge.recoverJobs();
  const job=await bridge.sdk.jobs.start('download',{});
  await until(()=>records.get(job.id).checkpoint?.offset===42);
  bridge.shutdown();
  assert.equal(signal.aborted,true);
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(records.get(job.id).status,'running');
  assert.deepEqual(records.get(job.id).checkpoint,{offset:42});
});
