import {gameIdentity,modGames} from './mod-games.mjs';

export const defaultModFilters=()=>({game:'all',kind:'all',status:'all',compatibility:'all',sort:'name',search:''});
export function modGameChoices(library,mods,catalog){
  const choices=new Map();
  for(const game of [...catalog.flatMap(modGames),...mods.flatMap(modGames),...library]){
    if(game.launcher&&game.app_id)choices.set(gameIdentity(game),game);
  }
  return [...choices.values()];
}
export function modDownloadStats(entry,catalog=[]){
  const sources=[catalog.find(m=>m.manifest.id===entry.manifest.id),entry,entry.source];
  return sources.find(source=>Number.isSafeInteger(source?.downloads)&&source.downloads>=0)||null;
}
export function filterModEntries(entries,filters,{mods=[],catalog=[],compatibility={},target='',locale='ru'}={}){
  const query=filters.search.trim().toLocaleLowerCase(locale);
  return entries.filter(entry=>{
    const installed=mods.find(m=>m.manifest.id===entry.manifest.id),effective=installed||entry,m=entry.manifest;
    if(filters.game==='general'&&modGames(effective).length)return false;
    if(!['all','general'].includes(filters.game)&&!modGames(effective).some(game=>gameIdentity(game)===filters.game))return false;
    if(filters.kind!=='all'&&!m[filters.kind])return false;
    if(filters.status==='installed'&&!installed||filters.status==='available'&&installed)return false;
    if(filters.status==='enabled'&&(!installed||!installed.enabled)||filters.status==='disabled'&&(!installed||installed.enabled))return false;
    const supported=compatibility[m.id]??(target?m.platforms?.includes(target):true);
    if(filters.compatibility==='compatible'&&!supported||filters.compatibility==='incompatible'&&supported)return false;
    const text=[m.name,m.id,m.description,entry.author||installed?.source?.author,...modGames(effective).flatMap(game=>[game.name,game.launcher,game.app_id])].join(' ').toLocaleLowerCase(locale);
    return !query||text.includes(query);
  }).sort((a,b)=>{
    if(filters.sort==='downloads'){
      const difference=(modDownloadStats(b,catalog)?.downloads??-1)-(modDownloadStats(a,catalog)?.downloads??-1);
      if(difference)return difference;
    }
    return a.manifest.name.localeCompare(b.manifest.name,locale);
  });
}
