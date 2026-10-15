import test from 'node:test';
import assert from 'node:assert/strict';
import {modGames,modMatchesGame,bridgeJSON} from '../../ui/mod-games.mjs';
import {defaultModFilters,modGameChoices,filterModEntries,modDownloadStats} from '../../ui/mod-filters.mjs';

test('combined mod filters apply games, installed state, type, compatibility, search and popularity',()=>{
  const genshin={launcher:'hoyoverse',app_id:'genshin',name:'Genshin Impact'},other={launcher:'steam',app_id:'genshin',name:'Different game'};
  const entry=(id,games,extra={})=>({manifest:{id,name:id,description:'inventory',platforms:['windows-x64'],games,ui:'index.html',...extra}});
  const catalog=[{...entry('alpha',[genshin]),downloads:12},{...entry('beta',[genshin],{theme:'theme.json'}),downloads:80},{...entry('gamma',[other],{ui:null,backend:'backend.mjs'}),downloads:2},{...entry('general',[]),downloads:0},entry('unknown',[])];
  const mods=[{...catalog[0],games:[other],enabled:false,source:{downloads:4}},{...catalog[1],enabled:true}];
  const environment={mods,catalog,compatibility:{beta:false},target:'windows-x64'};
  const filter=overrides=>filterModEntries(catalog,{...defaultModFilters(),...overrides},environment).map(m=>m.manifest.id);
  assert.deepEqual(filter({game:JSON.stringify(['hoyoverse','genshin'])}),['beta']);
  assert.deepEqual(filter({game:JSON.stringify(['steam','genshin']),status:'disabled',search:'Different'}),['alpha']);
  assert.deepEqual(filter({game:'general'}),['general','unknown']);
  assert.deepEqual(filter({status:'available'}),['gamma','general','unknown']);
  assert.deepEqual(filter({status:'enabled',kind:'theme'}),['beta']);
  assert.deepEqual(filter({status:'installed',compatibility:'compatible'}),['alpha']);
  assert.deepEqual(filter({compatibility:'incompatible'}),['beta']);
  assert.deepEqual(filter({kind:'backend'}),['gamma']);
  assert.deepEqual(filter({sort:'downloads'}),['beta','alpha','gamma','general','unknown']);
  assert.deepEqual(filter({search:'no such mod'}),[]);
  assert.equal(modDownloadStats(mods[0],catalog).downloads,12);
  assert.equal(modDownloadStats(catalog[3]).downloads,0);assert.equal(modDownloadStats(catalog[4]),null);
  assert.equal(modGameChoices([{...genshin,name:'Library name'}],mods,catalog).find(g=>g.launcher==='hoyoverse').name,'Library name');
});

test('bindings match stable launcher/app IDs, independent of display names',()=>{
  const mod={manifest:{games:[{launcher:'steam',app_id:'2767030',name:'Marvel Rivals'}]}};
  assert.equal(modMatchesGame(mod,{launcher:'steam',app_id:2767030,name:'Translated title'}),true);
  assert.equal(modMatchesGame(mod,{launcher:'epic',app_id:'2767030',name:'Marvel Rivals'}),false);
  assert.equal(modMatchesGame(mod,{launcher:'steam',app_id:'730',name:'Marvel Rivals'}),false);
  assert.equal(modMatchesGame({manifest:{}},{launcher:'steam',app_id:'730'}),false);
  assert.deepEqual(modGames({...mod,games:[]}),[]);
  assert.equal(modMatchesGame({...mod,games:[{launcher:'hoyoverse',app_id:'genshin'}]},{launcher:'hoyoverse',app_id:'genshin'}),true);
});

test('game and account text cannot terminate the iframe bootstrap script',()=>{
  const context={name:'</script><script>window.injected=true</script>',display_name:'<&>'};
  const encoded=bridgeJSON(context);
  assert.doesNotMatch(encoded,/[<>&]/);
  assert.deepEqual(JSON.parse(encoded),context);
});
