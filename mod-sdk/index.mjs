// The host supplies this SDK to the default export of backend.mjs.
export function createSDK(callHost, notify, registrations) {
  const launchers=new Map(), actions=new Map(),listeners=new Map();
  const storage=(kind)=>({
    get:(key)=>callHost(`${kind}.get`,{key}),
    set:(key,value)=>callHost(`${kind}.set`,{key,value}),
    delete:(key)=>callHost(`${kind}.delete`,{key}),
  });
  const sdk={
    apiVersion:'1.0.0',
    library:{list:()=>callHost('library.list',{})},
    accounts:{list:()=>callHost('accounts.list',{})},
    storage:storage('storage'), secrets:storage('secrets'),
    settings:{get:()=>callHost('settings.get',{}),set:value=>callHost('settings.set',{value})},
    events:{emit:(event,data)=>notify('event',{event,data}),on:(event,listener)=>{if(!listeners.has(event))listeners.set(event,new Set());listeners.get(event).add(listener);return ()=>listeners.get(event)?.delete(listener);}},
    registerLauncher(metadata,handlers){
      if(launchers.has(metadata.id))throw new Error('Duplicate launcher');
      if(registrations.frozen)throw new Error('Register contributions during activation');
      for(const op of metadata.capabilities||[])if(typeof handlers[op]!=='function')throw new Error('Declared capability has no handler: '+op);
      registrations.launchers.push(metadata);launchers.set(metadata.id,handlers);
    },
    registerTab(metadata){if(registrations.frozen)throw new Error('Register contributions during activation');registrations.tabs.push(metadata);},
    registerAction(metadata,handler){if(registrations.frozen)throw new Error('Register contributions during activation');if(actions.has(metadata.id))throw new Error('Duplicate action');registrations.actions.push(metadata);actions.set(metadata.id,handler);},
    log:(...args)=>console.error(...args),
  };
  return {sdk,async onEvent(event,data){for(const fn of listeners.get(event)||[])try{await fn(data);}catch(e){console.error('Event handler failed:',e.message);}},async dispatch(method,params){
    if(method==='action'){
      const action=actions.get(params.id);if(!action)throw new Error('Unknown action');return await action(params.input);
    }
    if(method==='launcher'){
      const provider=launchers.get(params.id),handler=provider?.[params.operation];
      if(typeof handler!=='function')throw new Error('Unsupported launcher operation');
      return await handler(params.input??{});
    }
    throw new Error('Unknown mod method');
  }};
}
