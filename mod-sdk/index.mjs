// The host supplies this SDK to the default export of backend.mjs.
export function createSDK(callHost, notify, registrations) {
  const launchers=new Map(), actions=new Map(),methods=new Map(),listeners=new Map(),jobHandlers=new Map(),runningJobs=new Map();
  registrations.job_types??=[];
  let jobsReady=false,stopping=false;
  const validLocalId=id=>typeof id==='string'&&id.length<=80&&/^[a-z0-9][a-z0-9._-]*$/.test(id)&&!id.endsWith('.')&&!/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/.test(id);
  const requireRegistration=()=>{if(registrations.frozen)throw new Error('Register contributions during activation');};
  const runJob=job=>{
    if(!jobsReady||stopping||job?.status!=='queued'||runningJobs.has(job.id))return;
    const controller=new AbortController();runningJobs.set(job.id,controller);
    void(async()=>{
      try{
        if(!jobHandlers.has(job.type)){await callHost('jobs.update',{id:job.id,status:'failed',error:'Job type is no longer registered: '+job.type});return;}
        const current=await callHost('jobs.update',{id:job.id,status:'running'});
        if(stopping||current.status!=='running')return;
        const context={id:job.id,signal:controller.signal,attempt:current.attempt,savedCheckpoint:current.checkpoint,
          progress:progress=>callHost('jobs.update',{id:job.id,progress}),
          checkpoint:checkpoint=>callHost('jobs.update',{id:job.id,checkpoint}),
        };
        controller.signal.throwIfAborted();
        const result=await jobHandlers.get(job.type)(current.input,context);
        if(!stopping)await callHost('jobs.update',{id:job.id,status:'completed',result:result??null});
      }catch(error){
        if(!stopping)try{await callHost('jobs.update',{id:job.id,status:'failed',error:String(error?.message??error)});}catch(failure){console.error('Job result could not be saved:',failure);}
      }finally{runningJobs.delete(job.id);}
    })();
  };
  const storage=(kind)=>({
    get:(key)=>callHost(`${kind}.get`,{key}),
    set:(key,value)=>callHost(`${kind}.set`,{key,value}),
    delete:(key)=>callHost(`${kind}.delete`,{key}),
  });
  const sdk={
    apiVersion:'1.1.0',
    library:{list:()=>callHost('library.list',{}),refreshSteam:(input={})=>callHost('library.refreshSteam',input)},
    accounts:{list:()=>callHost('accounts.list',{})},
    storage:storage('storage'), secrets:storage('secrets'),
    settings:{get:()=>callHost('settings.get',{}),set:value=>callHost('settings.set',{value})},
    files:{paths:()=>callHost('files.paths',{}),usage:()=>callHost('files.usage',{}),configure:(kind,directory)=>callHost('files.configure',{kind,directory}),clearCache:()=>callHost('files.clearCache',{})},
    dialogs:{openFile:(input={})=>callHost('dialogs.openFile',input),openFolder:(input={})=>callHost('dialogs.openFolder',input)},
    shell:{openFolder:path=>callHost('shell.openFolder',{path})},
    clipboard:{readText:()=>callHost('clipboard.readText',{}),writeText:text=>callHost('clipboard.writeText',{text})},
    tools:{list:()=>callHost('tools.list',{}),run:(id,args=[],options={})=>callHost('tools.run',{id,args,...options}),status:()=>callHost('tools.status',{}),stop:process_id=>callHost('tools.stop',{process_id})},
    jobs:{
      register(type,handler){requireRegistration();if(!validLocalId(type)||typeof handler!=='function')throw new Error('Invalid job definition');if(jobHandlers.has(type))throw new Error('Duplicate job type');jobHandlers.set(type,handler);registrations.job_types.push(type);},
      async start(type,input){if(!jobHandlers.has(type))throw new Error('Unknown job type: '+type);const job=await callHost('jobs.start',{type,input});setImmediate(()=>runJob(job));return job;},
      list:()=>callHost('jobs.list',{}),get:id=>callHost('jobs.get',{id}),cancel:id=>callHost('jobs.cancel',{id}),
    },
    events:{emit:(event,data)=>notify('event',{event,data}),on:(event,listener)=>{if(!listeners.has(event))listeners.set(event,new Set());listeners.get(event).add(listener);return ()=>listeners.get(event)?.delete(listener);}},
    registerLauncher(metadata,handlers){
      if(launchers.has(metadata.id))throw new Error('Duplicate launcher');
      if(registrations.frozen)throw new Error('Register contributions during activation');
      for(const op of metadata.capabilities||[])if(typeof handlers[op]!=='function')throw new Error('Declared capability has no handler: '+op);
      registrations.launchers.push(metadata);launchers.set(metadata.id,handlers);
    },
    registerTab(metadata){if(registrations.frozen)throw new Error('Register contributions during activation');registrations.tabs.push(metadata);},
    registerAction(metadata,handler){if(registrations.frozen)throw new Error('Register contributions during activation');if(actions.has(metadata.id))throw new Error('Duplicate action');registrations.actions.push(metadata);actions.set(metadata.id,handler);},
    registerMethod(id,handler){requireRegistration();if(typeof id!=='string'||!id||typeof handler!=='function')throw new Error('Invalid internal method');if(methods.has(id))throw new Error('Duplicate internal method');methods.set(id,handler);},
    log:(...args)=>console.error(...args),
  };
  return {sdk,async recoverJobs(){const jobs=await callHost('jobs.recover',{});jobsReady=true;for(const job of jobs)setImmediate(()=>runJob(job));},shutdown(){stopping=true;for(const controller of runningJobs.values())controller.abort(new Error('Mod stopped'));},async onEvent(event,data){
    if(event==='jobs.reconcile'&&jobsReady&&!stopping){
      try{for(const job of await callHost('jobs.list',{})){if(job.status==='cancelled')runningJobs.get(job.id)?.abort(new Error('Job cancelled'));else if(job.status==='queued')setImmediate(()=>runJob(job));}}catch(error){console.error('Job reconciliation failed:',error);}
    }
    if(event==='jobs.changed'){
      if(data?.status==='cancelled')runningJobs.get(data.id)?.abort(new Error('Job cancelled'));
      else if(data?.status==='queued')setImmediate(()=>runJob(data));
    }
    for(const fn of listeners.get(event)||[])try{await fn(data);}catch(e){console.error('Event handler failed:',e.message);}
  },async dispatch(method,params){
    if(method==='action'){
      const action=actions.get(params.id);if(!action)throw new Error('Unknown action');return await action(params.input);
    }
    if(method==='launcher'){
      const provider=launchers.get(params.id),handler=provider?.[params.operation];
      if(typeof handler!=='function')throw new Error('Unsupported launcher operation');
      return await handler(params.input??{});
    }
    if(method==='method'){
      const handler=methods.get(params.id);if(!handler)throw new Error('Unknown internal method');return await handler(params.input);
    }
    throw new Error('Unknown mod method');
  }};
}
