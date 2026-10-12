import {createInterface} from 'node:readline';
import {pathToFileURL} from 'node:url';
import {createSDK} from './index.mjs';
import {format} from 'node:util';
const MAX=1024*1024, pending=new Map();let sequence=0;
// stdout belongs to the protocol; normal console output goes to the log stream.
const send=message=>{const line=JSON.stringify({jsonrpc:'2.0',...message});if(Buffer.byteLength(line)>MAX)throw new Error('Message too large');process.stdout.write(line+'\n');};
console.log=(...args)=>console.error(format(...args));
const callHost=(method,params)=>new Promise((resolve,reject)=>{
  const id=`host:${++sequence}`;const timer=setTimeout(()=>{pending.delete(id);reject(new Error('Host request timed out'));},25000);
  pending.set(id,{resolve,reject,timer});send({id,method,params});
});
const registrations={launchers:[],tabs:[],actions:[]};
const bridge=createSDK(callHost,(method,params)=>send({method,params}),registrations);
let initialized=false,modId='';
const input=createInterface({input:process.stdin,crlfDelay:Infinity});
input.on('line',line=>{
  if(Buffer.byteLength(line)>MAX){process.exit(2);return;}
  let request;try{request=JSON.parse(line);}catch{process.exit(2);return;}
  if(!request.method){const p=pending.get(request.id);if(p){clearTimeout(p.timer);pending.delete(request.id);request.error?p.reject(new Error(request.error.message)):p.resolve(request.result);}return;}
  if(request.method==='event'&&!('id' in request)){const packet=request.params||{},prefix=`mod:${modId}:`;void bridge.onEvent(packet.event?.startsWith(prefix)?packet.event.slice(prefix.length):packet.event,packet.data??packet);return;}
  void (async()=>{
    try{
      let result;
      if(request.method==='initialize'){
        if(initialized)throw new Error('Already initialized');initialized=true;modId=request.params.id;
        const module=await import(pathToFileURL(process.argv[2]).href);
        if(typeof module.default!=='function')throw new Error('Backend must export default activate(swch)');
        await module.default(bridge.sdk);result={...registrations};registrations.frozen=true;
      }else result=await bridge.dispatch(request.method,request.params??{});
      send({id:request.id,result:result??null});
    }catch(error){send({id:request.id,error:{code:-32000,message:String(error.message).slice(0,1000)}});}
  })();
});
input.on('close',()=>process.exit(0));
process.on('uncaughtException',e=>{console.error(e);process.exit(1);});
