// One source of platform names, local logos and navigation rules.
export const PLATFORMS = {
  steam: {name:'Steam', logo:'/ui/platforms/steam.ico', inventory:true},
  epic: {name:'Epic Games', logo:'/ui/platforms/epic.ico', inventory:true},
  riot: {name:'Riot Games', logo:'/ui/platforms/riot.png', inventory:true},
  roblox: {name:'Roblox', logo:'/ui/platforms/roblox.png', inventory:true},
  minecraft: {name:'Minecraft', logo:'/ui/platforms/minecraft.png'},
  ubisoft: {name:'Ubisoft Connect', logo:'/ui/platforms/ubisoft.ico'},
  gog: {name:'GOG GALAXY', logo:'/ui/platforms/gog.png'},
  hoyoverse: {name:'HoYoverse', logo:'/ui/platforms/hoyoverse.ico'},
};
export const PLATFORM_IDS = Object.keys(PLATFORMS);
export function registerModPlatforms(platforms) {
  for(const id of PLATFORM_IDS.filter(id=>id.startsWith('mod:')))delete PLATFORMS[id];
  PLATFORM_IDS.splice(0,PLATFORM_IDS.length,...Object.keys(PLATFORMS));
  for(const p of platforms||[]) {
    if(!/^mod:[a-z0-9][a-z0-9_.-]*:[a-z0-9][a-z0-9_.-]*$/.test(p.id)||PLATFORMS[p.id])continue;
    PLATFORMS[p.id]={name:p.name,logo:'/ui/artwork/mod-platform.svg',mod_id:p.mod_id,capabilities:p.capabilities||[]};PLATFORM_IDS.push(p.id);
  }
}
export function inventoryPlatforms(accounts) {
  return PLATFORM_IDS.filter(id=>PLATFORMS[id].inventory && accounts.some(a=>a.launcher===id));
}
export function platformNavigation(games, accounts, page) {
  return PLATFORM_IDS.map(id => {
    const gameCount=games.filter(g=>g.launcher===id).length;
    const accountCount=accounts.filter(a=>a.launcher===id).length;
    return {id, count:page==='accounts'?accountCount:gameCount,
      visible:gameCount>0 || (page==='accounts' && accountCount>0)};
  });
}
