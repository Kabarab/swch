import {initI18n, t, getLocale, numberFormat, localizedConfirm} from './i18n.js?v=install-1';
import {gamePlaytime, timeLabel, timeScopeLabel} from './playtime.mjs?v=time-20261011-1';
import {PLATFORMS, PLATFORM_IDS, inventoryPlatforms} from './platforms.mjs?v=mods-1';
import {setupMods,loadModLibrary,modPageTitle,renderModPage,applyModTheme,isModPlatform,modCapability,modOperation,renderGameModsButton} from './mods.js?v=sff-navigation-20261018-2';
import {riotKinds, riotKindLabel, riotInventoryData} from './riot-inventory.mjs?v=inventory-20261011-2';
import {DEADLOCK_HEROES} from './deadlock-heroes.mjs?v=stats-20261011-1';
import {deadlockOverview,deadlockHeroAccess,deadlockHeroName,deadlockRankLabel} from './deadlock-stats.mjs?v=stats-20261011-1';
import {platformLogo, hydratePlatformLogos, renderPlatformNavigation, simplifyInventory, simplifyRiotInventory} from './ux.js?v=mods-1';
import { accountPlayedGames, DESIGNS, isMacPlatform, resolveDesign, STYLES, gameKey, metricKey, filterGames, normalizeEpic, normalizeMetrics, safeImage, mergeLibraries, normalizeSteam, steamStats, inventoryKey, appendInventory, eligibleSteamAccounts, aggregateSteamStats, filterInventory, priceKey, inventoryValue, CURRENCIES, convertUsdCents, accountLibraryValue, accountHasGameBan, accountRestrictionsSummary, isItemUnsellable, isIgnoredSteamApp, KNOWN_STEAM_APP_NAMES, KNOWN_GAME_SHORTS, getDota2HeroIconUrl, getInventoryGamesWithItems, DOTA2_LOGO_SVG, inventoryHeroItems, inventoryTagCounts, csInventoryCategories } from './model.mjs';
import { initStartup } from './startup.js?v=startup-20261014-switches';
import {readLibraryCache, writeLibraryCache} from './library-cache.mjs';
import {GamepadManager, detectControllerType} from './gamepad.mjs?v=gamepad-1';
void initI18n();
const confirm = localizedConfirm;
const $ = (id) => document.getElementById(id);
const paths = {
 library:'M4 4h6v16H4z M14 4h6v16h-6z M7 8h0 M17 8h0', heart:'M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1.1-1.1a5.5 5.5 0 0 0-7.8 7.8L12 21l8.8-8.6a5.5 5.5 0 0 0 0-7.8Z', users:'M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2 M16 3a4 4 0 0 1 0 8 M22 21v-2a4 4 0 0 0-3-3.9 M13 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0', plus:'M12 5v14 M5 12h14', palette:'M12 3a9 9 0 1 0 0 18h1a2 2 0 0 0 1-3.7c-1-.6-.5-2.3 1-2.3h3a3 3 0 0 0 3-3 9 9 0 0 0-9-9Z M7 10h.01 M10 6h.01 M15 6h.01 M18 10h.01', settings:'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm7.4-2.1c.1-.3.1-.6.1-.9s0-.6-.1-.9l2.1-1.6c.2-.2.3-.4.2-.7l-2-3.5c-.1-.2-.4-.3-.7-.2l-2.5 1c-.5-.4-1.1-.7-1.7-.9l-.4-2.7c0-.3-.3-.5-.6-.5h-4c-.3 0-.6.2-.6.5l-.4 2.7c-.6.2-1.2.5-1.7.9l-2.5-1c-.3-.1-.6 0-.7.2l-2 3.5c-.1.3 0 .5.2.7l2.1 1.6c-.1.3-.1.6-.1.9s0 .6.1.9l-2.1 1.6c-.2.2-.3.4-.2.7l2 3.5c.1.2.4.3.7.2l2.5-1c.5.4 1.1.7 1.7.9l.4 2.7c0 .3.3.5.6.5h4c.3 0 .6-.2.6-.5l.4-2.7c.6-.2 1.2-.5 1.7-.9l2.5 1c.3.1.6 0 .7-.2l2-3.5c.1-.3 0-.5-.2-.7l-2.1-1.6Z', refresh:'M20 7v5h-5 M4 17v-5h5 M6 7a7 7 0 0 1 12-1l2 6 M4 12l2 6a7 7 0 0 0 12-1', chevron:'m9 5 7 7-7 7', search:'M21 21l-5-5 M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0', grid:'M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z', list:'M3 3h5v18H3z M12 4h9 M12 10h9 M12 16h9 M12 21h9', close:'m6 6 12 12 M6 18 18 6', check:'m5 12 4 4L19 6', arrow:'M4 12h16 m-6-6 6 6-6 6', play:'m8 4 12 8-12 8Z', clock:'M12 8v5l3 2 M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0', diamond:'m12 3 9 9-9 9-9-9Z M3 12h18 M12 3l4 9-4 9-4-9Z', wallet:'M20 8V5H4a2 2 0 0 0-2 2v12h20V8H5 M22 11h-6v5h6 M18 13.5h.01', game:'M7 7h10a4 4 0 0 1 4 4l1 7a2 2 0 0 1-3 2l-4-3H9l-4 3a2 2 0 0 1-3-2l1-7a4 4 0 0 1 4-4Z M6 11v4 M4 13h4 M16 12h.01 M19 14h.01', ban:'M12 2a10 10 0 1 0 10 10A10 10 0 0 0 12 2Zm-5.7 14.3 10-10a8 8 0 0 1-10 10Zm11.4-8.6-10 10a8 8 0 0 1 10-10Z'
};
const ACCENT_COLORS = [
  { id: 'orange', name: 'Оранжевый', hex: '#f05a22', rgb: '240, 90, 34' },
  { id: 'apple-blue', name: 'Синий Apple', hex: '#0a84ff', rgb: '10, 132, 255' },
  { id: 'blue', name: 'Синий', hex: '#3b82f6', rgb: '59, 130, 246' },
  { id: 'emerald', name: 'Изумрудный', hex: '#10b981', rgb: '16, 185, 129' },
  { id: 'purple', name: 'Фиолетовый', hex: '#a855f7', rgb: '168, 85, 247' },
  { id: 'cyan', name: 'Бирюзовый', hex: '#06b6d4', rgb: '6, 182, 212' },
  { id: 'rose', name: 'Розовый', hex: '#f43f5e', rgb: '244, 63, 94' },
  { id: 'amber', name: 'Янтарный', hex: '#f59e0b', rgb: '245, 158, 11' },
  { id: 'indigo', name: 'Индиго', hex: '#6366f1', rgb: '99, 102, 241' },
];
let accentColor = read('swch-accent-color', 'orange');
function normalizeAccentHex(value) {
 return typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value) ? value.toLowerCase() : ACCENT_COLORS[0].hex;
}
let customAccentColor = normalizeAccentHex(read('swch-custom-accent-color', ACCENT_COLORS[0].hex));
const icon = name => `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${paths[name] || paths.game}"/></svg>`;
const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
function read(key, fallback) { try { return JSON.parse(localStorage.getItem(key)) ?? fallback; } catch { return fallback; } }
function save(key, value) { try { localStorage.setItem(key, JSON.stringify(value)); return true; } catch { toast('Не удалось сохранить: хранилище браузера недоступно.'); return false; } }
const demo = new URLSearchParams(location.search).get('demo') === '1';
const namespace = demo ? 'swch-demo' : 'swch';
let style = read('swch-style', 'epic'); if (!STYLES[style]) style = 'epic';
let savedDesign = read('swch-design', null);
let design = resolveDesign(savedDesign, isMacPlatform());
let hasSavedAccent = read('swch-accent-color', null) !== null;
let layout = read('swch-layout',STYLES[style].layout); if(!['cards','detail'].includes(layout))layout=STYLES[style].layout;
let currency=read('swch-currency','USD');if(!Object.hasOwn(CURRENCIES,currency))currency='USD';
let excludeUnsellableBanned = read('swch-exclude-unsellable-banned', true);
let inventoryLauncher = read('swch-inventory-launcher', 'steam');
let inventoryRenderGeneration=0;
let fx={rates:{USD:1},date:null,warning:null},fxLoading=false,fxError='';
const gamePriceQuotes=new Map();let libraryPriceJob=null;
let favorites = read(`${namespace}-favorites`, []); if (!Array.isArray(favorites)) favorites = [];
let recent = read(`${namespace}-recent`, {}), metrics = read(`${namespace}-metrics`, {});
if (!recent || typeof recent !== 'object' || Array.isArray(recent)) recent = {};
if (!metrics || typeof metrics !== 'object' || Array.isArray(metrics)) metrics = {};
let games = [], accounts = [], platform = 'all', installed = 'all', query = '', sort = 'default', page = 'library', selectedGame = read(`${namespace}-selected-game`, null);
let steamLibrarySources={};
let steamRows = [], steamAccountFilter = "all", steamAccessFilter = "all";
let accountRestrictions = new Map();
let restrictionsRefreshPending = false;
let currentEditingRestrictions = null;
let currentEditingGameBans = [];
const POPULAR_STEAM_GAMES = [
  { app_id: '730', name: 'Counter-Strike 2' },
  { app_id: '570', name: 'Dota 2' },
  { app_id: '440', name: 'Team Fortress 2' },
  { app_id: '252490', name: 'Rust' },
  { app_id: '578080', name: 'PUBG: BATTLEGROUNDS' },
  { app_id: '1172470', name: 'Apex Legends' },
  { app_id: '221100', name: 'DayZ' },
  { app_id: '381210', name: 'Dead by Daylight' },
  { app_id: '230410', name: 'Warframe' },
  { app_id: '1091500', name: 'Cyberpunk 2077' },
  { app_id: '271590', name: 'Grand Theft Auto V' },
];
let selectedAccounts = {}, busy = false, loading = true, metricTarget = null, toastTimer;
let libraryCacheRestored = false;
function restoreLibraryCache() {
 if(demo)return;
 let cached;
 try { cached=readLibraryCache(localStorage); } catch { return; }
 if(!cached)return;
 ({games,accounts,steamRows}=cached);
 libraryCacheRestored=true;
 $('footer-status').textContent=['Библиотека','(сохранено в кэше) ·','Загружаем игры и аккаунты.'].map(t).join(' ');
}
function persistLibraryCache() {
 try { writeLibraryCache(localStorage,{games,accounts,steamRows}); } catch { /* Storage can be disabled. */ }
}
const format = {format:value=>numberFormat({maximumFractionDigits:1}).format(value)};
const platformName = value => PLATFORMS[value]?.name || value;
let hoyoverseStatus = null;
let ubisoftStatus = null, gogStatus = null, gogAccountFilter = 'all';
async function launchGog(appId=null,accountId=null) {
 if(demo){toast('Демо: реальные приложения не запускаются.');return;}
 if(busy)return;busy=true;render();
 try {
  const result=await api(appId?'/api/gog/launch':'/api/gog/open-launcher',appId?{app_id:appId,account_id:accountId}:{});
  toast(result.message);
 }catch(e){toast(e.message);}finally{busy=false;await refresh();}
}

async function syncGog(accountId=null) {
 if(demo||busy)return;busy=true;render();
 try {
  const result=await api('/api/gog/library/sync',{account_id:accountId});
  toast(result.warnings?.length?result.warnings.join(' '):result.message);
 }catch(e){toast(e.message);}finally{busy=false;await refresh();}
}

async function launchUbisoft(appId=null, accountId=null) {
 if(demo){toast('Демо: реальные приложения не запускаются.');return;}
 if(busy)return;busy=true;render();
 try {
  const result=await api(appId?'/api/ubisoft/launch':'/api/ubisoft/open-launcher', {app_id:appId,account_id:accountId});
  toast(result.message);
 }catch(e){toast(e.message);}finally{busy=false;await refresh();}
}


async function launchHoyoverse(gameId=null, accountId=null) {
 if(demo){toast('Демо: реальные приложения не запускаются.');return;}
 if(busy)return;busy=true;render();
 try {
  if(accountId){
   toast('Переключаем аккаунт HoYoverse...');
   await api('/api/hoyoverse/switch-account',{account_id:accountId});
  }
  const result=await api(gameId?'/api/hoyoverse/launch':'/api/hoyoverse/open-launcher', gameId?{game:gameId}:{});
  toast(result.message || (gameId ? `Запуск ${gameId}...` : 'Открыт HoYoPlay'));
 }catch(e){toast('Ошибка HoYoverse: '+(e.message||e));}finally{busy=false;await refresh();}
}
let riotStatus = null, riotProfiles = [];
let riotInventoryAccount=read('swch-riot-inventory-account','all'), riotInventoryGame=read('swch-riot-inventory-game','valorant'), riotInventorySearch='';
let riotInventoryKind=read('swch-riot-inventory-kind-v2','all'),riotWeaponCategory='',riotWeapon='';
let riotSyncBusy=false, riotSyncProgress='';
const riotProfileErrors=new Map();
let robloxStatus = null, robloxAccounts = [];
let minecraftStatus = null, minecraftAccounts = [], minecraftProfiles = [], minecraftServers = [], minecraftWorlds = [];
const canRequestLaunch = g => isModPlatform(g.launcher) ? modCapability(g.launcher,'launch') : g.launcher==='gog' ? Boolean(gogStatus?.client_installed&&gogStatus?.launch_supported) : g.launcher==='ubisoft' ? Boolean(g.is_installed||ubisoftStatus?.client_installed) : g.launcher==='riot' ? Boolean(riotStatus?.capabilities?.launch) : (g.launcher==='roblox'||g.launcher==='minecraft'||g.launcher==='hoyoverse'||g.launcher==='steam'||g.launcher==='epic') ? true : Boolean(g.is_installed||g.update);
async function launchRiot(appId=null,accountId=null) {
 if(demo){toast('Демо: реальные приложения не запускаются.');return;}
 if(busy)return;busy=true;render();
 try {
   const target=accountId?accounts.find(a=>a.launcher==='riot'&&a.id===accountId):null;
   if(target&&!target.is_active){
     toast(`Переключаем сессию Riot на ${target.display_name||target.username}...`);
     await api('/api/riot/switch-account',{account_id:target.id,display_name:target.display_name,launch:false});
     accounts.forEach(a=>{if(a.launcher==='riot')a.is_active=a.id===target.id;});
   }
   const result=await api(appId?'/api/riot/launch':'/api/riot/open-launcher',appId?{app_id:appId}:{});toast(result.message);
 }
 catch(e){toast(e.message);}finally{busy=false;render();}
}
async function launchRoblox(placeId=null, accountId=null) {
 if(demo){toast('Демо: реальные приложения не запускаются.');return;}
 if(busy)return;busy=true;render();
 try {
   const target = accountId || accounts.find(a=>a.launcher==='roblox'&&a.is_active)?.id || accounts.find(a=>a.launcher==='roblox')?.id;
   const payload = { account_id: target, place_id: placeId };
   const result = await api('/api/roblox/launch', payload);
   toast(result.message || 'Roblox запущен');
 } catch(e){ toast('Ошибка запуска Roblox: ' + (e.message || e)); }
 finally { busy=false; render(); }
}
async function launchMinecraft({ accountId = null, profileId = null, mode = 'official', serverIp = null, launcherClient = null } = {}) {
 if(demo){toast('Демо: реальные приложения не запускаются.');return;}
 if(busy)return;busy=true;render();
 try {
   const target = accountId || accounts.find(a=>a.launcher==='minecraft'&&a.is_active)?.id || accounts.find(a=>a.launcher==='minecraft')?.id;
   const payload = {
     account_id: target || undefined,
     profile_id: profileId || undefined,
     mode: mode || 'official',
     launcher_client: launcherClient || undefined,
     server_ip: serverIp || undefined,
   };
   const result = await api('/api/minecraft/launch', payload);
   toast(result.message || 'Minecraft запускается...');
 } catch(e){ toast('Ошибка запуска Minecraft: ' + (e.message || e)); }
 finally { busy=false; render(); }
}
const art = ['#b05732','#414d85','#3f6265','#71506a','#486049','#6e5137'];
function fallback(game, wide = false) {
 const n = [...game.name].reduce((a,c) => a+c.charCodeAt(0), 0), color = art[n % art.length];
 return 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="${wide ? 1200 : 400}" height="600" viewBox="0 0 400 600"><defs><linearGradient id="a" x2=".7" y2="1"><stop stop-color="${color}"/><stop offset="1" stop-color="#131824"/></linearGradient></defs><rect width="400" height="600" fill="url(#a)"/><circle cx="310" cy="180" r="170" fill="none" stroke="#ffffff18" stroke-width="40"/><path d="M-40 530 200 180 480 490M-40 580 200 230 480 540" fill="none" stroke="#ffffff13" stroke-width="40"/><circle cx="210" cy="160" r="72" fill="#ffffff09"/></svg>`);
}
const cover = (g,wide = false) => !demo&&g.launcher==='steam'&&/^\d+$/.test(String(g.app_id))
 ? `/api/steam/artwork?app_id=${encodeURIComponent(g.app_id)}&kind=${wide?'hero':'poster'}`
 : safeImage(wide ? (g.hero_url || g.banner_url) : g.banner_url) || fallback(g,wide);
function imageTag(g, cls) { return `<img class="${cls}" src="${esc(cover(g))}" alt="" loading="lazy" ${!demo&&g.launcher==='steam'?`data-fallback-cover="${esc(cover(g,true))}"`:''} data-fallback="${esc(fallback(g))}">`; }
function hydrateIcons(root = document) {
 hydratePlatformLogos(root);
 root.querySelectorAll('[data-icon]').forEach(el => el.innerHTML = icon(el.dataset.icon));
 root.querySelectorAll('img[data-fallback]').forEach(img=>{
  if(img.dataset.fallbackBound)return;
  img.dataset.fallbackBound='true';
  const recover=()=>{
   if(img.dataset.fallbackCover&&!img.dataset.triedFallbackCover){img.dataset.triedFallbackCover='true';img.src=img.dataset.fallbackCover;}
   else{img.removeEventListener('error',recover);img.src=img.dataset.fallback;}
  };
  img.addEventListener('error',recover);
  if(img.complete&&!img.naturalWidth)recover();
 });
}
function toast(text) { $('toast').textContent = text; $('toast').hidden = false; clearTimeout(toastTimer); toastTimer = setTimeout(() => $('toast').hidden = true, 5000); }
function notice(text = '') { $('notice').textContent = text; $('notice').hidden = !text; }
async function api(path, body, options = {}) {
 let response;
 const timeout = options.timeout ?? (/^\/api\/(?:mods|platforms)(?:[/?]|$)/.test(path) ? null : path === '/api/riot/accounts/sync' ? 120000 : 60000);
 try { response = await fetch(path, {
   cache: 'no-store',
   ...(timeout === null ? {} : {signal: AbortSignal.timeout(timeout)}),
   ...(body === undefined ? {} : {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)})
 }); } catch (error) {
   if (error?.name === 'TimeoutError' || error?.name === 'AbortError') {
     throw new Error('SWCH не ответил вовремя. Обнови список аккаунтов и проверь результат операции.');
   }
   throw new Error('Нет соединения с сервером SWCH. Перезапусти SWCH и повтори действие.');
 }
 const isJson = Boolean(response.headers.get('content-type')?.includes('application/json'));
 const data = isJson ? await response.json().catch(() => undefined) : undefined;
 if (!response.ok || data === undefined) {
   if (path.startsWith('/api/epic/auth/browser/') && [404,405].includes(response.status)) {
     throw new Error('Запущен сервер SWCH без нового входа Epic. Обнови сборку и перезапусти SWCH. Подключение по коду доступно ниже.');
   }
   if (response.ok && !isJson) {
     throw new Error('SWCH бэкенд обновлен. Перезагрузи страницу (Cmd+R) или перезапусти SWCH Hub.');
   }
   const error=new Error(data?.error || `Сервис недоступен (${response.status}). Проверь запуск SWCH и повтори.`);error.status=response.status;throw error;
 }
 return data;
}
function demoData() {
 accounts = [
  {id:'demo-nova',username:'nova',display_name:'nova',launcher:'steam',is_active:true},
  {id:'demo-shadow',username:'shadow',display_name:'shadow',launcher:'steam',is_active:false},
  {id:'demo-epic',username:'nova.epic',display_name:'nova.epic',launcher:'epic',is_active:true},
  {id:'demo-roblox',account_id:'demo-roblox',user_id:'12345678',username:'BloxMaster',display_name:'BloxMaster',launcher:'roblox',is_active:true,robux_balance:1450,is_premium:true,rap:28500,headshot_url:'https://tr.rbxcdn.com/30DAY-AvatarHeadshot-9150B1B3674640EC3B1ECBE1FA8B6100-Png/150/150/AvatarHeadshot/Webp/noFilter'},
  {id:'demo-minecraft',account_id:'demo-minecraft',user_id:'demo-uuid-steve',username:'SteveCraft',display_name:'SteveCraft',launcher:'minecraft',is_active:true,account_type:'msa',avatar_url:'https://mc-heads.net/avatar/SteveCraft/64'},
  {id:'demo-tlauncher',account_id:'demo-tlauncher',user_id:'demo-uuid-alex',username:'Alex_TL',display_name:'Alex_TL',launcher:'minecraft',is_active:false,account_type:'tlauncher_free',avatar_url:'https://mc-heads.net/avatar/Alex_TL/64'}
 ];
 minecraftProfiles = [
  {id:'latest-release',name:'Latest Release (1.21.1)',profile_type:'latest-release',version_id:'1.21.1',icon:null},
  {id:'fabric-1.21',name:'Fabric Loader 1.21.1',profile_type:'custom',version_id:'fabric-loader-0.16.5-1.21.1',icon:null}
 ];
 const specs = [['730','Counter-Strike 2','Тактический шутер · Командная игра',true],['570','Dota 2','Стратегия · Командная игра',true],['1091500','Cyberpunk 2077','Ролевая игра · Открытый мир',true],['1245620','ELDEN RING','Ролевая игра · Приключение',true],['271590','Grand Theft Auto V','Экшен · Открытый мир',false],['1172470','Apex Legends','Шутер · Королевская битва',false]];
 games = specs.map(([app_id,name,description,is_installed]) => ({id:app_id,app_id,name,description,is_installed,launcher:'steam',size_bytes:58e9,banner_url:`https://cdn.akamai.steamstatic.com/steam/apps/${app_id}/library_600x900.jpg`,hero_url:`https://cdn.akamai.steamstatic.com/steam/apps/${app_id}/library_hero.jpg`}));
 games.push({id:'demo-fortnite',app_id:'Fortnite',name:'Fortnite',description:'Королевская битва · Творчество',launcher:'epic',is_installed:true,owner_id:'demo-epic',size_bytes:72e9});
 games.push({id:'demo-roblox-app',app_id:'roblox',name:'Roblox Player',description:'Официальный клиент Roblox',launcher:'roblox',is_installed:true,banner_url:'https://images.rbxcdn.com/e2e50cf605d15c83258c8ec6d81e05d0.png',hero_url:'https://images.rbxcdn.com/e2e50cf605d15c83258c8ec6d81e05d0.png'});
 games.push({id:'demo-roblox-place',app_id:'9285238704',name:'Blox Fruits',description:'Популярный плейс Roblox',launcher:'roblox',is_installed:true,banner_url:'https://tr.rbxcdn.com/180DAY-50a552bf36fec916f1f4438df3c9fc2e/768/432/Image/Webp/noFilter',hero_url:'https://tr.rbxcdn.com/180DAY-50a552bf36fec916f1f4438df3c9fc2e/768/432/Image/Webp/noFilter'});
 games.push({id:'minecraft_java',app_id:'minecraft_java',name:'Minecraft: Java Edition',description:'Песочница · Выживание и творчество',launcher:'minecraft',is_installed:true,banner_url:'https://launchercontent.mojang.com/gameDetails/javaEdition/hero.jpg',hero_url:'https://launchercontent.mojang.com/gameDetails/javaEdition/hero.jpg'});
 games.push({id:'minecraft_server_demo',app_id:'demo-hypixel',name:'Hypixel Network',description:'Сервер Minecraft · mc.hypixel.net',launcher:'minecraft',is_installed:true,banner_url:'https://launchercontent.mojang.com/gameDetails/javaEdition/hero.jpg',hero_url:'https://launchercontent.mojang.com/gameDetails/javaEdition/hero.jpg',namespace:'mc.hypixel.net'});
 for (const [gi,g] of games.entries()) for (const [ai,a] of accounts.entries()) if(g.launcher === a.launcher) {
   const key = metricKey(g,a); if (!Object.hasOwn(metrics,key)) metrics[key] = {skins:gi === 0 ? (ai ? 42 : 128) : 8+gi*7,value:gi === 0 ? (ai ? 380.5 : 1248.5) : 45+gi*32,hours:gi === 0 ? (ai ? 246 : 1240) : 21+gi*41};
 }
 accountRestrictions = new Map([
   ['demo-shadow', {
     account_id: 'demo-shadow',
     launcher: 'steam',
     community_banned: false,
     trade_banned: true,
     market_banned: true,
     vac_banned: true,
     vac_bans_count: 1,
     game_bans_count: 0,
     economy_ban: 'banned',
     is_limited: true,
     game_bans: [{ app_id: '730', name: 'Counter-Strike 2', ban_type: 'VAC' }],
     updated_at: new Date().toISOString()
   }]
 ]);
 selectedGame = selectedGame || 'steam:730';
}
async function refresh(force = false) {
 if((busy && !force) || riotSyncBusy) return; busy = true; loading = true; render(); notice();
 document.querySelector('[data-action="refresh"]').disabled = true;
 try {
   if(demo) { demoData(); $('connection-state').textContent='Демо-пространство'; $('footer-status').textContent='Демонстрация · пример библиотеки'; }
   else {
     const results = await Promise.allSettled([
       api('/api/steam/library'),
       api('/api/epic/accounts'),
       api('/api/accounts/restrictions'),
       api('/api/riot/library'),
       api('/api/riot/accounts'),
       api('/api/roblox/library'),
       api('/api/roblox/accounts'),
       api('/api/roblox/settings'),
       api('/api/minecraft/library'),
       api('/api/riot/sessions'),
       api('/api/ubisoft/library'),
       api('/api/gog/library'),
       api('/api/hoyoverse/library')
     ]);
     const errors = [];
     if(results[0].status === 'fulfilled') {
       applySteam(results[0].value);
       if(results[0].value.warnings?.length) notice(results[0].value.warnings.join(' '));
     } else errors.push('Steam');
     if(results[1].status === 'fulfilled') {
       const data = results[1].value;
       const epic = (data.accounts || []).map(a=>({...a,id:a.account_id,launcher:'epic',is_active:a.account_id === data.active_account_id}));
       accounts = [...accounts.filter(a=>a.launcher !== 'epic'),...epic];
       const libraries = await Promise.allSettled(epic.map(async a => normalizeEpic(await api(`/api/epic/library?account_id=${encodeURIComponent(a.id)}`), a.id)));
       const loaded = libraries.flatMap((result,index) => {
         if(result.status === 'fulfilled') return [result.value];
         errors.push(`Библиотека Epic: ${epic[index].display_name}`);
         const id=epic[index].id;
         return [games.filter(g=>g.launcher==='epic'&&(g.owner_ids||[g.owner_id]).includes(id)).map(g=>({...g,owner_id:id,owner_ids:[id]}))];
       });
       games = [...games.filter(g=>g.launcher !== 'epic'), ...mergeLibraries(loaded)];
     } else errors.push('Аккаунты Epic Games');
     if(results[2]?.status === 'fulfilled' && Array.isArray(results[2].value?.restrictions)) {
       accountRestrictions = new Map(results[2].value.restrictions.map(r => [r.account_id, r]));
     }
     if(results[3]?.status==='fulfilled' && Array.isArray(results[3].value?.games)) {
       riotStatus=results[3].value;
       games=[...games.filter(g=>g.launcher!=='riot'),...riotStatus.games.map(g=>({...g,description:'Запуск игры через Riot Client с выбранным аккаунтом.'}))];
     } else {riotStatus=null;errors.push('Riot Games');}
     if((results[4]?.status==='fulfilled' && results[4].value) || (results[9]?.status==='fulfilled' && results[9].value)) {
       const payload = results[4]?.value || {};
       if (results[9]?.status==='fulfilled' && Array.isArray(results[9].value?.sessions)) {
         payload.sessions = results[9].value.sessions;
       }
       applyRiotProfiles(payload);
     } else errors.push('Профили Riot');
     if(results[5]?.status==='fulfilled' && results[5].value?.games) {
       robloxStatus = results[5].value;
       const robloxGames = robloxStatus.games.map(g => ({...g, launcher: 'roblox'}));
       games = [...games.filter(g => g.launcher !== 'roblox'), ...robloxGames];
       const robloxCountEl = $('roblox-count');
       if (robloxCountEl) robloxCountEl.textContent = String(robloxGames.length);
     } else { robloxStatus = null; errors.push('Roblox'); }
     if(results[6]?.status==='fulfilled' && Array.isArray(results[6].value)) {
       robloxAccounts = results[6].value;
       const mapped = robloxAccounts.map(a => ({
         ...a,
         id: a.account_id,
         launcher: 'roblox',
         rap: a.rap_value || 0,
       }));
       accounts = [...accounts.filter(a => a.launcher !== 'roblox'), ...mapped];
     } else errors.push('Аккаунты Roblox');
     if(results[7]?.status==='fulfilled') {
       const toggle = $('roblox-multi-instance-toggle');
       if (toggle && results[7].value?.multi_instance_enabled !== undefined) {
         toggle.checked = Boolean(results[7].value.multi_instance_enabled);
       }
     }
     if(results[8]?.status==='fulfilled' && results[8].value) {
       minecraftStatus = results[8].value;
       const mcGames = (minecraftStatus.games || []).map(g => ({...g, launcher: 'minecraft'}));
       games = [...games.filter(g => g.launcher !== 'minecraft'), ...mcGames];
       minecraftProfiles = minecraftStatus.profiles || [];
       minecraftServers = minecraftStatus.servers || [];
       minecraftWorlds = minecraftStatus.worlds || [];
       minecraftAccounts = (minecraftStatus.accounts || []).map(a => ({
         ...a,
         id: a.id || a.account_id,
         launcher: 'minecraft',
         display_name: a.username,
       }));
       accounts = [...accounts.filter(a => a.launcher !== 'minecraft'), ...minecraftAccounts];
       const mcCountEl = $('minecraft-count');
       if (mcCountEl) mcCountEl.textContent = String(mcGames.length);
     } else { minecraftStatus = null; errors.push('Minecraft'); }
     if(results[10]?.status==='fulfilled' && Array.isArray(results[10].value?.games)) {
       ubisoftStatus=results[10].value;
       games=[...games.filter(g=>g.launcher!=='ubisoft'),...ubisoftStatus.games];
       accounts=[...accounts.filter(a=>a.launcher!=='ubisoft'),...(ubisoftStatus.accounts||[])];
     } else {ubisoftStatus=null;errors.push('Ubisoft Connect');}
     if(results[11]?.status==='fulfilled'&&Array.isArray(results[11].value?.games)) {
       gogStatus=results[11].value;
       games=[...games.filter(g=>g.launcher!=='gog'),...gogStatus.games];
       accounts=[...accounts.filter(a=>a.launcher!=='gog'),...(gogStatus.accounts||[])];
       if(gogAccountFilter!=='all'&&!accounts.some(a=>a.launcher==='gog'&&a.id===gogAccountFilter))gogAccountFilter='all';
     }else{gogStatus=null;errors.push('GOG GALAXY');}
     if(results[12]?.status==='fulfilled' && Array.isArray(results[12].value?.games)) {
       hoyoverseStatus = results[12].value;
       games = [...games.filter(g => g.launcher !== 'hoyoverse'), ...hoyoverseStatus.games];
       accounts = [...accounts.filter(a => a.launcher !== 'hoyoverse'), ...(hoyoverseStatus.accounts || [])];
       const hoyoCountEl = $('hoyoverse-count');
       if (hoyoCountEl) hoyoCountEl.textContent = String(hoyoverseStatus.games.length);
       const hoyoTabBadge = $('hoyoverse-tab-badge');
       if (hoyoTabBadge) hoyoTabBadge.textContent = String((hoyoverseStatus.accounts || []).length);
     } else { hoyoverseStatus = null; errors.push('HoYoverse'); }
     try {
       const extensions=await loadModLibrary();
       games=[...games.filter(g=>!isModPlatform(g.launcher)),...extensions.games];
       accounts=[...accounts.filter(a=>!isModPlatform(a.launcher)),...extensions.accounts];
     } catch { /* Older backends and offline extensions do not hide built-in libraries. */ }
     if(platform!=='all'&&!games.some(g=>g.launcher===platform)&&!(page==='accounts'&&accounts.some(a=>a.launcher===platform)))platform='all';
     const partial = errors.length > 0;
     // Preserve the previous snapshot if every request failed. Successful empty
     // responses still replace removed accounts/games in the next snapshot.
     if(results.some(result=>result.status==='fulfilled'))persistLibraryCache();
     $('connection-state').textContent = partial ? 'Нет полной связи' : 'Подключено';
     $('footer-status').textContent = partial ? 'Не все данные загружены' : 'Библиотека обновлена · ' + new Date().toLocaleTimeString(getLocale(),{hour:'2-digit',minute:'2-digit'});
     $('footer-dot').style.background = partial ? '#d9ad6a' : '#8cba9c';
     if(partial) notice(`Не удалось загрузить: ${errors.join(', ')}. Проверь запуск локального сервиса SWCH и нажми «Обновить». Доступные данные сохранены на экране.`);
   }
 } catch { notice('Не удалось прочитать библиотеку. Проверь локальный сервис и повтори загрузку.'); }
 finally { busy=false;loading=false;document.querySelector('[data-action="refresh"]').disabled=false;render();if($('game-modal')?.open&&$('game-modal').dataset.game)openGameModal($('game-modal').dataset.game);void hydratePlaytime(force); }
}
function getEffectiveAccentId() {
  return (!hasSavedAccent && accentColor === 'orange' && design === 'apple') ? 'apple-blue' : accentColor;
}
function renderDesignOptions() {
  const isMac = isMacPlatform();
  design = resolveDesign(savedDesign, isMac);
  document.querySelectorAll('[data-design-choice]').forEach(b => {
    const choice = b.dataset.designChoice;
    const isSelected = choice === design;
    const locked = Boolean(DESIGNS[choice]?.macOnly && !isMac);
    b.classList.toggle('selected', isSelected);
    b.classList.toggle('mac-locked', locked);
    b.disabled = locked;
    b.setAttribute('aria-pressed', String(isSelected));
    if (locked) b.setAttribute('aria-disabled', 'true');
    else b.removeAttribute('aria-disabled');
  });
  const badgeEl = $('design-platform-badge');
  if (badgeEl) {
    badgeEl.textContent = t(isMac ? 'macOS HIG' : 'Только для Mac');
    badgeEl.classList.toggle('is-mac', isMac);
  }
  const noteEl = $('design-mac-note');
  if (noteEl) {
    noteEl.textContent = t(
      isMac
        ? 'На ваш Mac можно применить нативный дизайн в стиле Apple или классический интерфейс SWCH.'
        : 'Дизайн в стиле Apple доступен только на компьютерах Mac (macOS).'
    );
  }
}
function applyStyle() {
 const isMac = isMacPlatform();
 design = resolveDesign(savedDesign, isMac);
 document.documentElement.dataset.style=style;
 document.documentElement.dataset.design=design;
 document.documentElement.classList.toggle('platform-macos', isMac);
 document.documentElement.classList.toggle('apple-design', design === 'apple');
 const effectiveAccent = getEffectiveAccentId();
 const activeColor = effectiveAccent === 'custom'
   ? {hex: customAccentColor, rgb: customAccentColor.slice(1).match(/.{2}/g).map(channel => parseInt(channel, 16)).join(', ')}
   : ACCENT_COLORS.find(c => c.id === effectiveAccent) || ACCENT_COLORS[0];
 if (false) {
 document.documentElement.dataset.style=style;
 const activeColor = accentColor === 'custom'
   ? {hex: customAccentColor, rgb: customAccentColor.slice(1).match(/.{2}/g).map(channel => parseInt(channel, 16)).join(', ')}
   : ACCENT_COLORS.find(c => c.id === accentColor) || ACCENT_COLORS[0];
 }
 document.documentElement.style.setProperty('--accent', activeColor.hex);
 document.documentElement.style.setProperty('--accent-rgb', activeColor.rgb);
 const modeName = t(layout === 'cards' ? 'Карточки' : 'Список');
 const labelEl = $('style-label');
 if (labelEl) labelEl.textContent = `Настройки (${modeName})`;
 renderDesignOptions();
 document.querySelectorAll('[data-layout]').forEach(b=> { b.classList.toggle('active',b.dataset.layout===layout);b.setAttribute('aria-pressed',String(b.dataset.layout===layout)); });
 document.querySelectorAll('[data-layout-choice]').forEach(b=> {
   const isSelected = b.dataset.layoutChoice === layout;
   b.classList.toggle('selected', isSelected);
   b.setAttribute('aria-pressed', String(isSelected));
 });
}
function availableAccounts(g) { if(['roblox','minecraft','riot'].includes(g.launcher))return accounts.filter(a=>a.launcher===g.launcher); if(!demo && g.launcher==='steam')return eligibleSteamAccounts(g,accounts,steamRows); return accounts.filter(a=>a.launcher===g.launcher && (g.launcher==='steam' || (g.owner_ids ? g.owner_ids.includes(a.id) : (!g.owner_id || a.id===g.owner_id)))); }
function chosenAccount(g) { const list=availableAccounts(g); const unbanned=list.filter(a=>!accountHasGameBan(a,g.app_id,g.name,accountRestrictions)); const pool=unbanned.length?unbanned:list; return pool.find(a=>a.id===selectedAccounts[gameKey(g)]) || pool.find(a=>a.is_active) || pool[0]; }
function banButton(g) {
  if(g.launcher==='riot'||g.launcher==='roblox'||g.launcher==='minecraft')return '<span class="muted">—</span>';
  let isBanned = false;
  let title = 'Назначить в бан для аккаунта';
  if (steamAccountFilter !== 'all') {
    const acc = accounts.find(a => a.id === steamAccountFilter);
    if (acc) {
      isBanned = accountHasGameBan(acc, g.app_id, g.name, accountRestrictions);
      title = isBanned ? `Снять бан (${esc(acc.display_name || acc.username)})` : `Назначить в бан (${esc(acc.display_name || acc.username)})`;
    }
  } else {
    const ch = chosenAccount(g);
    if (ch && accountHasGameBan(ch, g.app_id, g.name, accountRestrictions)) {
      isBanned = true;
    }
    title = 'Назначить в бан для аккаунта';
  }
  return `<button class="card-ban-btn ${isBanned ? 'banned' : ''}" data-action="card-ban" data-game="${esc(gameKey(g))}" title="${title}" aria-label="${title}">${icon('ban')}</button>`;
}
const importedPlaytime=new Map();
function gameTime(g,accountId=null){return gamePlaytime(g,{accounts,steamRows,riotProfiles,records:[...importedPlaytime.values()].flat(),metrics,accountId,demo});}
function timeCaption(g,accountId=null){const time=gameTime(g,accountId);if(!time)return '';const scope=timeScopeLabel(time);return `<span class="game-time" title="${esc(time.source)}${time.accounts>1?' · '+time.accounts+' аккаунта':''}${time.cached?' · сохранённые данные':''}" data-playtime-seconds="${time.seconds}">${icon('clock')}${esc(timeLabel(time))}${scope?`<span class="game-time-scope">${esc(scope)}</span>`:''}</span>`;}
function displayedMetrics(g,a) {const time=gameTime(g,a?.id??null);return {...(a?(metrics[metricKey(g,a)]||{}):{}),hours:time?time.seconds/3600:null,time};}
let playtimeGeneration=0;
async function hydratePlaytime(force=false){
 if(demo)return;const generation=++playtimeGeneration;
 const scopes=[...accounts.filter(a=>a.launcher==='epic').map(a=>({launcher:'epic',account_id:a.id})),...['minecraft'].filter(p=>accounts.some(a=>a.launcher===p)||games.some(g=>g.launcher===p)).map(launcher=>({launcher})),{launcher:'gog'}];
 const valid=new Set(scopes.map(p=>p.launcher+':'+(p.account_id||'local')));
 for(const key of importedPlaytime.keys())if(!valid.has(key))importedPlaytime.delete(key);
 await Promise.allSettled(scopes.map(async scope=>{
  try{const params=new URLSearchParams({...scope,refresh:String(force)});const data=await api('/api/playtime?'+params);if(generation!==playtimeGeneration||!Array.isArray(data.records))return;importedPlaytime.set(scope.launcher+':'+(scope.account_id||'local'),data.records);
   if(['library','favorites'].includes(page)){renderLibrary();hydrateIcons();}const modal=$('game-modal');if(modal?.open&&modal.dataset.game)openGameModal(modal.dataset.game);
  }catch{} // Other platforms and saved statistics remain usable offline.
 }));
}

function getGamePriceInfo(g, a = null) {
  if(g.launcher==='ubisoft'||g.launcher==='gog')return {text:'Нет данных',isFree:false};
  if(g.launcher==='riot')return {text:'Нет данных',isFree:false};
  if(g.launcher==='roblox')return {text:'Бесплатно',isFree:true,cents:0,currency:currency};
  if(g.launcher==='minecraft')return {text:'Java Edition',isFree:false,cents:0,currency:currency};
  const accCurrency = a ? getAccountCurrency(a) : currency;
  if (g.is_free) {
    return { text: 'Бесплатно', isFree: true, cents: 0, currency: accCurrency || currency };
  }
  const appId = String(g.app_id);
  const nameKey = g.name ? g.name.toLowerCase() : '';
  const quote = gamePriceQuotes.get(`${accCurrency}:${appId}`)
    || gamePriceQuotes.get(`${currency}:${appId}`)
    || gamePriceQuotes.get(appId)
    || (nameKey ? (gamePriceQuotes.get(`${accCurrency}:${nameKey}`) || gamePriceQuotes.get(nameKey)) : null);

  if (quote) {
    if (quote.is_free || quote.price_cents === 0) return { text: 'Бесплатно', isFree: true, cents: 0, currency: quote.currency || currency };
    if (quote.price_cents != null) {
      if (quote.currency === accCurrency) {
        return { text: formatAccountMoney(quote.price_cents, accCurrency), isFree: false, cents: quote.price_cents, currency: accCurrency };
      }
      if (quote.currency === currency) {
        return { text: formatAccountMoney(quote.price_cents, currency), isFree: false, cents: quote.price_cents, currency };
      }
      if (quote.currency === 'USD' && currency !== 'USD') {
        const converted = convertUsdCents(quote.price_cents, currency, fx.rates);
        if (converted != null) {
          return { text: formatAccountMoney(converted, currency), isFree: false, cents: converted, currency };
        }
      }
      return { text: formatAccountMoney(quote.price_cents, quote.currency), isFree: false, cents: quote.price_cents, currency: quote.currency };
    }
  }
  return { text: 'Бесплатно', isFree: true, cents: 0, currency: accCurrency || currency };
}

function dateLabel(date) {
  const d = new Date(date);
  return Number.isNaN(d.valueOf()) ? 'нет даты' : d.toLocaleString(getLocale());
}

let currentFortniteSummary = null;
const fortniteSummaries = new Map();
let currentFortniteTab = 'all';
let currentFortniteSearch = '';
let currentFortniteRarity = 'all';
let currentEpicInventoryAccount = read('swch-epic-inventory-account', null);
let fortniteLoading = false;
let fortniteLoadingAccountId = null;
let fortniteError = null;

function getActiveFortniteSummary() {
  if (currentEpicInventoryAccount && fortniteSummaries.has(currentEpicInventoryAccount)) {
    return fortniteSummaries.get(currentEpicInventoryAccount);
  }
  if (currentFortniteSummary && currentFortniteSummary.account_id === currentEpicInventoryAccount) {
    return currentFortniteSummary;
  }
  return null;
}

const FORTNITE_CATEGORIES = [
  { id: 'outfit', label: 'Экипировки (Скины)', shortLabel: 'Скины', icon: '👕' },
  { id: 'backpack', label: 'Рюкзаки (Украшения на спину)', shortLabel: 'Рюкзаки', icon: '🎒' },
  { id: 'pickaxe', label: 'Кирки (Инструменты)', shortLabel: 'Кирки', icon: '⛏️' },
  { id: 'emote', label: 'Эмоции и танцы', shortLabel: 'Эмоции', icon: '💃' },
  { id: 'glider', label: 'Дельтапланы (Глайдеры)', shortLabel: 'Дельтапланы', icon: '🪂' },
  { id: 'wrap', label: 'Обёртки для оружия', shortLabel: 'Обёртки', icon: '🎨' },
  { id: 'music', label: 'Музыка лобби', shortLabel: 'Музыка', icon: '🎵' },
  { id: 'contrail', label: 'Следы падения', shortLabel: 'Следы', icon: '✨' },
  { id: 'loadingscreen', label: 'Экраны загрузки', shortLabel: 'Экраны', icon: '🖼️' },
  { id: 'other', label: 'Прочие предметы', shortLabel: 'Прочее', icon: '📦' },
];

function getDemoFortniteSummary() {
  return {
    account_id: 'demo-epic',
    display_name: 'nova.epic',
    country: 'US',
    total_vbucks: 2450,
    vbucks_purchased: 1500,
    vbucks_earned: 950,
    season_level: 68,
    account_level: 215,
    battle_pass_purchased: true,
    battle_pass_level: 68,
    has_save_the_world: true,
    is_stw_founder: true,
    lifetime_wins: 142,
    total_cosmetics: 17,
    outfits_count: 4,
    backpacks_count: 2,
    pickaxes_count: 2,
    gliders_count: 2,
    emotes_count: 2,
    wraps_count: 2,
    exclusive_count: 7,
    updated_at: Math.floor(Date.now() / 1000) - 300,
    cosmetics: [
      { id: 'cid_028_athenacharacter', name: 'Рыцарь Черепа (Skull Trooper)', cosmetic_type: 'outfit', rarity: 'epic', is_exclusive: true, is_favorite: true, variants_count: 3, image_url: 'https://fortnite-api.com/images/cosmetics/br/cid_028_athenacharacter_s_f/icon.png' },
      { id: 'cid_travis_scott', name: 'Трэвис Скотт (Travis Scott)', cosmetic_type: 'outfit', rarity: 'icon', is_exclusive: true, is_favorite: true, variants_count: 2, image_url: 'https://fortnite-api.com/images/cosmetics/br/cid_703_athenacharacter_s_f/icon.png' },
      { id: 'cid_john_wick', name: 'Джон Уик (John Wick)', cosmetic_type: 'outfit', rarity: 'legendary', is_exclusive: false, is_favorite: false, variants_count: 2, image_url: 'https://fortnite-api.com/images/cosmetics/br/cid_409_athenacharacter_s_f/icon.png' },
      { id: 'cid_black_widow', name: 'Чёрная Вдова (Black Widow Outfit)', cosmetic_type: 'outfit', rarity: 'marvel', is_exclusive: false, is_favorite: false, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/cid_397_athenacharacter_s_f/icon.png' },
      { id: 'bid_black_shield', name: 'Чёрный щит (Black Shield)', cosmetic_type: 'backpack', rarity: 'legendary', is_exclusive: true, is_favorite: true, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/bid_004_blackshield/icon.png' },
      { id: 'bid_fallen_wings', name: 'Крылья падшего (Fallen Wings)', cosmetic_type: 'backpack', rarity: 'shadow', is_exclusive: false, is_favorite: false, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/bid_205_fallenlove/icon.png' },
      { id: 'pickaxe_reaper', name: 'Жнец (Reaper)', cosmetic_type: 'pickaxe', rarity: 'rare', is_exclusive: true, is_favorite: true, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/pickaxe_id_015_halloween/icon.png' },
      { id: 'pickaxe_starwand', name: 'Звездопад (Star Wand)', cosmetic_type: 'pickaxe', rarity: 'rare', is_exclusive: false, is_favorite: false, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/pickaxe_id_178_starwand/icon.png' },
      { id: 'eid_floss', name: 'Флосс (Floss)', cosmetic_type: 'emote', rarity: 'rare', is_exclusive: true, is_favorite: true, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/eid_floss/icon.png' },
      { id: 'eid_fresh', name: 'Свежесть (Fresh)', cosmetic_type: 'emote', rarity: 'rare', is_exclusive: true, is_favorite: false, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/eid_fresh/icon.png' },
      { id: 'glider_mako', name: 'Мако (Mako Glider)', cosmetic_type: 'glider', rarity: 'uncommon', is_exclusive: true, is_favorite: false, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/glider_id_001/icon.png' },
      { id: 'glider_astroworld', name: 'Астропоезд (Astroworld Cyclone)', cosmetic_type: 'glider', rarity: 'icon', is_exclusive: true, is_favorite: false, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/glider_id_204_travisscott/icon.png' },
      { id: 'wrap_magma', name: 'Лавовый след (Magma)', cosmetic_type: 'wrap', rarity: 'epic', is_exclusive: false, is_favorite: false, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/wrap_042_magma/icon.png' },
      { id: 'wrap_slurp', name: 'Нектарный поток (Slurp)', cosmetic_type: 'wrap', rarity: 'slurp', is_exclusive: false, is_favorite: false, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/wrap_143_slurp/icon.png' },
      { id: 'music_og', name: 'Классический мотив (OG Classic)', cosmetic_type: 'music', rarity: 'rare', is_exclusive: true, is_favorite: true, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/musicpack_011_og/icon.png' },
      { id: 'contrail_lightning', name: 'Молния (Lightning)', cosmetic_type: 'contrail', rarity: 'rare', is_exclusive: false, is_favorite: false, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/trails_id_013_lightning/icon.png' },
      { id: 'ls_travis', name: 'Астрономический полёт (Astronomical)', cosmetic_type: 'loadingscreen', rarity: 'icon', is_exclusive: true, is_favorite: false, variants_count: 0, image_url: 'https://fortnite-api.com/images/cosmetics/br/lsid_228_cyclone/icon.png' },
    ]
  };
}

function normalizeFortniteCosmeticType(t) {
  const s = (t || '').trim().toLowerCase();
  if (s === 'skin' || s === 'character' || s === 'outfit') return 'outfit';
  if (s === 'backbling' || s === 'backpack') return 'backpack';
  if (s === 'harvestingtool' || s === 'pickaxe') return 'pickaxe';
  if (s === 'dance' || s === 'emote') return 'emote';
  if (s === 'glider') return 'glider';
  if (s === 'wrap') return 'wrap';
  if (s === 'music') return 'music';
  if (s === 'contrail') return 'contrail';
  if (s === 'loadingscreen' || s === 'loading_screen') return 'loadingscreen';
  return 'other';
}

function calculateFortniteLockerValuation(s) {
  if (!s) return { baseCents: 0, vbucksCents: 0, inGameBonusCents: 0, totalCents: 0 };
  const vbucksCents = Math.round((s.total_vbucks || 0) * 0.899); // ~$8.99 per 1000 VB
  let inGameBonusCents = 0;
  if (s.is_stw_founder) inGameBonusCents += 4000; // STW Founder daily V-Bucks pass ($40)
  else if (s.has_save_the_world) inGameBonusCents += 1500; // STW campaign ($15)
  if (s.battle_pass_purchased) inGameBonusCents += 950; // Current Battle Pass ($9.50)
  const exclusiveCount = s.exclusive_count || (s.cosmetics || []).filter(c => c.is_exclusive).length;
  if (exclusiveCount > 0) inGameBonusCents += exclusiveCount * 500; // $5.00 per rare exclusive
  const totalCents = vbucksCents + inGameBonusCents;
  return { baseCents: 0, vbucksCents, inGameBonusCents, totalCents };
}

function renderFortniteCosmeticCard(c) {
  const typeLabels = {
    outfit: 'Экипировка',
    backpack: 'Рюкзак',
    pickaxe: 'Кирка',
    glider: 'Дельтаплан',
    emote: 'Эмоция',
    wrap: 'Обёртка',
    music: 'Музыка',
    contrail: 'След падения',
    loadingscreen: 'Экран загрузки',
    other: 'Предмет'
  };

  const rarityLabels = {
    common: 'Обычный',
    uncommon: 'Необычный',
    rare: 'Редкий',
    epic: 'Эпический',
    legendary: 'Легендарный',
    mythic: 'Мифический',
    icon: 'Звёздная серия',
    marvel: 'Marvel',
    dc: 'DC',
    gaminglegends: 'Легенды видеоигр',
    starwars: 'Star Wars',
    shadow: 'Тёмная серия',
    frozen: 'Ледяная серия',
    lava: 'Лавовая серия',
    slurp: 'Нектарная серия'
  };

  const fallbackSvgUri = 'data:image/svg+xml;utf8,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120" viewBox="0 0 24 24" fill="none" stroke="#666" stroke-width="1.5"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>');

  const normType = normalizeFortniteCosmeticType(c.cosmetic_type);
  const rarityCls = (c.rarity || 'common').toLowerCase();
  const cardImgSrc = safeImage(c.image_url) || fallbackSvgUri;
  return `
    <article class="fortnite-card rarity-${esc(rarityCls)}" data-cosmetic-id="${esc(c.id)}" data-cosmetic-type="${esc(normType)}">
      <div class="fortnite-card-badges">
        ${c.is_exclusive ? '<span class="exclusive-badge">Эксклюзив</span>' : ''}
        ${c.is_favorite ? '<span class="fav-badge">★</span>' : ''}
        ${c.variants_count > 0 ? `<span class="variant-badge">🎨 ${c.variants_count}</span>` : ''}
      </div>
      <img class="fortnite-card-img" src="${esc(cardImgSrc)}" alt="${esc(c.name)}" loading="lazy" data-fallback="${esc(fallbackSvgUri)}" onerror="this.onerror=null;this.src='${fallbackSvgUri}';">
      <div class="fortnite-card-info">
        <h4 class="fortnite-card-title" title="${esc(c.name)}">${esc(c.name)}</h4>
        <div class="fortnite-card-meta">
          <span>${esc(typeLabels[normType] || c.cosmetic_type)}</span>
          <span>${esc(rarityLabels[rarityCls] || c.rarity)}</span>
        </div>
      </div>
    </article>
  `;
}

function renderFortniteSectionsHtml(s) {
  if (!s) return '';
  const search = (currentFortniteSearch || '').trim().toLowerCase();
  const rarityFilter = (currentFortniteRarity || 'all').trim().toLowerCase();
  const tab = currentFortniteTab || 'all';

  const matchesFilter = c => {
    const normType = normalizeFortniteCosmeticType(c.cosmetic_type);
    if (tab === 'exclusive' && !c.is_exclusive) return false;
    if (tab !== 'all' && tab !== 'exclusive' && normType !== tab) return false;
    if (rarityFilter !== 'all' && (c.rarity || '').toLowerCase() !== rarityFilter) return false;
    if (search && !c.name.toLowerCase().includes(search) && !c.id.toLowerCase().includes(search)) return false;
    return true;
  };

  const filtered = (s.cosmetics || []).filter(matchesFilter);

  const categoryGroups = [];
  for (const cat of FORTNITE_CATEGORIES) {
    const itemsInCat = filtered.filter(c => normalizeFortniteCosmeticType(c.cosmetic_type) === cat.id);
    if (itemsInCat.length > 0) {
      categoryGroups.push({ ...cat, items: itemsInCat });
    }
  }

  if (!categoryGroups.length) {
    const isFiltered = Boolean(search || rarityFilter !== 'all' || (tab !== 'all' && tab !== 'exclusive'));
    return `
      <div class="fortnite-loading">
        <p style="font-size:15px;font-weight:600;margin-bottom:6px">${isFiltered ? 'Предметы не найдены' : 'В шкафчике пока нет предметов'}</p>
        <small class="muted">${isFiltered ? 'Попробуй изменить фильтр категории, редкости или поисковый запрос' : 'Синхронизируй профиль Fortnite или войди в игру, чтобы получить экипировку'}</small>
      </div>
    `;
  }

  return `
    <div class="fortnite-categories-container">
      ${categoryGroups.map(cat => `
        <section class="fortnite-category-section" id="fortnite-sec-${esc(cat.id)}">
          <div class="fortnite-section-header">
            <div class="fortnite-section-title">
              <span class="fortnite-section-icon">${cat.icon}</span>
              <h3>${esc(cat.label)}</h3>
              <span class="fortnite-section-count">${cat.items.length}</span>
            </div>
            <small class="muted" style="font-size:11px">Все предметы раздела «${esc(cat.shortLabel)}»</small>
          </div>
          <div class="fortnite-grid">
            ${cat.items.map(c => renderFortniteCosmeticCard(c)).join('')}
          </div>
        </section>
      `).join('')}
    </div>
  `;
}

function updateFortniteItems() {
  const s = getActiveFortniteSummary() || currentFortniteSummary;
  if (!s) return;
  const modalContent = $('fortnite-modal-content');
  if (modalContent) modalContent.innerHTML = renderFortniteSectionsHtml(s);
  const invContent = $('inventory-fortnite-content');
  if (invContent) invContent.innerHTML = renderFortniteSectionsHtml(s);
  if (!modalContent && !invContent) {
    if ($('fortnite-dialog')?.open) renderFortniteLocker();
    if (page === 'inventory' && inventoryLauncher === 'epic') renderInventory();
  }
}

if (typeof window !== 'undefined') {
  window.updateFortniteItems = updateFortniteItems;
  window.getDemoFortniteSummary = getDemoFortniteSummary;
  try {
    Object.defineProperty(window, 'currentFortniteSummary', {
      configurable: true,
      get() { return currentFortniteSummary; },
      set(v) {
        currentFortniteSummary = v;
        if (v && v.account_id) fortniteSummaries.set(v.account_id, v);
      }
    });
  } catch {}
}

function renderFortniteLockerBodyHtml(s, { isModal = true, accountId = '' } = {}) {
  if (!s) return '';
  const valuation = calculateFortniteLockerValuation(s);
  const cacheDateStr = s.updated_at
    ? dateLabel(s.updated_at > 1e11 ? s.updated_at : s.updated_at * 1000)
    : 'только что';

  const allCosmetics = s.cosmetics || [];
  const outfitsCount = s.outfits_count || allCosmetics.filter(c => normalizeFortniteCosmeticType(c.cosmetic_type) === 'outfit').length;
  const backpacksCount = s.backpacks_count || allCosmetics.filter(c => normalizeFortniteCosmeticType(c.cosmetic_type) === 'backpack').length;
  const pickaxesCount = s.pickaxes_count || allCosmetics.filter(c => normalizeFortniteCosmeticType(c.cosmetic_type) === 'pickaxe').length;
  const emotesCount = s.emotes_count || allCosmetics.filter(c => normalizeFortniteCosmeticType(c.cosmetic_type) === 'emote').length;
  const glidersCount = s.gliders_count || allCosmetics.filter(c => normalizeFortniteCosmeticType(c.cosmetic_type) === 'glider').length;
  const wrapsCount = s.wraps_count || allCosmetics.filter(c => normalizeFortniteCosmeticType(c.cosmetic_type) === 'wrap').length;
  const exclusiveCount = s.exclusive_count || allCosmetics.filter(c => c.is_exclusive).length;

  const tabs = [
    { id: 'all', label: 'Все категории', count: allCosmetics.length || s.total_cosmetics || 0 },
    { id: 'outfit', label: 'Скины', count: outfitsCount },
    { id: 'backpack', label: 'Рюкзаки', count: backpacksCount },
    { id: 'pickaxe', label: 'Кирки', count: pickaxesCount },
    { id: 'emote', label: 'Эмоции', count: emotesCount },
    { id: 'glider', label: 'Дельтапланы', count: glidersCount },
    { id: 'wrap', label: 'Обёртки', count: wrapsCount },
    { id: 'exclusive', label: '⭐ Эксклюзивы', count: exclusiveCount },
  ];

  const rarityFilter = (currentFortniteRarity || 'all').trim().toLowerCase();
  const tab = currentFortniteTab || 'all';

  const searchId = isModal ? 'fortnite-search-input' : 'inventory-fortnite-search';
  const rarityId = isModal ? 'fortnite-rarity-select' : 'inventory-fortnite-rarity';
  const contentId = isModal ? 'fortnite-modal-content' : 'inventory-fortnite-content';

  return `
    <div class="fortnite-hero-banner">
      <div class="fortnite-hero-card">
        <small>Оценка шкафчика (${esc(currency)})</small>
        <strong class="fortnite-vbucks-value" style="color:var(--accent)!important">${money(valuation.totalCents)}</strong>
        <span class="muted" style="font-size:11px">Формула: ${format.format(s.total_vbucks || 0)} VB (${money(valuation.vbucksCents)}) + внутриигровая валюта и бонусы (${money(valuation.inGameBonusCents)})</span>
      </div>
      <div class="fortnite-hero-card">
        <small>Баланс V-Bucks</small>
        <strong class="fortnite-vbucks-value">🪙 ${format.format(s.total_vbucks || 0)}</strong>
        <span class="muted" style="font-size:11px">Куплено: ${format.format(s.vbucks_purchased || 0)} · Заработано: ${format.format(s.vbucks_earned || 0)}</span>
      </div>
      <div class="fortnite-hero-card">
        <small>Уровень и боевой пропуск</small>
        <strong>Общий ур. ${s.account_level || s.season_level || 1}</strong>
        <span class="muted" style="font-size:11px">Сезон: ${s.season_level || 1} ур. · ${s.battle_pass_purchased ? '✓ БП куплен (' + (s.battle_pass_level || 1) + ' ур.)' : '○ Без БП'}</span>
      </div>
      <div class="fortnite-hero-card">
        <small>Сражение с бурей (ПВЕ валюта)</small>
        <strong class="${s.is_stw_founder ? 'fortnite-stw-founder' : ''}">${s.is_stw_founder ? '🛡️ Основатель' : s.has_save_the_world ? 'Доступ к ПВЕ' : 'Без ПВЕ'}</strong>
        <span class="muted" style="font-size:11px">${s.is_stw_founder ? 'Ежедневный фарм V-Bucks (+$40)' : s.has_save_the_world ? 'Кампания ПВЕ (+$15)' : 'Не приобретено'}</span>
      </div>
    </div>

    <div class="fortnite-cache-notice">
      <small class="muted">🕒 Информация о шкафчике актуальна на: <strong>${esc(cacheDateStr)}</strong> (сохранено в кэше) · <button type="button" class="text-button" data-action="fortnite-refresh" data-account="${esc(accountId || s.account_id)}">Обновить сейчас ↻</button></small>
    </div>

    <div class="fortnite-counts-bar">
      <span>Всего предметов: <strong>${format.format(allCosmetics.length || s.total_cosmetics || 0)}</strong></span>
      <span>·</span>
      <span>Скины: <strong>${format.format(outfitsCount)}</strong></span>
      <span>·</span>
      <span>Рюкзаки: <strong>${format.format(backpacksCount)}</strong></span>
      <span>·</span>
      <span>Кирки: <strong>${format.format(pickaxesCount)}</strong></span>
      <span>·</span>
      <span>Эмоции: <strong>${format.format(emotesCount)}</strong></span>
      <span>·</span>
      <span>Дельтапланы: <strong>${format.format(glidersCount)}</strong></span>
      <span>·</span>
      <span>Обёртки: <strong>${format.format(wrapsCount)}</strong></span>
      <span>·</span>
      <span style="color:#f59e0b">Эксклюзивов: <strong>${format.format(exclusiveCount)}</strong></span>
    </div>

    <div class="fortnite-tabs">
      ${tabs.map(t => `
        <button type="button" class="fortnite-tab-btn ${tab === t.id ? 'active' : ''}" data-fortnite-tab="${esc(t.id)}">
          ${esc(t.label)} <span class="fortnite-tab-count">${t.count}</span>
        </button>
      `).join('')}
    </div>

    <div class="fortnite-toolbar">
      <input type="search" class="fortnite-search-input" id="${searchId}" placeholder="Поиск по названию скина..." value="${esc(currentFortniteSearch)}">
      <select class="fortnite-filter-select" id="${rarityId}">
        <option value="all" ${rarityFilter === 'all' ? 'selected' : ''}>Все редкости</option>
        <option value="legendary" ${rarityFilter === 'legendary' ? 'selected' : ''}>Легендарные (Gold)</option>
        <option value="epic" ${rarityFilter === 'epic' ? 'selected' : ''}>Эпические (Purple)</option>
        <option value="rare" ${rarityFilter === 'rare' ? 'selected' : ''}>Редкие (Blue)</option>
        <option value="uncommon" ${rarityFilter === 'uncommon' ? 'selected' : ''}>Необычные (Green)</option>
        <option value="icon" ${rarityFilter === 'icon' ? 'selected' : ''}>Звёздная серия (Icon)</option>
        <option value="marvel" ${rarityFilter === 'marvel' ? 'selected' : ''}>Серия Marvel</option>
        <option value="gaminglegends" ${rarityFilter === 'gaminglegends' ? 'selected' : ''}>Легенды видеоигр</option>
        <option value="starwars" ${rarityFilter === 'starwars' ? 'selected' : ''}>Звёздные войны</option>
        <option value="dc" ${rarityFilter === 'dc' ? 'selected' : ''}>Серия DC</option>
      </select>
      <button type="button" class="secondary" data-action="fortnite-refresh" data-account="${esc(accountId || s.account_id)}">Обновить данные ↻</button>
    </div>

    <div id="${contentId}" class="fortnite-content-body">
      ${renderFortniteSectionsHtml(s)}
    </div>
  `;
}

async function ensureFortniteSummary(accountId, force = false) {
  const targetAcc = accounts.find(a => a.id === accountId) || accounts.find(a => a.launcher === 'epic');
  const accId = targetAcc ? targetAcc.id : (accountId || '');
  if (!accId) return;

  const cached = fortniteSummaries.get(accId);
  if (cached && !force) {
    currentFortniteSummary = cached;
    fortniteError = null;
    if (page === 'inventory' && inventoryLauncher === 'epic') renderInventory();
    return;
  }

  if (fortniteLoading && fortniteLoadingAccountId === accId && !force) return;

  fortniteLoading = true;
  fortniteLoadingAccountId = accId;
  fortniteError = null;
  if (page === 'inventory' && inventoryLauncher === 'epic') renderInventory();

  try {
    if (demo && (accId === 'demo-epic' || !accId)) {
      currentFortniteSummary = getDemoFortniteSummary();
      fortniteSummaries.set(accId, currentFortniteSummary);
    } else {
      const url = '/api/epic/fortnite/summary?account_id=' + encodeURIComponent(accId) + (force ? '&refresh=true' : '') + '&_t=' + Date.now();
      try {
        const data = await api(url);
        currentFortniteSummary = data;
        fortniteSummaries.set(accId, data);
      } catch (err) {
        if (demo) {
          currentFortniteSummary = getDemoFortniteSummary();
          fortniteSummaries.set(accId, currentFortniteSummary);
        } else {
          throw err;
        }
      }
    }
  } catch (err) {
    fortniteError = err.message || 'Ошибка загрузки шкафчика';
  } finally {
    fortniteLoading = false;
    fortniteLoadingAccountId = null;
    if (page === 'inventory' && inventoryLauncher === 'epic') renderInventory();
  }
}

async function openFortniteLocker(accountId, forceRefresh = false) {
  const dlg = $('fortnite-dialog');
  if (!dlg) return;

  const targetAcc = accounts.find(a => a.id === accountId) || accounts.find(a => a.launcher === 'epic');
  const accId = targetAcc ? targetAcc.id : (accountId || '');

  $('fortnite-dialog-title').textContent = 'Шкафчик и профиль Fortnite';
  $('fortnite-dialog-subtitle').textContent = targetAcc ? (targetAcc.display_name || targetAcc.username) : 'Загрузка…';
  if (!dlg.open) dlg.showModal();

  $('fortnite-modal-body').innerHTML = `
    <div class="fortnite-loading">
      <p style="font-size:16px;font-weight:600;margin-bottom:8px">Синхронизация с серверами Epic Games…</p>
      <p class="muted">Загружаем профиль, баланс V-Bucks, статус 2FA и список косметики из Microservice Client Platform (MCP)</p>
    </div>
  `;

  try {
    if (demo && (accId === 'demo-epic' || !accId)) {
      currentFortniteSummary = getDemoFortniteSummary();
      fortniteSummaries.set(accId, currentFortniteSummary);
    } else {
      const cached = fortniteSummaries.get(accId);
      if (cached && !forceRefresh) {
        currentFortniteSummary = cached;
      } else {
        const url = '/api/epic/fortnite/summary?account_id=' + encodeURIComponent(accId) + (forceRefresh ? '&refresh=true' : '') + '&_t=' + Date.now();
        try {
          currentFortniteSummary = await api(url);
          fortniteSummaries.set(accId, currentFortniteSummary);
        } catch (err) {
          if (demo) {
            currentFortniteSummary = getDemoFortniteSummary();
            fortniteSummaries.set(accId, currentFortniteSummary);
          } else throw err;
        }
      }
    }
    renderFortniteLocker();
  } catch (err) {
    $('fortnite-modal-body').innerHTML = `
      <div class="fortnite-loading" style="color:var(--danger,#ef4444)">
        <p style="font-size:16px;font-weight:700">Не удалось загрузить данные Fortnite</p>
        <p class="muted">${esc(err.message || 'Ошибка подключения к Epic API')}</p>
        <button class="secondary" style="margin-top:14px" data-action="fortnite-refresh" data-account="${esc(accId)}">Попробовать снова ↻</button>
      </div>
    `;
    hydrateIcons(dlg);
  }
}

function renderFortniteLocker() {
  const s = currentFortniteSummary;
  if (!s) return;
  const body = $('fortnite-modal-body');
  if (!body) return;

  $('fortnite-dialog-subtitle').textContent = `${s.display_name || s.account_id}${s.country ? ' · ' + s.country : ''} · Всего предметов: ${format.format(s.total_cosmetics || 0)}`;
  body.innerHTML = renderFortniteLockerBodyHtml(s, { isModal: true, accountId: s.account_id });

  const dlg = $('fortnite-dialog');
  if (dlg) hydrateIcons(dlg);
}

function openGameModal(key) {
  try {
    const g = games.find(x => gameKey(x) === key);
    if (!g) return;
    selectedGame = key;
    save(`${namespace}-selected-game`, key);
    recent[key] = Date.now();
    save(`${namespace}-recent`, recent);
    if (!demo && g.launcher === 'steam' && (g.is_free == null || g.has_inventory == null)) void ensureGameDetails(g);

    const modal = $('game-modal');
    const body = $('game-modal-body');
    if (!modal || !body) return;
    modal.dataset.game = key;
    const headerTitle = $('game-modal-header-title');
    if (headerTitle) headerTitle.textContent = g.name;

    const a = chosenAccount(g);
    const priceInfo = getGamePriceInfo(g, a);
    const m = displayedMetrics(g, a);
    const isFortnite = Boolean(g.name?.toLowerCase().includes('fortnite') || g.app_id?.toLowerCase().includes('fortnite'));
    const isDownloading = downloadInProgress(g);
    const desc = g.description || (g.is_installed
      ? 'Игра установлена на этом устройстве. Выбирай активный аккаунт и запускай игру.'
      : (isDownloading
        ? (g.update?.phase === 'paused' ? 'Загрузка приостановлена. Нажми кнопку для продолжения.' : 'Идёт загрузка и обновление в Steam. Игра запустится после завершения.')
        : (isFortnite
          ? 'Fortnite привязан к твоему аккаунту Epic Games. Ты можешь просмотреть шкафчик, V-Bucks и скины онлайн прямо в SWCH или установить клиент через Epic Games.'
          : 'Игра находится в твоей библиотеке. Установи её для запуска.')));

    body.innerHTML = `
      <div class="game-modal-hero playnite-style">
        <div class="game-modal-backdrop-blur" style="background-image:url('${esc(cover(g, true).replaceAll("'", '%27').replaceAll('"', '%22'))}')"></div>
        <div class="game-modal-backdrop-overlay"></div>
        <div class="game-modal-hero-content">
          <div class="game-modal-poster-col">
            ${downloadCover(g,'game-modal-poster-img')}
          </div>
          <div class="game-modal-info-col">
            <div class="game-modal-badges">
              <span class="badge" style="background:rgba(255,255,255,0.1);backdrop-filter:blur(4px);color:white;font-weight:700;font-size:11px;padding:4px 10px;border-radius:6px">${platformName(g.launcher).toUpperCase()}</span>
              <span class="badge" data-game-install-status style="background:${downloadInProgress(g)||g.update?.required?'rgba(245,158,11,0.15)':g.is_installed ? 'rgba(46,204,113,0.18)' : 'rgba(255,255,255,0.06)'};color:${downloadInProgress(g)||g.update?.required?'#e5ae60':g.is_installed ? '#2ecc71' : 'var(--muted)'};font-size:11px;padding:4px 10px;border-radius:6px">
                ${esc(gameInstallBadge(g))}
              </span>
              ${timeCaption(g,a?.id??null)}
            </div>
            <h1 id="game-modal-title">${esc(g.name)}</h1>
            <p class="game-modal-desc">${esc(desc)}</p>
            <div class="game-modal-price-strip">
              <div class="game-modal-price-box">
                <span class="game-modal-price-label">Стоимость игры:</span>
                <strong class="game-modal-price-val ${priceInfo.isFree ? 'is-free' : ''}">${esc(priceInfo.text)}</strong>
              </div>
              <div class="game-modal-size-box">
                <span class="muted">На диске:</span>
                <strong>${g.size_bytes > 0 ? format.format(g.size_bytes / 1024**3) + ' ГБ' : (isFortnite ? 'Онлайн-сервис' : 'Нет данных')}</strong>
              </div>
            </div>
            <div class="game-modal-actions">
              <button class="primary game-modal-launch-btn" data-action="quick-launch" data-game="${esc(gameKey(g))}" ${busy || !canRequestLaunch(g) || downloadState(g)?.phase==='requested' ? 'disabled' : ''}>
                ${icon('play')}${esc(playLabel(g))}
              </button>
              ${isFortnite ? `
            <button type="button" class="secondary btn-fortnite-locker" data-action="open-fortnite" data-account="${esc(a?.id || '')}">
              <i data-icon="diamond"></i>Шкафчик и профиль Fortnite
            </button>
          ` : ''}
              ${!g.is_installed && g.launcher === 'epic' && a ? `
            <button type="button" class="secondary" data-action="open-launcher" data-account="${esc(a.id)}">
              Открыть в Epic Games Store
            </button>
          ` : ''}
              ${favoriteButton(g)}
              ${banButton(g)}
            </div>
          </div>
        </div>
      </div>
      <div class="game-modal-details-scroll">
        <div data-game-update>${renderUpdate(g)}</div>
        ${renderGameModsButton(g)}${renderGameAccounts(g)}
      </div>
    `;
    hydrateIcons(modal);
    if (!modal.open) modal.showModal();
  } catch (err) {
    console.error('Error opening game modal:', err);
    toast('Ошибка открытия карточки: ' + (err?.message || err));
  }
}
function hero(g, detail=false) {
 const account=chosenAccount(g),m=displayedMetrics(g,account);
 return `<section class="hero"><div class="hero-art" style="background-image:url('${esc(cover(g,true).replaceAll("'",'%27').replaceAll('\"','%22'))}')"></div><div class="hero-copy"><div class="hero-kicker"><span class="status-dot"></span>${platformName(g.launcher).toUpperCase()}</div><h2>${esc(g.name)}</h2><p>${esc(g.description || (g.is_installed ? 'Игра установлена. Выбери аккаунт и возвращайся в игру.' : 'Игра в библиотеке. Установи её через оригинальный лаунчер.'))}</p><div class="hero-actions"><button class="primary" data-action="${detail ? 'launch' : 'open-game'}" data-game="${esc(gameKey(g))}" ${detail && (busy || !canRequestLaunch(g)) ? 'disabled' : ''}>${icon(detail ? 'play' : 'arrow')}${detail ? playLabel(g) : 'Открыть игру'}</button><small>${g.launcher==='riot'?'Текущий аккаунт в Riot Client':account ? esc(account.display_name || account.username || 'Аккаунт') : g.launcher==='riot'?'Аккаунт в Riot Client':'Нет аккаунта'}${timeCaption(g,account?.id??null)}</small></div></div>${!detail ? '<span class="hero-count">01 / SWCH</span>' : ''}</section>`;
}
function empty(title,message,action='reset',label='Сбросить фильтры') { return `<div class="empty-state"><i>${icon('game')}</i><h2>${title}</h2><p>${message}</p><div class="empty-actions"><button class="primary" data-action="${action}">${label}</button>${!demo && games.length===0 ? '<button class="secondary" data-action="demo">Посмотреть демо</button>' : ''}</div></div>`; }
function render() {
 applyStyle(); applyModTheme(); renderCurrency(); $('demo-label').hidden=!demo;
  $('library-count').textContent=games.length; $('all-count').textContent=games.length; $('installed-count').textContent=games.filter(g=>g.is_installed).length;
  renderPlatformNavigation(games, accounts, page, platform);
  const accEl = $('accounts-count'); if(accEl) accEl.textContent = accounts.length;
  updateConnectTabBadges();
 document.querySelectorAll('[data-page]').forEach(b=>{b.classList.toggle('active',b.dataset.page===page);b.setAttribute('aria-current',b.dataset.page===page?'page':'false');});
 document.querySelectorAll('[data-platform]').forEach(b=>{b.classList.toggle('selected',b.dataset.platform===platform);b.setAttribute('aria-pressed',String(b.dataset.platform===platform));});
 document.querySelectorAll('[data-filter]').forEach(b=>{b.classList.toggle('active',b.dataset.filter===installed);b.setAttribute('aria-pressed',String(b.dataset.filter===installed));});
 const titles={library:['Твоя библиотека','Разные платформы. Одна точка старта.','Библиотека'],favorites:['Твои любимые игры','Самое важное — всегда под рукой.','Избранное'],accounts:['Твои аккаунты','Выбор профиля и управление подключениями.','Аккаунты'],inventory:['Твой инвентарь','Предметы и скины твоих игровых аккаунтов.','Инвентарь']};
 const title=modPageTitle(page)||titles[page]||titles.library;
 $('page-title').innerHTML=esc(title[0])+'<span>.</span>'; $('page-subtitle').textContent=title[1];if($('breadcrumb'))$('breadcrumb').textContent=title[2];
 $('library-view').hidden=!['library','favorites'].includes(page); $('accounts-view').hidden=page!=='accounts'; $('inventory-view').hidden=page!=='inventory';
 renderSteamFilters();
 $('library-advanced').hidden = !['library','favorites'].includes(page) || (!accounts.some(a=>a.launcher===platform || (platform==='all'&&a.launcher==='steam')) && !['gog','hoyoverse'].includes(platform));
 $('hoyoverse-filters').hidden = platform!=='hoyoverse';
 const gogFilters=$('gog-filters');gogFilters.hidden=page!=='library'||platform!=='gog';
 if(!gogFilters.hidden){$('gog-account-filter').innerHTML='<option value="all">Все аккаунты</option>'+accounts.filter(a=>a.launcher==='gog').map(a=>`<option value="${esc(a.id)}" translate="no">${esc(a.display_name||a.username)}</option>`).join('');$('gog-account-filter').value=gogAccountFilter;}
 renderModPage(page);
 if(page==='accounts') renderAccounts(); else if(page==='inventory') renderInventory(); else if(['library','favorites'].includes(page)) renderLibrary();
 hydrateIcons();hydratePlatformLogos();
 if(gamepadManager?.isActive) gamepadManager.updateHUD();
}
function renderLibrary() {
 const container=$('library-content');
 if(loading && !games.length && !libraryCacheRestored) {container.innerHTML='<div class="empty-state" role="status"><h2>Собираем твою библиотеку…</h2><p>Загружаем игры и аккаунты.</p></div>';return;}
 const eligible=games.filter(g=>{
   if(g.launcher!=='steam')return true;
   if(steamAccountFilter!=='all'){
     const acc=accounts.find(a=>a.id===steamAccountFilter);
     if(acc && accountHasGameBan(acc,g.app_id,g.name,accountRestrictions)) return false;
   }
   return (steamAccountFilter==='all' && steamAccessFilter==='all') || steamRows.some(r=>r.app_id===g.app_id && (steamAccountFilter==='all'||r.account_id===steamAccountFilter) && (steamAccessFilter==='all'||r.access===steamAccessFilter));
 });
 const playtime=Object.fromEntries(games.map(g=>[gameKey(g),gameTime(g,g.launcher==='steam'&&steamAccountFilter!=='all'?steamAccountFilter:null)?.seconds/60]));
 const scoped=eligible.filter(g=>g.launcher!=='gog'||gogAccountFilter==='all'||g.owner_ids?.includes(gogAccountFilter));
 const filtered=filterGames(scoped,{platform,installed,query,sort,recent,favoriteKeys:favorites,playtime,favorites:page==='favorites'?favorites:null});
 if(!filtered.length) {
   if(platform==='gog'&&!query&&installed==='all'&&page==='library'){container.innerHTML=empty('GOG GALAXY',gogStatus?.accounts?.length?'Синхронизируй библиотеку GOG через управление аккаунтом.':'Подключи аккаунт GOG, чтобы увидеть купленные игры.','connect','Подключить аккаунт GOG');return;}
   if(platform==='riot'&&!query&&installed==='all'&&page==='library'){container.innerHTML=empty('Riot Games',riotStatus?.message||'Не удалось загрузить Riot Games. Обнови библиотеку.','refresh','Обновить');return;}
   if(platform==='roblox'&&!query&&installed==='all'&&page==='library'){container.innerHTML=empty('Roblox',robloxStatus?.message||'Аккаунты или плейсы Roblox пока не добавлены. Нажми «Подключить аккаунт», чтобы найти сессии или добавить плейсы.','connect','Подключить аккаунт');return;}
   if(platform==='minecraft'&&!query&&installed==='all'&&page==='library'){container.innerHTML=empty('Minecraft',minecraftStatus?.message||'Minecraft пока не обнаружен. Нажми «Подключить аккаунт», чтобы выполнить поиск или добавить сервер.','connect','Подключить аккаунт');return;}
   if(platform==='epic' && accounts.some(a=>a.launcher==='epic') && !games.some(g=>g.launcher==='epic')) {
     container.innerHTML = empty('Библиотека Epic Games не синхронизирована', 'Твой аккаунт подключен. Нажми «Синхронизировать игры», чтобы загрузить игры из Epic Games Store.', 'sync-epic-active', 'Синхронизировать игры');
     return;
   }
   container.innerHTML=games.length ? empty('Пока ничего не найдено',page==='favorites'?'Добавь игры в избранное с помощью сердечка на карточке.':'Попробуй другое название или измени фильтры.') : empty('Твои игры появятся здесь','Подключи аккаунт Epic Games или загрузи локальную библиотеку Steam в сервис SWCH.','connect','Подключить аккаунт');
   return;
 }
 if(!filtered.some(g=>gameKey(g)===selectedGame)) selectedGame=gameKey(filtered[0]);
 const selected=filtered.find(g=>gameKey(g)===selectedGame);
 if(!demo&&selected.launcher==='steam'&&(selected.is_free==null||selected.has_inventory==null))void ensureGameDetails(selected);
 if(layout==='cards') {
  container.innerHTML=`<div class="collection-heading"><h3>${page==='favorites'?'Избранные игры':'Вся коллекция'} <span> / ${filtered.length}</span></h3><span>Выбери, во что сыграешь сегодня</span></div><div class="games-grid">${filtered.map(g=>`<article class="game-card" data-download-game="${esc(gameKey(g))}"><button class="poster-button" data-action="open-game" data-game="${esc(gameKey(g))}" aria-label="Открыть ${esc(g.name)}">${downloadCover(g,'poster-art')}<span class="poster-overlay"></span><span class="poster-platform">${platformName(g.launcher).toUpperCase()}</span><span class="poster-name">${esc(g.name)}</span><span class="poster-open">↗</span></button><div class="card-caption"><div><h3>${esc(g.name)}</h3><small data-download-caption>${g.is_installed?'<span class="status-dot"></span>':''}${esc(gameState(g))}${timeCaption(g,g.launcher==='steam'&&steamAccountFilter!=='all'?steamAccountFilter:null)}</small></div><div style="display:flex;align-items:center;gap:4px">${banButton(g)}${favoriteButton(g)}</div></div><div data-card-download>${downloadInProgress(g)?downloadMetrics(g,true):''}</div><button class="primary card-play" data-action="quick-launch" data-game="${esc(gameKey(g))}" ${busy||!canRequestLaunch(g)||downloadState(g)?.phase==='requested'?'disabled':''}>${icon('play')}${esc(playLabel(g))}</button></article>`).join('')}</div>`;
 } else {
  const priceInfo = getGamePriceInfo(selected, chosenAccount(selected));
  container.innerHTML=`<div class="detail-layout"><nav class="game-list" aria-label="Выбор игры">${filtered.map(g=>`<button data-action="select-game" data-game="${esc(gameKey(g))}" class="${selectedGame===gameKey(g)?'selected':''}" aria-pressed="${selectedGame===gameKey(g)}">${imageTag(g,'')}<span><strong>${esc(g.name)}</strong><small>${platformName(g.launcher)} · ${esc(gameState(g))}${timeCaption(g,g.launcher==='steam'&&steamAccountFilter!=='all'?steamAccountFilter:null)}</small></span></button>`).join('')}</nav><div class="detail-panel">${STYLES[style].layout==='cards'?'<button class="text-button" data-action="back-cards">← Назад к карточкам</button>':''}${hero(selected,true)}${renderUpdate(selected)}${renderGameModsButton(selected)}${renderGameAccounts(selected)}<div class="game-info"><div>Стоимость<strong><span class="detail-price-box ${priceInfo.isFree?'is-free':''}">${esc(priceInfo.text)}</span></strong></div><div>Платформа<strong>${platformName(selected.launcher)}</strong></div><div>На диске<strong>${selected.size_bytes>0?format.format(selected.size_bytes/1024**3)+' ГБ':'Нет данных'}</strong></div><div>В избранном<strong>${favoriteButton(selected)}</strong></div><div>Блокировка<strong>${banButton(selected)}</strong></div></div></div></div>`;
 }
}
function favoriteButton(g) {const saved=favorites.includes(gameKey(g));return `<button class="favorite ${saved?'saved':''}" data-action="favorite" data-game="${esc(gameKey(g))}" aria-pressed="${saved}" aria-label="${saved?'Убрать из избранного':'В избранное'}: ${esc(g.name)}">${icon('heart')}</button>`;}
function renderGameAccounts(g) {
 if(g.launcher==='riot')return renderRiotGame(g);
 if(!demo && g.launcher==='steam')return renderSteamAccounts(g);
 const list=availableAccounts(g);
 const activeList=list.filter(acc=>!accountHasGameBan(acc, g.app_id, g.name, accountRestrictions));
 const bannedList=list.filter(acc=>accountHasGameBan(acc, g.app_id, g.name, accountRestrictions));
 const a=chosenAccount(g),m=displayedMetrics(g,a);
 const isCurrentBanned = a && accountHasGameBan(a, g.app_id, g.name, accountRestrictions);
 const banAlert = isCurrentBanned ? `<div class="game-ban-alert"><strong>⚠️ Внимание: на выбранном аккаунте блокировка в ${esc(g.name)}!</strong><p>На профиле ${esc(a.display_name||a.username)} обнаружен бан в этой игре. При запуске не будет доступа к сетевой игре или защищённым серверам.</p></div>` : '';
 const bannedDetails = bannedList.length ? `<details class="banned-accounts-toggle"><summary>Скрытые аккаунты с баном (${bannedList.length})</summary><div style="display:flex;flex-direction:column;gap:6px;margin-top:8px">${bannedList.map(acc=>`<div class="quick-ban-row"><div class="account-info">${accountAvatar(acc)}<div><strong>${esc(acc.display_name||acc.username)}</strong><small class="muted" style="display:block;color:#ff8373">⚠️ Заблокирован в этой игре</small></div></div>${isVerifiedGameBan(accountRestrictions.get(acc.id),{app_id:g.app_id,name:g.name})?'<span class="muted small-text">Подтверждено Steam</span>':`<button class="secondary account-ban-btn is-banned" data-action="toggle-game-ban" data-account="${esc(acc.id)}" data-game="${esc(gameKey(g))}">✓ Снять бан</button>`}</div>`).join('')}</div></details>` : '';
 return `<section class="account-section"><div class="account-heading"><div><h3>Аккаунты и статистика</h3><small>${g.launcher==='steam'?'Профили Steam · наличие игры на аккаунте проверяет Steam':'Аккаунты, которым принадлежит игра'}</small></div>${a?'<button class="text-button" data-action="metrics">Изменить данные ↗</button>':''}</div>${activeList.length?`<div class="account-selector">${activeList.map(acc=>{
   return `<button class="account-chip ${a?.id===acc.id?'selected':''}" data-action="select-account" data-account="${esc(acc.id)}" aria-pressed="${a?.id===acc.id}">${accountAvatar(acc)}<span>${esc(acc.display_name || acc.username || 'Аккаунт')}${acc.is_active?'<small>Активен</small>':''}</span></button>`;
 }).join('')}</div>`:(bannedList.length?'<p class="muted">Все аккаунты с доступом к игре скрыты из-за блокировок.</p>':'<p class="muted">Нет подключённых аккаунтов этой платформы.</p><button class="text-button" data-action="connect">Подключить аккаунт ↗</button>')}${bannedDetails}${banAlert}<div class="metrics">${[['skins','diamond','Скины'],['value','wallet','Инвентарь'],['hours','clock','Время в игре']].map(([key,ico,label])=>`<div class="metric"><span class="metric-label">${icon(ico)}${label}</span><strong>${m[key]==null?'—':(key==='value'?money(Math.round(m[key]*100)):format.format(m[key])+(key==='hours'?' ч':''))}</strong><small>${m[key]==null?'Нет данных':key==='hours'&&m.time?esc(m.time.source):demo?'Пример данных':'Введено вручную'}</small></div>`).join('')}</div><div class="account-actions">${a&&!a.is_active?`<button class="secondary" data-action="switch" data-account="${esc(a.id)}" ${busy?'disabled':''}>Сделать активным</button>`:''}${a?(isCurrentBanned?(isVerifiedGameBan(accountRestrictions.get(a.id),{app_id:g.app_id,name:g.name})?'<span class="muted small-text">Бан подтверждён Steam</span>':`<button class="secondary btn-ban-action" data-action="toggle-game-ban" data-account="${esc(a.id)}" data-game="${esc(gameKey(g))}">✓ Снять бан с ${esc(a.display_name||a.username)}</button>`):`<button class="secondary btn-ban-action" data-action="toggle-game-ban" data-account="${esc(a.id)}" data-game="${esc(gameKey(g))}">${icon('ban')}Назначить в бан для ${esc(a.display_name||a.username)}</button>`):''}<p>${demo?'Демо-режим: значения приведены для примера.':a?.is_active?'Запуск с активным аккаунтом. Статистика хранится отдельно для каждой игры.':'Выбор профиля показывает его статистику. Для игры сделай его активным.'}</p></div></section>`;
}
function renderAccountRestrictions(a) {
 const summary=accountRestrictionsSummary(a, accountRestrictions);
 const hasBanData=accountRestrictions.has(a.id);
 const badges=[];
 if(summary.market_banned)badges.push('<span class="restriction-badge badge-danger">Торговая площадка: бан</span>');
 if(summary.trade_banned)badges.push('<span class="restriction-badge badge-danger">Обмены: бан</span>');
 if(summary.vac_banned)badges.push(`<span class="restriction-badge badge-danger">VAC-бан${summary.vac_bans_count>0?` (${summary.vac_bans_count})`:''}</span>`);
 if(summary.community_banned)badges.push('<span class="restriction-badge badge-danger">Сообщество: бан</span>');
 if(summary.game_bans.length)badges.push(`<span class="restriction-badge badge-danger">Блокировка в игре (${summary.game_bans.length})</span>`);
 if(summary.game_bans_count>summary.game_bans.length)badges.push(`<span class="restriction-badge badge-danger">Игра не указана (${summary.game_bans_count-summary.game_bans.length})</span>`);
 if(summary.is_limited)badges.push('<span class="restriction-badge badge-warning">Лимит $5</span>');

 const gameBansList=summary.game_bans.map(b=>{const g=games.find(g=>g.launcher==='steam'&&String(g.app_id)===String(b.app_id));const name=g?.name||b.name||('App #'+b.app_id);return `<div class="game-ban-item"><span><strong>⚠️ ${esc(name)}</strong><small class="muted">${esc(b.ban_type||'Бан')}</small></span></div>`;}).join('');
 const unknownCount=Math.max(0,summary.game_bans_count-summary.game_bans.length);
 const unknownNote=unknownCount?`<p class="muted small-text" style="color:#ff9389;margin:6px 0">Для ${unknownCount} ${unknownCount===1?'блокировки Steam не указал игру. Проверь её':'блокировок Steam не указал игры. Проверь их'} в Steam Support и при необходимости добавь игру вручную. Страница поддержки видна только вошедшему владельцу; SWCH не может прочитать вкладку Steam автоматически.</p>`:'';
 const banNote=gameBansList?`<div style="margin-bottom:8px"><small class="muted" style="margin-bottom:4px">Баны в играх:</small>${gameBansList}</div>${unknownNote}`:unknownNote|| (summary.vac_banned?`<p class="muted small-text" style="color:#ff9389;margin:6px 0">Зафиксирован VAC-бан, но Steam не указал игру. Проверь её в Steam Support. SWCH не может прочитать авторизованную вкладку Steam автоматически.</p>`:'');
 const keyCheck=a.launcher==='steam'&&!demo&&(unknownCount>0||summary.vac_banned&&!summary.game_bans.length)?`<details class="ban-key-check" style="margin-top:8px"><summary>Найти игру с ключом Steam Web API</summary><p class="muted small-text">Если Steam вернёт точный AppID, игра появится автоматически. Ключ используется один раз и не сохраняется.</p><div class="game-stats-key"><label>Ключ Steam Web API<input type="password" autocomplete="off" placeholder="Ключ Steam Web API" data-ban-api-key></label><button type="button" class="secondary" data-action="check-restrictions-key" data-account="${esc(a.id)}">Проверить игру</button></div><p class="muted small-text">Для проверки выбранного SteamID подойдёт существующий ключ от другого твоего аккаунта Steam. Создавать ключ для каждого аккаунта не нужно.</p><button type="button" class="text-button" data-action="steam-api-page" data-account="${esc(a.id)}">Открыть страницу Steam Web API для ${esc(a.display_name||a.username||a.id)} ↗</button><p class="muted small-text">Если здесь открылась форма регистрации, можно использовать уже имеющийся ключ. При ошибке «слишком много запросов» повтори попытку позже.</p></details>`:'';

  return `<section class="account-restrictions-box"><div class="restrictions-header"><small>Ограничения аккаунта</small><span class="${summary.has_restrictions?'status-indicator-danger':hasBanData?'status-indicator-clean':'status-indicator-unknown'}">${summary.has_restrictions?'Есть ограничения':hasBanData?'Бан не найден в доступных данных':'Нет данных'}</span></div><div class="restrictions-badges">${badges.length?badges.join(''):`<span class="restriction-badge ${hasBanData?'badge-success':'badge-unknown'}">${hasBanData?'Нет подтверждённых данных о блокировках':'Статус блокировок ещё не получен'}</span>`}</div>${banNote}<div class="restrictions-actions"><button class="secondary mini-btn" data-action="manage-restrictions" data-account="${esc(a.id)}">Настроить ограничения</button>${a.launcher==='steam'&&!demo?`<button class="text-button" style="font-size:10px;text-align:center" data-action="check-restrictions" data-account="${esc(a.id)}" ${busy?'disabled':''}>Проверить в Steam онлайн ↗</button>`:''}${a.launcher==='steam'&&!demo&&(summary.vac_banned||summary.game_bans_count>0)?`<button class="text-button" style="font-size:10px;text-align:center" data-action="steam-ban-page" data-account="${esc(a.id)}" ${busy?'disabled':''}>Открыть блокировки в Steam ↗</button>`:''}</div>${keyCheck}</section>`;
}

function getAccountCurrency(a) {
  const restr = accountRestrictions.get(a.id);
  return restr?.currency || a.currency || currency || 'USD';
}

function currencyToCountry(curr) {
  const map = { BRL: 'BR', RUB: 'RU', KZT: 'KZ', UAH: 'UA', EUR: 'DE', GBP: 'GB', PLN: 'PL', CNY: 'CN', USD: 'US' };
  return map[curr] || 'US';
}

function getAccountCountry(a) {
  const restr = accountRestrictions.get(a.id);
  if (restr?.country) return restr.country;
  if (a.country) return a.country;
  return currencyToCountry(getAccountCurrency(a));
}

function formatAccountMoney(cents, accCurrency) {
  if (cents == null) return '—';
  try {
    return new Intl.NumberFormat(getLocale(), { style: 'currency', currency: accCurrency }).format(cents / 100);
  } catch {
    return `${(cents / 100).toFixed(2)} ${accCurrency}`;
  }
}

let accountSearch = '';
const accountPlatforms = PLATFORM_IDS;
function filterAccountRows() {
 const search=accountSearch.trim().toLocaleLowerCase(getLocale()),view=$('accounts-view');
 view.querySelectorAll('[data-account-search]').forEach(row=>{row.hidden=!row.dataset.accountSearch.includes(search);});
 let count=0;
 view.querySelectorAll('.account-platform-group').forEach(group=>{
  const visible=group.querySelectorAll('.account-row:not([hidden])').length;group.hidden=visible===0;count+=visible;
 });
 const noResults=$('accounts-no-results');if(noResults)noResults.hidden=count>0||!search;
}
function renderCompactAccount(a) {
 const name=a.display_name||a.username||'Аккаунт',id=esc(a.id),launcher=a.launcher;
 if(isModPlatform(launcher))return `<article class="account-tile account-row" data-account-search="${esc([name,a.username,platformName(launcher)].join(' ').toLocaleLowerCase(getLocale()))}"><div class="account-row-main"><div class="account-row-identity">${accountAvatar(a)}<div><h3>${esc(name)}</h3><p>${esc(platformName(launcher))}</p></div></div><span class="account-state">${a.is_active?'Активен':'Сохранён'}</span><div class="account-row-actions"><button class="text-button" data-action="account-games" data-account="${id}">Игры</button>${!a.is_active&&modCapability(launcher,'switch')?`<button class="primary" data-action="switch" data-account="${id}">Сделать активным</button>`:''}${modCapability(launcher,'disconnect')?`<button class="secondary" data-mod-platform="${esc(launcher)}" data-mod-operation="disconnect" data-account="${id}">Отключить</button>`:''}</div></div></article>`;
 const active=launcher==='gog'?Boolean(a.launcher_active):a.is_active;
 const restricted=accountRestrictionsSummary(a,accountRestrictions).has_restrictions||a.is_banned;
 const switchAction={riot:'switch-riot',roblox:'switch-roblox',minecraft:'switch-minecraft'}[launcher]||'switch';
 const openAction=launcher==='riot'?'open-riot':launcher==='roblox'?'switch-roblox':'open-launcher';
 const removeAction={epic:'remove-epic',riot:'remove-riot',roblox:'remove-roblox',minecraft:'remove-minecraft',ubisoft:'remove-ubisoft',gog:'remove-gog',hoyoverse:'remove-hoyoverse'}[launcher];
 const button=(action,label,extra='')=>`<button class="secondary" data-action="${action}" data-account="${id}" ${extra} ${busy?'disabled':''}>${label}</button>`;
 const profiles=launcher==='riot'?riotProfiles.filter(p=>p.account_id===a.id):[];
 const syncErrors=launcher==='riot'?['valorant','league_of_legends'].map(app=>riotProfileErrors.get(`${a.id}:${app}`)).filter(Boolean):[];
 return `<article class="account-tile account-row ${launcher}-account" data-account-search="${esc([name,a.username,platformName(launcher)].join(' ').toLocaleLowerCase(getLocale()))}">
  <div class="account-row-main">
   <div class="account-row-identity">${accountAvatar(a)}<div><h3>${esc(name)}</h3><p>${esc(platformName(launcher))}${a.username&&a.username!==name?' · '+esc(a.username):''}</p></div></div>
   <div class="account-row-status"><span class="account-state ${active?'is-active':''}">${active?(launcher==='gog'?'В GALAXY':'Активен'):'Сохранён'}</span>${restricted?'<span class="account-warning">Есть ограничения</span>':''}${syncErrors.length?'<span class="account-warning">Ошибка обновления</span>':''}</div>
   <div class="account-row-actions"><button class="text-button" data-action="account-games" data-account="${id}">${icon('game')}Игры</button><button class="${active?'secondary':'primary'}" data-action="${active?openAction:switchAction}" data-account="${id}" ${busy?'disabled':''}>${active?(launcher==='gog'?'Открыть GALAXY':launcher==='roblox'?'Запустить клиент':launcher==='riot'?'Открыть Riot Client':'Открыть в лаунчере'):(launcher==='gog'?'Войти в GALAXY':'Сделать активным')}</button></div>
  </div>
  <details class="account-management"><summary>Управление<span aria-hidden="true">⌄</span></summary><div class="account-management-body"><div class="account-management-actions">
   ${!active?button(openAction,launcher==='riot'?'Открыть Riot Client':launcher==='roblox'?'Запустить клиент':'Открыть в лаунчере'):''}
   ${launcher==='epic'&&!demo?button('sync-epic','Синхронизировать игры')+button('open-fortnite','Шкафчик и профиль Fortnite'):''}
   ${launcher==='gog'&&!demo?button('sync-gog','Синхронизировать игры'):''}
   ${launcher==='roblox'?button('roblox-inventory','Инвентарь и RAP'):''}
   ${launcher==='minecraft'?button('minecraft-launch-account','Запустить игру…')+button('open-launcher','Открыть Minecraft Launcher','data-client="official"')+(minecraftStatus?.tlauncher_installed?button('open-launcher','Открыть T-Launcher','data-client="tlauncher"'):''):''}
   ${profiles.map(p=>button('riot-inventory','Скины в инвентаре · '+esc(riotGameName(p.app_id)),`data-app="${esc(p.app_id)}"`)).join('')}
   ${removeAction&&!demo?`<button class="text-button account-remove" data-action="${removeAction}" data-account="${id}" ${busy?'disabled':''}>${launcher==='riot'?'Удалить из SWCH':'Отключить аккаунт'}</button>`:''}
  </div>
   ${launcher==='steam'||launcher==='epic'?`<div class="account-settings-line"><span>Валюта цен: ${esc(getAccountCurrency(a))}</span><button class="text-button" data-action="manage-restrictions" data-account="${id}">Настройки аккаунта</button></div>${renderAccountRestrictions(a)}`:''}
   ${launcher==='riot'?syncErrors.map(e=>`<p class="error-text">${esc(e)}</p>`).join('')+riotSyncButtons('valorant',a.id):''}
  </div></details></article>`;
}
function renderAccounts() {
 const visible=platform==='all'?accounts:accounts.filter(a=>a.launcher===platform);
 const groups=accountPlatforms.filter(p=>visible.some(a=>a.launcher===p));
 $('accounts-view').innerHTML=`
  <div class="accounts-toolbar"><div class="account-platform-tabs" aria-label="Фильтр аккаунтов">${['all',...accountPlatforms.filter(p=>accounts.some(a=>a.launcher===p))].map(p=>`<button data-platform="${p}" class="${p===platform?'selected':''}" aria-pressed="${p===platform}">${p==='all'?'Все':platformLogo(p)+esc(platformName(p))}<span>${p==='all'?accounts.length:accounts.filter(a=>a.launcher===p).length}</span></button>`).join('')}</div><label class="search account-search">${icon('search')}<input id="account-search-input" type="search" placeholder="Найти аккаунт" aria-label="Найти аккаунт" value="${esc(accountSearch)}"></label></div>
  <div class="accounts-list-heading"><span>${visible.length} аккаунтов · ${visible.filter(a=>a.is_active).length} активных</span><button class="primary" data-action="connect" ${PLATFORMS[platform]&&!isModPlatform(platform)?`data-connect-tab="${platform}"`:''}>${icon('plus')}Добавить аккаунт</button></div>
  ${visible.length?groups.map(p=>`<section class="account-platform-group" aria-label="Аккаунты ${esc(platformName(p))}"><div class="account-group-heading">${platformLogo(p)}<h2>${esc(platformName(p))}</h2><span>${visible.filter(a=>a.launcher===p).length}</span></div><div class="account-grid account-list">${visible.filter(a=>a.launcher===p).map(renderCompactAccount).join('')}</div></section>`).join(''):empty(platform==='all'?'Все аккаунты — под рукой':'Нет аккаунтов '+platformName(platform),'Добавь аккаунт, чтобы выбирать профиль для игры.','connect','Подключить аккаунт')}
  <p id="accounts-no-results" class="accounts-no-results" hidden>Аккаунты не найдены. Попробуй другое имя или платформу.</p>
  <p class="accounts-hint">${icon('game')}Статистика, ранг и время в игре — в карточке игры для выбранного аккаунта.</p>
  ${!demo&&(platform==='all'||platform==='riot')?`<details class="accounts-service"><summary>Riot Games · Сессия и обновление профилей</summary><section class="riot-profile"><button class="secondary" data-action="save-riot-session" ${busy?'disabled':''}>Сохранить текущую сессию Riot</button>${riotSyncButtons()}</section></details>`:''}
  ${!demo&&visible.some(a=>['steam','epic'].includes(a.launcher))?`<details class="accounts-service accounts-valuation"><summary>Оценка библиотеки</summary>${renderLibraryValueToolbar()}${visible.filter(a=>['steam','epic'].includes(a.launcher)).map(a=>`<section class="account-valuation"><h3>${esc(a.display_name||a.username)}</h3>${renderLibraryValue(a)}</section>`).join('')}</details>`:''}`;
 filterAccountRows();$('account-search-input').addEventListener('input',e=>{accountSearch=e.target.value;filterAccountRows();});
}
function applyRiotProfiles(data) {
 riotProfiles = data.profiles || [];
 const identities = new Map();
 for (const a of (Array.isArray(data.accounts)?[]:accounts.filter(a => a.launcher === 'riot'))) {
  identities.set(a.id, a);
 }
 if (Array.isArray(data.accounts)) {
  for (const a of data.accounts) {
   identities.set(a.id, {
    id: a.id,
    launcher: 'riot',
    username: a.username || a.display_name,
    display_name: a.display_name || a.username,
    is_active: Boolean(a.is_active),
   });
  }
 }
 if (Array.isArray(data.sessions)) {
  for (const s of data.sessions) {
   const id = s.account_id.startsWith('riot_') ? s.account_id : `riot_${s.account_id}`;
   if (!identities.has(id) && !identities.has(s.account_id)) {
    identities.set(id, {
     id,
     launcher: 'riot',
     username: (s.username && !s.username.includes('-') && s.username !== 'null') ? s.username : (s.display_name ? s.display_name.split('#')[0] : s.account_id),
     display_name: s.display_name || s.username || s.account_id,
     is_active: false,
    });
   }
  }
 }
 for(const p of riotProfiles) {
  if (!identities.has(p.account_id)) {
   identities.set(p.account_id, {
    id: p.account_id,
    launcher: 'riot',
    username: p.display_name,
    display_name: p.display_name,
    is_active: false,
   });
  }
 }
 accounts = [...accounts.filter(a => a.launcher !== 'riot'), ...identities.values()];
 updateConnectTabBadges();
}
const riotGameName=id=>({league_of_legends:'League of Legends',valorant:'VALORANT',bacon:'Legends of Runeterra'}[id]||id);
function riotSyncButtons(appId=null, accountId=null) {
 return `<div class="account-actions">${(appId?[appId]:['league_of_legends','valorant']).filter(id=>['league_of_legends','valorant'].includes(id)).map(id=>`<button class="secondary" data-action="sync-riot" data-app="${esc(id)}" ${accountId?`data-account="${esc(accountId)}"`:''} ${busy||riotSyncBusy?'disabled':''}>${riotSyncBusy?'Загружаем…':'Обновить профиль · '+riotGameName(id)}</button>`).join('')}${!appId?`<button class="secondary" data-action="sync-all-riot" ${busy||riotSyncBusy?'disabled':''}>Обновить все аккаунты · VALORANT</button>`:''}</div><p class="muted small-text">VALORANT не нужно устанавливать или запускать: для обновления достаточно входа в Riot Client. Сохранённые данные доступны и при закрытом клиенте. SWCH автоматически запустит установленный клиент League и дождётся профиля.</p>${riotSyncProgress?`<p class="muted" role="status">${esc(riotSyncProgress)}</p>`:''}`;
}
async function syncRiotProfile(appId, accountId=null) {
 if(demo){toast('В демо синхронизация отключена.');return;}
 if(busy||riotSyncBusy)return;
 riotSyncBusy=true;busy=true;render();
 try {
  if(accountId&&['valorant','league_of_legends'].includes(appId)&&!accounts.find(a=>a.id===accountId)?.is_active)await api('/api/riot/switch-account',{account_id:accountId});
  applyRiotProfiles(await api('/api/riot/accounts/sync',{app_id:appId,...(accountId?{account_id:accountId}:{})}));riotProfileErrors.delete(`${accountId}:${appId}`);if($('connect-dialog').open){$('connect-dialog').close();page='accounts';}toast('Профиль обновлён. Скины доступны в инвентаре.');}
 catch(e){if(accountId)riotProfileErrors.set(`${accountId}:${appId}`,e.message);toast(e.message);}finally{riotSyncBusy=false;busy=false;render();if($('game-modal')?.open&&$('game-modal').dataset.game)openGameModal($('game-modal').dataset.game);}
}
async function syncAllRiotProfiles() {
 if(demo){toast('В демо синхронизация отключена.');return;}
 if(busy||riotSyncBusy)return;
 const list=accounts.filter(a=>a.launcher==='riot');
 const original=list.find(a=>a.is_active);
 let currentId=original?.id, switched=false, completed=0, restoreError='';
 busy=true;riotSyncBusy=true;
 try {
  for(const [index,account] of list.entries()) {
   riotSyncProgress=`Обновляем ${index+1}/${list.length}: ${account.display_name||account.username}. Игра не запускается.`;render();
   try {
    if(account.id!==currentId){switched=true;await api('/api/riot/switch-account',{account_id:account.id});currentId=account.id;}
    applyRiotProfiles(await api('/api/riot/accounts/sync',{app_id:'valorant',account_id:account.id}));
    riotProfileErrors.delete(`${account.id}:valorant`);completed++;
   } catch(error){currentId=null;riotProfileErrors.set(`${account.id}:valorant`,error.message);}
  }
 } finally {
  if(original&&switched) {
   riotSyncProgress='Возвращаем исходный аккаунт Riot Client…';render();
   try{await api('/api/riot/switch-account',{account_id:original.id});}catch(error){restoreError=`Не удалось вернуть ${original.display_name||original.username}: ${error.message}`;}
  }
  try{applyRiotProfiles(await api('/api/riot/accounts'));}catch(error){restoreError=restoreError||error.message;}
  riotSyncProgress='';busy=false;riotSyncBusy=false;render();
  toast(`Обновлено аккаунтов: ${completed}/${list.length}.${completed<list.length?' Причины ошибок показаны в карточках.':''}${restoreError?' '+restoreError:''}`);
 }
}
function renderPlayedGames(a) {
 if(a.launcher!=='steam')return '';
 const played=accountPlayedGames(a,steamRows);
 return `<details class="played-games"><summary>Игры с проведённым временем · ${played.length}</summary>${played.length?played.map(g=>`<p>${esc(g.name)} · ${format.format(g.playtime_minutes/60)} ч</p>`).join(''):'<p class="muted">Нет подтверждённого времени игры.</p>'}</details>`;
}
function riotCharacterCards(items) {
 return `<div class="riot-character-grid">${items.map(item=>`<article class="riot-character">${safeImage(item.image_url)?`<img loading="lazy" src="${esc(safeImage(item.image_url))}" alt="${esc(item.name)}">`:'<div class="inventory-placeholder">'+icon('users')+'</div>'}<strong>${esc(item.name)}</strong></article>`).join('')}</div>`;
}
function riotProfileSummary(p) {
 const count=section=>section?.value==null?'Нет данных':section.value.length+(section.error?' · сохранено':'');
 const time=p.playtime?.value;
 const timeData=gamePlaytime({launcher:'riot',app_id:p.app_id},{riotProfiles:[p],accountId:p.account_id});
 const ranked=p.ranked?.value;
 const access=ranked?.eligibility??(p.level==null?null:p.level>=(p.app_id==='valorant'?20:30));
 const rank=ranked?(ranked.rank||'Нет ранга в текущем акте'):'Нет данных';
 return `<div class="metrics"><div class="metric riot-playtime"><span>Время в доступных матчах</span><strong>${timeData?esc(timeLabel(timeData)):'Нет данных'}</strong><small>${time?time.matches+' матчей · история Riot':'Обнови профиль, чтобы получить историю'}${p.playtime?.error?' · сохранённые данные':''}</small></div><div class="metric"><span>Уровень аккаунта</span><strong>${p.level??'Нет данных'}</strong></div><div class="metric"><span>Скины</span><strong>${count(p.skins)}</strong></div><div class="metric"><span>${p.app_id==='valorant'?'Доступные агенты':'Открытые чемпионы'}</span><strong>${count(p.champions)}</strong></div><div class="metric"><span>Рейтинг · доступ по уровню</span><strong>${access==null?'Нет данных':access?'Доступен':'Недоступен'}</strong></div><div class="metric"><span>Текущий ранг</span><strong>${esc(rank)}${ranked?.rank&&ranked.rating!=null?' · '+esc(ranked.rating)+(p.app_id==='valorant'?' RR':' LP'):''}</strong></div></div>${p.champions?.value?`<details class="riot-agent-list"><summary>${p.app_id==='valorant'?'Какие агенты доступны':'Какие чемпионы открыты'} · ${p.champions.value.length}</summary>${riotCharacterCards(p.champions.value)}</details>`:''}<p class="muted small-text">${time?'Время рассчитано по доступной истории матчей, а не за всё время игры.':''} ${ranked?.note?esc(ranked.note):'Ограничения и требования подтверждения входа проверяются в игре.'}</p>${p.ranked?.error?`<p class="muted small-text">${esc(p.ranked.error)}${ranked?' Показаны сохранённые данные.':''}</p>`:''}${p.champions?.error?`<p class="muted small-text">Агенты: ${esc(p.champions.error)}</p>`:''}`;
}
function renderRiotGame(g) {
 const riotAccs = accounts.filter(a => a.launcher === 'riot');
 const activeAcc = riotAccs.find(a => a.id === selectedAccounts[gameKey(g)]) || riotAccs.find(a => a.is_active) || riotAccs[0];
 const profiles = riotProfiles.filter(p => p.app_id === g.app_id && p.account_id === activeAcc?.id);
 return `<section class="game-accounts account-section">
  <div class="account-heading"><div><h3>Аккаунт для игры</h3><small>Выбери профиль, чтобы посмотреть его статистику</small></div></div>
  ${riotAccs.length ? `
    <div class="account-chips" style="display:flex;gap:8px;flex-wrap:wrap;margin:10px 0 14px;">
      ${riotAccs.map(a => {
        const isSelected = activeAcc && activeAcc.id === a.id;
        return `<button type="button" class="account-chip ${isSelected ? 'selected' : ''}" data-action="select-riot-game-account" data-game="${esc(gameKey(g))}" data-account="${esc(a.id)}" aria-pressed="${Boolean(isSelected)}">
          <span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${a.is_active ? '#4ade80' : '#888'};"></span>
          <strong>${esc(a.display_name || a.username)}</strong>
          ${a.is_active ? '<span style="font-size:10px;opacity:0.8;">(активен)</span>' : ''}
        </button>`;
      }).join('')}
    </div>
  ` : '<p class="muted">Сохранённых аккаунтов Riot пока нет. Войди в нужный аккаунт в Riot Client и нажми «Сохранить текущую сессию».</p>'}
  <div style="display:flex;gap:10px;align-items:center;flex-wrap:wrap;margin:12px 0;">
    <button class="primary" data-action="launch" data-game="${esc(gameKey(g))}" ${busy || !canRequestLaunch(g) || downloadState(g)?.phase==='requested' ? 'disabled' : ''}>
      ${icon('play')} Запустить ${esc(g.name)} ${activeAcc ? `(${esc(activeAcc.display_name || activeAcc.username)})` : ''}
    </button>
    <button class="secondary" data-action="open-riot" ${busy || !riotStatus?.client_installed ? 'disabled' : ''}>
      Открыть Riot Client
    </button>
    <button class="secondary" data-action="save-riot-session" ${busy ? 'disabled' : ''}>
      Сохранить текущую сессию
    </button>
  </div>
  ${profiles.length ? profiles.map(p => `<article class="riot-profile"><h4>Статистика ${esc(g.name)} · ${esc(p.display_name)}</h4>${riotProfileSummary(p)}<button class="text-button" data-action="riot-inventory" data-account="${esc(p.account_id)}" data-app="${esc(p.app_id)}">Скины в инвентаре ↗</button><p class="muted small-text">Обновлено: ${esc(dateLabel(p.synced_at))}</p></article>`).join('') : `<p class="muted">Информация об этом аккаунте ещё не загружена. Нажми «Обновить профиль».</p>`}
  ${riotSyncButtons(g.app_id,activeAcc?.id)}
 </section>`;
}
function openRiotCollection(accountId,appId) {
 const p=riotProfiles.find(p=>p.account_id===accountId&&p.app_id===appId);if(!p)return;
 let dialog=$('riot-collection-dialog');if(!dialog){dialog=document.createElement('dialog');dialog.id='riot-collection-dialog';document.body.append(dialog);}
 const section=(title,data)=>`<section><h3>${title} · ${data?.value==null?'Нет данных':data.value.length}</h3>${data?.error?`<p class="error-text">${esc(data.error)}${data.value?' Показана сохранённая коллекция.':''}</p>`:''}<small class="muted">${data?.updated_at?'Обновлено: '+esc(dateLabel(data.updated_at)):'Ещё не загружено'}</small><div class="riot-items">${data?.value?.map(item=>`<article>${safeImage(item.image_url)?`<img loading="lazy" src="${esc(safeImage(item.image_url))}" alt="">`:''}<strong>${esc(item.name)}</strong></article>`).join('')||'<p class="muted">'+(data?.value?'Коллекция пуста.':'Данные недоступны.')+'</p>'}</div></section>`;
 dialog.innerHTML=`<div class="dialog-header"><div><h2>${esc(p.display_name)}</h2><p>${riotGameName(appId)} · Коллекция аккаунта</p></div><button class="icon-button" data-close aria-label="Закрыть">${icon('close')}</button></div>${riotProfileSummary(p)}${p.playtime?.error?`<p class="error-text">${esc(p.playtime.error)}</p>`:''}${section('Скины',p.skins)}${section(appId==='valorant'?'Разблокированные агенты':'Открытые чемпионы',p.champions)}`;
 dialog.showModal();
}

function renderColorOptions() {
  const colorContainer = $('color-options');
  const activeAccent = getEffectiveAccentId();
  if (colorContainer) {
    colorContainer.innerHTML = ACCENT_COLORS.map(c => `
      <button type="button" class="color-swatch-btn ${c.id === activeAccent ? 'selected' : ''}" data-color-choice="${c.id}" aria-pressed="${c.id === activeAccent}">
        <span class="color-dot" style="background:${c.hex}"></span>
        <span class="color-name">${c.name}</span>
        ${c.id === activeAccent ? '<span class="color-check">✓</span>' : ''}
      </button>
    `).join('') + `
      <label class="color-swatch-btn custom-color-swatch ${accentColor === 'custom' ? 'selected' : ''}">
        <span class="color-dot" style="background:${customAccentColor}"></span>
        <span class="color-name">Свой цвет</span>
        <span class="color-check" ${accentColor === 'custom' ? '' : 'hidden'}>✓</span>
        <input id="custom-accent-color" type="color" value="${customAccentColor}" aria-label="Свой цвет интерфейса">
      </label>`;
  }
}
function updateCustomAccentColor(value) {
 customAccentColor = normalizeAccentHex(value);
 accentColor = 'custom';
 hasSavedAccent = true;
 save('swch-custom-accent-color', customAccentColor);
 save('swch-accent-color', accentColor);
 applyStyle();
 const container = $('color-options');
 container.querySelectorAll('[data-color-choice]').forEach(button => {
   button.classList.remove('selected');
   button.setAttribute('aria-pressed', 'false');
   button.querySelector('.color-check')?.remove();
 });
 const swatch = container.querySelector('.custom-color-swatch');
 swatch.classList.add('selected');
 swatch.querySelector('.color-dot').style.background = customAccentColor;
 swatch.querySelector('.color-check').hidden = false;
}
function showStyles() {
  renderDesignOptions();
  renderColorOptions();
  document.querySelectorAll('[data-layout-choice]').forEach(b => {
    const isSelected = b.dataset.layoutChoice === layout;
    b.classList.toggle('selected', isSelected);
    b.setAttribute('aria-pressed', String(isSelected));
  });
  renderCurrency();
  refreshStartup();
  if (gamepadManager) {
    gamepadManager.updateConnectedList();
    gamepadManager.renderSettingsDevices();
  }
  const dlg = $('styles-dialog');
  if (dlg) dlg.showModal();
}
function updateConnectTabBadges() {
  const steamCount = accounts.filter(a => a.launcher === 'steam').length;
  const epicCount = accounts.filter(a => a.launcher === 'epic').length;
  const riotCount = accounts.filter(a => a.launcher === 'riot').length;
  const robloxCount = accounts.filter(a => a.launcher === 'roblox').length;
  const mcCount = accounts.filter(a => a.launcher === 'minecraft').length;

  const sB = $('steam-tab-badge'); if (sB) sB.textContent = steamCount;
  const eB = $('epic-tab-badge'); if (eB) eB.textContent = epicCount;
  const rB = $('riot-tab-badge'); if (rB) rB.textContent = riotCount;
  const rbB = $('roblox-tab-badge'); if (rbB) rbB.textContent = robloxCount;
  const mB = $('minecraft-tab-badge'); if (mB) mB.textContent = mcCount;
  const uB = $('ubisoft-tab-badge'); if (uB) uB.textContent = accounts.filter(a=>a.launcher==='ubisoft').length;
  const uDot = $('ubisoft-status-dot'); if(uDot) uDot.className='status-indicator-dot '+(ubisoftStatus?.client_installed?'online':'warning');
  const uDesc = $('ubisoft-status-description'); if(uDesc) uDesc.textContent=ubisoftStatus?.message||'Установленные игры и сохранённые аккаунты на Windows.';
  const gB=$('gog-tab-badge');if(gB)gB.textContent=accounts.filter(a=>a.launcher==='gog').length;
  const gDot=$('gog-status-dot');if(gDot)gDot.className='status-indicator-dot '+(accounts.some(a=>a.launcher==='gog')?'online':'warning');
  const gDesc=$('gog-status-description');if(gDesc)gDesc.textContent=gogStatus?.message||'Подключи аккаунты GOG, чтобы увидеть купленные игры.';

  const accBadge = $('accounts-count'); if (accBadge) accBadge.textContent = accounts.length;

  const epicDot = $('epic-status-dot');
  if (epicDot) {
    epicDot.className = 'status-indicator-dot ' + (epicCount > 0 ? 'online' : 'warning');
  }
  const riotDot = $('riot-status-dot');
  if (riotDot) {
    riotDot.className = 'status-indicator-dot ' + (riotCount > 0 ? 'online' : 'warning');
  }
}

let epicBrowserSession = null;
let epicBrowserTimer = null;
let epicBrowserStarting = false;

function resetEpicBrowserProgress() {
  $('epic-browser-progress').hidden = true;
  $('epic-browser-return').hidden = true;
  $('epic-browser-return').removeAttribute('href');
  $('open-epic-browser-btn').disabled = false;
  $('cancel-epic-browser-btn').disabled = false;
  $('login-form').querySelector('button[type="submit"]').disabled = false;
}

async function cancelEpicBrowserLogin() {
  const session = epicBrowserSession;
  if (!session) return true;
  if (!session.connected) {
    try {
      const result = await api('/api/epic/auth/browser/cancel', {session_id: session.id});
      if (result.status === 'authenticating' || result.status === 'connected') {
        toast('Подключение аккаунта уже началось. Дождись результата.');
        return false;
      }
    } catch { /* Expired or restarted backend: a new session can be started. */ }
  }
  if (epicBrowserSession === session) {
    clearTimeout(epicBrowserTimer);
    epicBrowserSession = null;
    resetEpicBrowserProgress();
  }
  return true;
}

async function pollEpicBrowserLogin(session) {
  if (epicBrowserSession !== session) return;
  try {
    const result = await api('/api/epic/auth/browser/status', {session_id: session.id});
    if (epicBrowserSession !== session) return;
    if (result.status === 'authenticating') {
      $('epic-browser-status').textContent = 'Подключаем аккаунт Epic Games…';
      $('cancel-epic-browser-btn').disabled = true;
      $('open-epic-browser-btn').disabled = true;
      $('login-form').querySelector('button[type="submit"]').disabled = true;
    } else if (result.status === 'connected') {
      if (!session.connected) {
        session.connected = true;
        $('oauth-code-input').value = '';
        $('connect-dialog').close();
        toast(`Аккаунт ${result.account?.display_name || 'Epic Games'} подключён. Загружаем библиотеку…`);
        await refresh();
      }
      if (result.library_status !== 'syncing') {
        if (epicBrowserSession !== session) return;
        epicBrowserSession = null;
        resetEpicBrowserProgress();
        await refresh();
        toast(result.library_status === 'failed'
          ? 'Аккаунт подключён. Библиотеку можно загрузить кнопкой «Синхронизировать игры».'
          : 'Библиотека Epic Games загружена.');
        return;
      }
    } else if (['failed', 'expired', 'cancelled'].includes(result.status)) {
      epicBrowserSession = null;
      resetEpicBrowserProgress();
      $('login-error').textContent = result.error || (result.status === 'cancelled'
        ? 'Вход отменён. Можно начать заново.' : 'Время входа истекло. Нажми «Войти в Epic Games» ещё раз.');
      return;
    }
  } catch (error) {
    if (epicBrowserSession !== session) return;
    $('login-error').textContent = error.message;
    // Poll through short connection failures, but never indefinitely.
    if (Date.now() > session.deadline) {
      epicBrowserSession = null;
      resetEpicBrowserProgress();
      return;
    }
  }
  if (epicBrowserSession === session) epicBrowserTimer = setTimeout(() => pollEpicBrowserLogin(session), 1000);
}

async function startEpicBrowserLogin() {
  if (demo || epicBrowserStarting) return;
  epicBrowserStarting = true;
  try {
    if (!await cancelEpicBrowserLogin()) return;
    $('open-epic-browser-btn').disabled = true;
    $('login-error').textContent = '';
    const data = await api('/api/epic/auth/browser/start', {});
    const url = new URL(data.browser_url);
    if (url.origin !== location.origin || url.pathname !== '/epic/connect'
        || !/^[a-f0-9]{64}$/.test(data.session_id || '')) throw new Error('Не удалось создать сессию входа Epic.');
    const session = {id: data.session_id, connected: false, deadline: Date.now() + 15 * 60 * 1000};
    epicBrowserSession = session;
    $('epic-browser-progress').hidden = false;
    $('epic-browser-status').textContent = 'Войди в Epic в окне браузера. Аккаунт появится здесь автоматически. Если расширение ещё не установлено, на странице есть инструкция.';
    $('epic-browser-return').href = url.href;
    try { await api('/api/epic/auth/browser/open', {session_id: session.id}); }
    catch {
      $('epic-browser-return').hidden = false;
      $('epic-browser-status').textContent = 'Браузер не открылся автоматически. Нажми «Открыть страницу входа».';
    }
    pollEpicBrowserLogin(session);
  } catch (error) {
    $('login-error').textContent = error.message;
  } finally {
    epicBrowserStarting = false;
    $('open-epic-browser-btn').disabled = false;
  }
}

let ubisoftNameRequest = 0;
async function fillUbisoftAccountName(quiet=false) {
 const input=$('ubisoft-account-name'),status=$('ubisoft-name-status');if(!input||demo)return;
 const request=++ubisoftNameRequest,previous=input.value;
 if(!quiet)status.textContent='Определяем имя аккаунта Ubisoft Connect…';
 try {
  const profile=await api('/api/ubisoft/current-account');
  if(request!==ubisoftNameRequest)return;
  if(profile.display_name){
   if(input.value===previous&&(!input.value||input.value===input.dataset.autoName)){
    input.value=profile.display_name;input.dataset.autoName=profile.display_name;
   }
   status.textContent='Имя найдено в Ubisoft Connect. Его можно изменить.';
  }else {
   if(!profile.account_id&&input.value===input.dataset.autoName){input.value='';delete input.dataset.autoName;}
   status.textContent='Имя пока не найдено. Можно оставить поле пустым или указать своё название.';
  }
  if(profile.account_id&&!ubisoftAutoLoginId&&$('ubisoft-login-step'))$('ubisoft-login-step').textContent='Текущий аккаунт определён. Другой аккаунт можно добавить одной кнопкой.';
 }catch {if(request===ubisoftNameRequest)status.textContent='Не удалось определить имя. Можно оставить поле пустым или указать своё название.';}
}

let ubisoftPolling=false;
async function pollUbisoftAccounts() {
 const connecting=$('connect-dialog')?.open&&document.querySelector('[data-pane="ubisoft"]')?.classList.contains('active');
 if(demo||busy||document.hidden||ubisoftPolling||!(page==='accounts'||connecting))return;
 ubisoftPolling=true;
 try {
  const data=await api('/api/ubisoft/accounts');
  if(!Array.isArray(data.accounts)||busy)return;
  const previous=accounts.filter(a=>a.launcher==='ubisoft');
  if(JSON.stringify(previous)!==JSON.stringify(data.accounts)){
   accounts=[...accounts.filter(a=>a.launcher!=='ubisoft'),...data.accounts];
   if(page==='accounts'){renderAccounts();hydrateIcons();}
   updateConnectTabBadges();
  }
  if(connecting)await fillUbisoftAccountName(true);
 }catch {} finally {ubisoftPolling=false;}
}

let ubisoftAutoLoginId=null,ubisoftAutoPolling=false;
function ubisoftAutoControls() {
 document.querySelector('[data-action="prepare-ubisoft"]').disabled=Boolean(ubisoftAutoLoginId);
 document.querySelector('[data-action="open-ubisoft"]').disabled=Boolean(ubisoftAutoLoginId);
 $('ubisoft-session-form').querySelector('button[type="submit"]').disabled=Boolean(ubisoftAutoLoginId);
 document.querySelector('[data-action="cancel-ubisoft-login"]').hidden=!ubisoftAutoLoginId;
}
async function pollUbisoftAutoLogin() {
 if(!ubisoftAutoLoginId||ubisoftAutoPolling||busy||document.hidden)return;
 ubisoftAutoPolling=true;const id=ubisoftAutoLoginId;
 try {
  const result=await api(`/api/ubisoft/auth/status?session_id=${encodeURIComponent(id)}`);
  if(id!==ubisoftAutoLoginId)return;
  $('ubisoft-session-error').textContent='';
  if(result.status==='waiting'){$('ubisoft-login-step').textContent='Ожидаем вход в Ubisoft Connect. После входа аккаунт сохранится автоматически.';return;}
  if(result.status==='saving'){$('ubisoft-login-step').textContent='Вход подтверждён. Сохраняем аккаунт…';return;}
  ubisoftAutoLoginId=null;ubisoftAutoControls();
  if(result.status==='connected'){
   const account=result.account;
   if(account)accounts=[...accounts.filter(a=>a.launcher!=='ubisoft'||a.id!==account.id),account];
   if($('connect-dialog').open&&document.querySelector('[data-pane="ubisoft"]').classList.contains('active'))$('connect-dialog').close();
   page='accounts';platform='ubisoft';
   toast(`Аккаунт ${account?.display_name||account?.username||'Ubisoft'} сохранён автоматически`);
   await refresh();
  }else if(result.status==='cancelled')$('ubisoft-login-step').textContent='Добавление отменено. Сохранённые аккаунты доступны в списке.';
  else $('ubisoft-session-error').textContent=result.error||(result.status==='expired'?'Время ожидания истекло. Нажми «Добавить аккаунт Ubisoft» снова.':'Не удалось добавить аккаунт. Повтори вход.');
 }catch(error){
  if(id===ubisoftAutoLoginId){
   if(error.status===404){ubisoftAutoLoginId=null;ubisoftAutoControls();$('ubisoft-session-error').textContent=error.message;}
   else $('ubisoft-session-error').textContent=error.message+' Повторяем проверку автоматически.';
  }
 }finally{ubisoftAutoPolling=false;}
}
async function cancelUbisoftAutoLogin() {
 if(!ubisoftAutoLoginId)return true;
 const id=ubisoftAutoLoginId;
 try {
  const result=await api('/api/ubisoft/auth/cancel',{session_id:id});
  if(ubisoftAutoLoginId===id){ubisoftAutoLoginId=null;ubisoftAutoControls();}
  $('ubisoft-login-step').textContent=result.status==='connected'?'Аккаунт уже сохранён автоматически.':'Добавление отменено. Предыдущий аккаунт восстановлен, если он был сохранён.';
  await refresh();return true;
 }catch(error){$('ubisoft-session-error').textContent=error.message;return false;}
}

async function prepareUbisoftLogin() {
 if(demo||busy||ubisoftAutoLoginId)return;
 busy=true;render();
 const button=document.querySelector('[data-action="prepare-ubisoft"]');
 button.disabled=true;$('ubisoft-session-error').textContent='';
 try {
  const result=await api('/api/ubisoft/accounts/prepare',{automatic:true});
  if(!result.automatic_session_id)throw new Error('Обнови сервис SWCH, чтобы добавить аккаунт автоматически.');
  ubisoftAutoLoginId=result.automatic_session_id;ubisoftAutoControls();
  ++ubisoftNameRequest;
  $('ubisoft-account-name').value='';delete $('ubisoft-account-name').dataset.autoName;
  $('ubisoft-name-status').textContent='Имя заполнится после входа в другой аккаунт.';
  $('ubisoft-login-step').textContent='Войди в открывшемся Ubisoft Connect с «Запомнить меня». SWCH сохранит аккаунт автоматически.';
  toast(result.message);
 }catch(error){$('ubisoft-session-error').textContent=error.message;}
 finally{busy=false;ubisoftAutoControls();await refresh();}
}

function switchConnectTab(tabName) {
  const tabs = document.querySelectorAll('#connect-dialog .connect-tab-btn');
  const panes = document.querySelectorAll('#connect-dialog .connect-tab-pane');
  if(![...tabs].some(btn=>btn.dataset.tab===tabName))tabName='steam';
  tabs.forEach(btn => {
    const selected=btn.dataset.tab===tabName;
    btn.classList.toggle('active', selected);
    btn.setAttribute('role','tab');
    btn.id='connect-tab-'+btn.dataset.tab;
    btn.setAttribute('aria-controls','connect-pane-'+btn.dataset.tab);
    btn.setAttribute('aria-selected',String(selected));
    btn.tabIndex=selected?0:-1;
  });
  panes.forEach(pane => {
    const selected=pane.dataset.pane===tabName;
    pane.classList.toggle('active', selected);
    pane.id='connect-pane-'+pane.dataset.pane;
    pane.setAttribute('role','tabpanel');
    pane.setAttribute('aria-labelledby','connect-tab-'+pane.dataset.pane);
    pane.tabIndex=0;
    pane.hidden=!selected;
  });
  $('connect-platform-select').value=tabName;
  document.querySelector('#connect-dialog .connect-tabs-content').scrollTop=0;
  if(tabName==='ubisoft')void fillUbisoftAccountName();
}

const connectNavigation=document.querySelector('#connect-dialog .connect-tabs-nav');
connectNavigation.setAttribute('role','tablist');
connectNavigation.setAttribute('aria-orientation','vertical');
connectNavigation.addEventListener('keydown',event=>{
  const tabs=[...connectNavigation.querySelectorAll('.connect-tab-btn')];
  const index=tabs.indexOf(event.target);
  if(index<0||!['ArrowUp','ArrowDown','Home','End'].includes(event.key))return;
  event.preventDefault();
  const next=event.key==='Home'?0:event.key==='End'?tabs.length-1:(index+(event.key==='ArrowDown'?1:-1)+tabs.length)%tabs.length;
  switchConnectTab(tabs[next].dataset.tab);
  tabs[next].focus();
});
$('connect-platform-select').addEventListener('change',event=>switchConnectTab(event.target.value));

async function connect(defaultTab = 'steam') {
  if(demo) {toast('В демо подключение отключено. Для реальных аккаунтов открой обычную библиотеку.');return;}
  if (!await cancelEpicBrowserLogin()) return;
  renderSteamConnect();
  $('steam-scan-status').textContent='';
  updateConnectTabBadges();
  switchConnectTab(defaultTab);
  $('login-error').textContent='';$('oauth-code-input').value='';$('epic-auth-link').removeAttribute('href');$('connect-dialog').showModal();
  try {const data=await api('/api/epic/auth/url');const url=new URL(data.url || data.auth_url);if(url.protocol!=='https:' || !(url.hostname==='epicgames.com'||url.hostname.endsWith('.epicgames.com'))) throw new Error('Не удалось получить адрес входа Epic.');$('epic-auth-link').href=url.href;} catch(e) {$('login-error').textContent=e.message;}
}

$('connect-dialog').addEventListener('cancel',event=>{
 if(!ubisoftAutoLoginId)return;
 event.preventDefault();void cancelUbisoftAutoLogin().then(cancelled=>{if(cancelled)$('connect-dialog').close();});
});
function editMetrics() {
  const g=games.find(g=>gameKey(g)===selectedGame),a=g&&chosenAccount(g);if(!a)return;
  metricTarget={g,a};const m=displayedMetrics(g,a);$('metrics-context').textContent=`${g.name} · ${a.display_name||a.username}`;
  for(const key of ['skins','value','hours']) $('metrics-form').elements[key].value=m[key]??'';
  $('metrics-dialog').showModal();
}
async function saveRiotSession() {
  if (demo) { toast('В демо сохранение сессии отключено.'); return; }
  if (busy) return;
  busy = true; render();
  toast('Сохраняем текущую сессию Riot Client...');
  try {
    const res = await api('/api/riot/accounts/save', {});
    toast(res.message || 'Сессия Riot Client успешно сохранена!');
    if (res.account) {
      accounts = [
        ...accounts.filter(a => a.launcher !== 'riot' || a.id !== res.account.id),
        { ...res.account, is_active: true }
      ];
      accounts.forEach(a => { if (a.launcher === 'riot' && a.id !== res.account.id) a.is_active = false; });
    }
    if ($('connect-dialog')?.open) {
      $('connect-dialog').close();
      page = 'accounts';
    }
    platform = 'riot';
    page = 'accounts';
    updateConnectTabBadges();
    if(res.account) {
      toast('Сессия сохранена. Загружаем профиль VALORANT без запуска игры…');
      try {
        applyRiotProfiles(await api('/api/riot/accounts/sync',{app_id:'valorant',account_id:res.account.id}));
        riotProfileErrors.delete(`${res.account.id}:valorant`);
        toast('Сессия и профиль VALORANT сохранены.');
      } catch(error) {
        riotProfileErrors.set(`${res.account.id}:valorant`,error.message);
        toast('Сессия сохранена. Профиль пока не загружен: '+error.message);
      }
    }
  } catch (err) {
    toast('Ошибка сохранения сессии Riot: ' + (err?.message || err));
  } finally {
    busy = false;
    await refresh(true);
    updateConnectTabBadges();
  }
}
async function switchAccount(id) {
 if(busy)return false;const a=accounts.find(a=>a.id===id);if(!a)return false;
 if(demo){accounts.forEach(acc=>{if(acc.launcher===a.launcher)acc.is_active=acc.id===id;});render();toast(`Демо: выбран аккаунт ${a.display_name}`);return true;}
 busy=true;render();
  try {
    if(isModPlatform(a.launcher)) {await modOperation(a.launcher,'switch',{account_id:a.id});toast('Аккаунт переключён');return true;}
    if(a.launcher==='roblox') {
      const result=await api('/api/roblox/switch-account',{account_id:a.id});
      accounts.forEach(acc=>{if(acc.launcher==='roblox')acc.is_active=acc.id===id;});
      toast(result.message || `Аккаунт Roblox @${a.username} активирован`);
      return true;
    }
    if(a.launcher==='minecraft') {
      const result=await api('/api/minecraft/switch-account',{account_id:a.id});
      accounts.forEach(acc=>{if(acc.launcher==='minecraft')acc.is_active=acc.id===id;});
      toast(result.message || `Аккаунт Minecraft ${a.username} активирован`);
      return true;
    }
    if(a.launcher==='riot') {
      const result=await api('/api/riot/switch-account',{account_id:a.id,display_name:a.display_name});
      accounts.forEach(acc=>{if(acc.launcher==='riot')acc.is_active=acc.id===id;});
      toast(result.message || `Аккаунт Riot Games ${a.display_name||a.username} активирован`);
      return true;
    }
    if(a.launcher==='ubisoft') {
      const result=await api('/api/ubisoft/switch-account',{account_id:a.id});
      toast(result.message);
      return true;
    }
    if(a.launcher==='gog') {
      toast('Переключаем аккаунт GOG GALAXY…');
      const result=await api('/api/gog/switch-account',{account_id:a.id});
      toast(result.message);return true;
    }
    if(a.launcher==='hoyoverse') {
      toast('Переключаем аккаунт HoYoverse...');
      const result=await api('/api/hoyoverse/switch-account',{account_id:a.id});
      accounts.forEach(acc=>{if(acc.launcher==='hoyoverse')acc.is_active=acc.id===id;});
      toast(result.message || 'Аккаунт HoYoverse активирован');
      return true;
    }
    const result=await api(a.launcher==='steam'?'/api/steam/switch-account':'/api/epic/switch',a.launcher==='steam'?{account_name:a.id}:{account_id:a.id});
    accounts.forEach(acc=>{if(acc.launcher===a.launcher)acc.is_active=acc.id===id;});
    if(a.launcher==='epic'){
      currentEpicInventoryAccount=a.id;
      save('swch-epic-inventory-account', currentEpicInventoryAccount);
      void ensureFortniteSummary(currentEpicInventoryAccount);
    }
    toast(result.message || `Аккаунт ${a.display_name||a.username} активирован`);
    return true;
  } catch(e){toast(e.message);return false;} finally{busy=false;await refresh();}
}
async function openLauncher(id, client = null) {
 if(busy)return false;const a=accounts.find(a=>a.id===id);if(!a)return false;
 if(demo){accounts.forEach(acc=>{if(acc.launcher===a.launcher)acc.is_active=acc.id===id;});render();toast(`Демо: открыт ${platformName(a.launcher)} с аккаунтом ${a.display_name||a.username}`);return true;}
 busy=true;render();
 try {
   if(a.launcher==='roblox') {
     const result=await api('/api/roblox/launch',{account_id:a.id});
     accounts.forEach(acc=>{if(acc.launcher==='roblox')acc.is_active=acc.id===id;});
     toast(result.message || 'Roblox запущен');
     return true;
   }
   if(a.launcher==='minecraft') {
     const isTl = (a.account_type || '').startsWith('tlauncher') || a.account_type === 'offline';
     const result=await api('/api/minecraft/open-launcher',{
       account_id: a.id,
      launcher_client: client || (isTl ? 'tlauncher' : 'official')
     });
     accounts.forEach(acc=>{if(acc.launcher==='minecraft')acc.is_active=acc.id===id;});
     toast(result.message || ((client || (isTl ? 'tlauncher' : 'official')) === 'tlauncher' ? 'T-Launcher запущен' : 'Официальный лаунчер Minecraft запущен'));
     return true;
   }
    if(a.launcher==='riot') {
      const result=await api('/api/riot/switch-account',{account_id:a.id,display_name:a.display_name});
      accounts.forEach(acc=>{if(acc.launcher==='riot')acc.is_active=acc.id===id;});
      toast(result.message || 'Riot Client запущен с выбранным аккаунтом');
      return true;
    }
    if(a.launcher==='ubisoft') {
      const result=await api('/api/ubisoft/open-launcher',{account_id:a.id});
      toast(result.message);
      return true;
    }
    if(a.launcher==='gog') {
      const result=await api('/api/gog/open-launcher',{account_id:a.id});toast(result.message);return true;
    }
   const endpoint=a.launcher==='steam'?'/api/steam/open-launcher':'/api/epic/open-launcher';
   const payload = a.launcher === 'steam' ? { account_name: a.id } : { account_id: a.id };
   let result;
   try {
     result = await api(endpoint, payload);
   } catch(e) {
     if (e.message && (e.message.includes('404') || e.message.includes('405'))) {
       result = await api(a.launcher==='steam'?'/api/steam/switch-account':'/api/epic/switch', payload);
     } else {
       throw e;
     }
   }
   accounts.forEach(acc=>{if(acc.launcher===a.launcher)acc.is_active=acc.id===id;});
   toast(result.message || `${platformName(a.launcher)} открыт с выбранным аккаунтом`);
   return true;
 } catch(e){
   toast(e.message);
   return false;
 } finally{busy=false;await refresh();}
}
async function sellSteamItem(id, itemUrl, itemName) {
  if(busy)return false;
  const a=accounts.find(acc=>acc.id===id);
  const accountName=a?(a.display_name||a.username):id;
  if(demo){
    accounts.forEach(acc=>{if(acc.launcher==='steam')acc.is_active=acc.id===id;});
    render();
    toast(`Демо: вход в аккаунт ${accountName} и открытие страницы продажи «${itemName||'предмета'}»`);
    return true;
  }
  busy=true;render();
  const isCurrent = a?.is_active;
  toast(isCurrent ? `Открываем продажу «${itemName||'предмета'}» в Steam…` : `Переключаемся на ${accountName} и открываем продажу в Steam…`);
  try {
    const payload = {
      account_id: id,
      open_url: itemUrl,
    };
    const result = await api('/api/steam/switch-account', payload);
    accounts.forEach(acc=>{if(acc.launcher==='steam')acc.is_active=acc.id===id;});
    toast(result.message || `Steam запущен с аккаунтом ${accountName}`);
    return true;
  } catch(e){
    toast('Ошибка переключения: '+e.message);
    return false;
  } finally{busy=false;await refresh();}
}
function gameState(g) {
 if(g.launcher==='roblox')return g.is_installed?'Готова к запуску':'Клиент не найден';
 if(g.launcher==='minecraft')return g.id.startsWith('minecraft_server_') ? 'Сервер' : (g.is_installed ? 'Готова к запуску' : 'Лаунчер не найден');
 const u=downloadState(g);
 if(u?.phase==='requested')return 'Ожидаем Steam';
 if(u?.phase==='queued')return 'В очереди обновлений';
 if(u?.phase==='preparing')return 'Подготовка обновления';
 if(u?.phase==='downloading')return 'Обновляется';
 if(u?.phase==='installing')return 'Установка обновления';
 if(u?.phase==='verifying')return 'Проверка файлов';
 if(u?.phase==='paused')return 'Обновление приостановлено';
 if(u?.required)return 'Требуется обновление';
 return g.is_installed?'Установлена':'Не установлена';
}
function gameInstallBadge(g){return downloadInProgress(g)||g.update?.required?gameState(g):g.is_installed?'● Установлена':'○ Не установлена';}
const pendingDownloads=new Map();
let downloadsStale=false;
function downloadState(g){
 const pending=pendingDownloads.get(gameKey(g));
 if((pending&&pending>Date.now()||g.launch_after_update?.status==='watching')&&!['downloading','installing','verifying','preparing','paused','queued'].includes(g.update?.phase))return {...g.update,phase:'requested',downloaded:0,download_total:0,staged:0,stage_total:0,telemetry:null};
 return g.update;
}
function downloadInProgress(g){return g.launcher==='steam'&&['requested','queued','preparing','downloading','installing','verifying','paused'].includes(downloadState(g)?.phase);}
function downloadProgress(g){
 const u=downloadState(g);
 if(!u)return {percent:null,done:0,total:0,stage:false};
 const stage=['installing','verifying'].includes(u.phase);
 const done=Number(stage?u.staged:u.downloaded)||0,total=Number(stage?u.stage_total:u.download_total)||0;
 const indeterminate=(stage||u.phase==='downloading')&&done===0;
 return {percent:!indeterminate&&total>0?Math.max(0,Math.min(100,done/total*100)):null,done,total,stage};
}
function coverProgress(g){
 const u=downloadState(g),p=downloadProgress(g);
 // Keep a sliver grey until Steam confirms that all installation work is done.
 const percent=u?.download_total>0?Math.max(0,Math.min(100,u.downloaded/u.download_total*100)):p.percent;
 return !downloadInProgress(g)?100:u.phase==='requested'?0:Math.min(99,percent??0);
}
function downloadCover(g,cls){
 if(g.launcher!=='steam')return imageTag(g,cls);
 return `<span class="download-cover ${downloadInProgress(g)?'is-downloading':''}" data-download-cover="${esc(gameKey(g))}" style="--download-fill:${coverProgress(g)}%">${imageTag(g,cls+' download-cover-base')}${imageTag(g,cls+' download-cover-color')}</span>`;
}
function downloadBytes(value){return value>=1024**3?`${format.format(value/1024**3)} ГБ`:`${format.format(value/1024**2)} МБ`;}
function downloadRate(value){return Number.isFinite(value)&&value>=0?`${downloadBytes(value)}/с`:'—';}
function downloadEta(g){
 const u=downloadState(g),seconds=u?.telemetry?.eta_seconds;
 if(downloadsStale||u?.telemetry?.data_stale)return 'Нет свежих данных';
 if(u?.phase==='paused')return 'На паузе';
 if(u?.phase==='queued'||u?.phase==='requested')return 'Ожидаем начала';
 if(!Number.isFinite(seconds)||seconds<=0)return 'Рассчитываем…';
 const mins=Math.ceil(seconds/60);
 return mins>=60?`≈ ${Math.floor(mins/60)} ч ${mins%60} мин`:`≈ ${mins} мин`;
}
function downloadMetrics(g,compact=false){
 const u=downloadState(g),t=u?.telemetry||{},p=downloadProgress(g);
 const clientRate=!(t.download_bps>0)&&Number.isFinite(t.steam_download_bps);
 const rate=clientRate?t.steam_download_bps:t.download_bps>0?t.download_bps:(Number.isFinite(t.steam_download_bps)?t.steam_download_bps:(t.progress_stale||t.download_bps===0?null:t.download_bps));
 const rateLabel=(clientRate||(!t.download_bps&&Number.isFinite(t.steam_download_bps)))?'Скачивание Steam':'Скачивание';
 if(compact)return `<div class="card-download" role="status"><div><strong>${p.percent==null?'—':format.format(p.percent)+'%'}</strong><span>${p.percent==null?'—':downloadBytes(p.done)} / ${p.total>0?downloadBytes(p.total):'—'}${p.stage?' на диске':''}</span></div><small>${rateLabel}: ${downloadRate(rate)} · ${downloadEta(g)}</small><small>Диск Steam: ↓ ${downloadRate(t.steam_read_bps)} · ↑ ${downloadRate(t.steam_write_bps)}</small>${downloadsStale?'<small class="error-text">Связь со Steam потеряна. Показаны последние данные.</small>':''}</div>`;
 return `<dl class="download-metrics"><div><dt>${rateLabel}</dt><dd>${downloadRate(rate)}</dd></div><div><dt>${p.stage?'Осталось на установку':'Осталось на скачивание'}</dt><dd>${downloadEta(g)}</dd></div><div><dt>Чтение диска · Steam</dt><dd>${downloadRate(t.steam_read_bps)}</dd></div><div><dt>Запись диска · Steam</dt><dd>${downloadRate(t.steam_write_bps)}</dd></div></dl>`;
}
function playLabel(g){if(g.launcher==='gog')return !gogStatus?.launch_supported?'Запуск на Windows':!gogStatus?.client_installed?'Нужен GALAXY':g.is_installed?'Играть':'Установить';if(g.launcher==='riot')return !riotStatus?.client_installed?'Нужен Riot Client':g.is_installed?'Играть':'Установить / открыть';if(g.launcher==='roblox')return 'Играть в Roblox';if(g.launcher==='minecraft')return g.id.startsWith('minecraft_server_') ? 'Войти на сервер' : 'Играть в Minecraft';return downloadState(g)?.phase==='requested'?'Ожидаем Steam':downloadInProgress(g)&&g.update?.phase!=='paused'?'Играть после обновления':g.update?.required?'Обновить и играть':g.is_installed?'Играть':'Установить';}
function renderUpdate(g){
 if(g.launcher!=='steam')return '';
 const u=downloadState(g);
 const intent=g.launch_after_update;
 const launchInfo=intent?`<div class="launch-after-update"><small>${intent.status==='failed'?'Не удалось повторить запуск. Нажми «Играть» ещё раз.':intent.status==='watching'?'Steam проверяет обновления и запуск игры.':'Игра запустится после обновления. SWCH отслеживает завершение.'}</small><button class="secondary" data-action="cancel-steam-launch" data-game="${esc(gameKey(g))}">Отменить запуск</button></div>`:'';
 if(!u||u.phase==='ready')return `<p class="muted small-text">Статус обновления: ${u?'по локальным данным Steam игра готова':'нет локальных данных'}. Новую версию проверяет клиент Steam.</p>${launchInfo}`;
 const {percent:progress,done,total,stage}=downloadProgress(g);
 return `<section class="download-status" role="status"><strong>${esc(gameState(g))}</strong>${progress!=null?`<progress max="100" value="${progress}"></progress><span>${format.format(progress)}% · ${downloadBytes(done)} / ${downloadBytes(total)}${stage?' на диске':''}</span>`:`<progress max="100" aria-label="${esc(gameState(g))}"></progress><p>${stage?'Steam применяет файлы. Точный процент пока недоступен.':u.phase==='requested'?'Проверяем запуск и обновления в Steam…':u.phase==='downloading'?'Steam скачивает игру, но ещё не обновил счётчик загруженных байтов. Точный процент пока недоступен.':'Ожидаем данные о размере загрузки от Steam.'}</p>`}${downloadMetrics(g)}${launchInfo}<button class="secondary" data-action="update-game" data-game="${esc(gameKey(g))}" ${busy||u.phase==='requested'?'disabled':''}>${u.phase==='paused'?'Продолжить обновление':'Обновить без запуска игры'}</button>${downloadsStale?'<small class="error-text">Не удалось получить свежий прогресс. Показаны последние данные.</small>':''}<small>Время приблизительное; после скачивания Steam может продолжить установку. Чтение и запись относятся ко всему клиенту Steam. Steam может запросить вход или подтверждение.</small></section>`;
}
let launchWarningResolver = null;
function confirmLaunchWithBan(g, a) {
 return new Promise(resolve => {
  launchWarningResolver = resolve;
  const dialog = $('launch-warning-dialog');
  $('launch-warning-title').textContent = `⚠️ Бан в ${g.name}`;
  $('launch-warning-subtitle').textContent = `Аккаунт: ${a.display_name || a.username}`;
  $('launch-warning-text').innerHTML = `На аккаунте <strong>${esc(a.display_name || a.username)}</strong> зафиксирована блокировка в игре <strong>${esc(g.name)}</strong>.<br><br>Если запустить игру с этого аккаунта, доступ к сетевой игре или серверам с защитой (VAC/античит) будет заблокирован.<br><br>Ты уверен, что хочешь продолжить запуск?`;
  dialog.showModal();
 });
}
function chooseLaunchAccount(key,updateOnly=false){
 const g=games.find(g=>gameKey(g)===key);if(!g)return;

 if(g.launcher==='minecraft'){
   const serverIp = g.id.startsWith('minecraft_server_') ? (g.namespace || null) : null;
   openMinecraftLaunchModal(serverIp);
   return;
 }
 const list=availableAccounts(g);
 const unbanned=list.filter(a=>!accountHasGameBan(a, g.app_id, g.name, accountRestrictions));
 const launchList=unbanned.length?unbanned:list;
 if(launchList.length<=1){void launch(key,launchList[0]?.id,updateOnly);return;}
 let dialog=$('launch-account-dialog');if(!dialog){dialog=document.createElement('dialog');dialog.id='launch-account-dialog';document.body.append(dialog);}
 dialog.innerHTML=`<div class="dialog-heading"><h2>${updateOnly?'Обновить':!g.is_installed?'Установить':'Играть'} · ${esc(g.name)}</h2><button class="icon-button" data-close aria-label="Закрыть">${icon('close')}</button></div><p class="muted">${g.launcher==='gog'?'SWCH войдёт в выбранный аккаунт GALAXY перед запуском игры. Перед переключением заверши игру.':'Выбери аккаунт — SWCH автоматически сделает его активным.'}</p><div class="launch-account-options">${launchList.map(a=>{
   const isBanned = accountHasGameBan(a, g.app_id, g.name, accountRestrictions);
   return `<button class="account-chip ${isBanned?'is-banned':''}" data-action="launch-account" data-game="${esc(key)}" data-account="${esc(a.id)}" data-update="${updateOnly}">${accountAvatar(a)}<span>${esc(a.display_name||a.username)}${isBanned?'<span class="chip-ban-badge">⚠️ Бан в игре</span>':''}<small>${g.launcher==='ubisoft'?'Доступ проверит клиент':g.launcher==='steam'?esc(g.is_free&&!steamStats(steamRows,g.app_id,a.id)?'Бесплатная игра':accessLabel(steamStats(steamRows,g.app_id,a.id)?.access)):'Своя игра'}${(g.launcher==='gog'?a.launcher_active:a.is_active)?(g.launcher==='gog'?' · В GALAXY':' · Активен'):''}</small></span></button>`;
 }).join('')}</div>`;
 dialog.showModal();
}
async function launch(key,accountId=null,updateOnly=false) {
 if(busy)return;const g=games.find(g=>gameKey(g)===key);if(!g||!canRequestLaunch(g)||downloadState(g)?.phase==='requested')return;
 if(isModPlatform(g.launcher)) {
   busy=true;render();
   try{const account=accountId||chosenAccount(g)?.id;const result=await modOperation(g.launcher,'launch',{game_id:g.id,...(account?{account_id:account}:{})});toast(result?.message||'Запрос на запуск передан.');}catch(e){toast(e.message);}finally{busy=false;await refresh();}return;
 }
 if(g.launcher==='riot'){
   const targetAcc = accountId ? accounts.find(a=>a.id===accountId) : (chosenAccount(g) || accounts.find(a=>a.launcher==='riot'&&a.is_active) || accounts.find(a=>a.launcher==='riot'));
   await launchRiot(g.app_id,targetAcc?.id);
   return;
 }
 if(g.launcher==='roblox'){const placeId=g.app_id==='roblox'?null:g.app_id;await launchRoblox(placeId,accountId);return;}
 if(g.launcher==='ubisoft'){await launchUbisoft(g.is_installed?g.app_id:null,accountId||chosenAccount(g)?.id||null);return;}
 if(g.launcher==='gog'){await launchGog(g.app_id,accountId||chosenAccount(g)?.id||null);return;}
 if(g.launcher==='hoyoverse'){await launchHoyoverse(g.app_id, accountId || chosenAccount(g)?.id || null);return;}
 if(g.launcher==='minecraft'){
   const serverIp = g.id.startsWith('minecraft_server_') ? (g.namespace || null) : null;
   openMinecraftLaunchModal(serverIp, accountId);
   return;
 }
 const a=accountId?availableAccounts(g).find(a=>a.id===accountId):chosenAccount(g);if(!a){toast('Сначала подключи аккаунт с доступом к игре.');return;}const installOrUpdate=updateOnly||(!g.is_installed&&!g.update);
 if(!installOrUpdate && accountHasGameBan(a, g.app_id, g.name, accountRestrictions)) {
  const confirmed = await confirmLaunchWithBan(g, a);
  if(!confirmed) return;
 }
 if(demo){toast('Демо: запуск игры показан для примера. Реальные приложения не запускаются.');return;}
 selectedAccounts[key]=a.id;
 if(g.launcher==='steam')pendingDownloads.set(key,Number.POSITIVE_INFINITY);
 busy=true;render();refreshGameModalDownload();refreshDownloadVisuals();
 try {
  let result;
  if(g.launcher==='steam')result=await api('/api/steam/launch',{app_id:g.app_id,account_id:a.id,update_only:installOrUpdate});
  else if(!g.is_installed){try{result=await api('/api/epic/open-launcher',{account_id:a.id});}catch(e){if(e.message&&(e.message.includes('404')||e.message.includes('405')))result=await api('/api/epic/switch',{account_id:a.id});else throw e;}}else {if(!a.is_active)await api('/api/epic/switch',{account_id:a.id});result=await api('/api/epic/launch',{app_name:g.app_id,account_id:a.id});}
  accounts.forEach(acc=>{if(acc.launcher===a.launcher)acc.is_active=acc.id===a.id;});
  if(pendingDownloads.has(key))pendingDownloads.set(key,Date.now()+60000);
  toast(result.message||'Запрос на запуск передан.');
  if(g.launcher==='steam')void pollDownloads();
 }catch(e){pendingDownloads.delete(key);toast(e.message);}finally{busy=false;await refresh();refreshGameModalDownload();refreshDownloadVisuals();}
}
let downloadPolling=false;
function refreshDownloadVisuals(){
 if(!document.querySelectorAll)return;
 for(const cover of document.querySelectorAll('[data-download-cover]')){
  const g=games.find(g=>gameKey(g)===cover.dataset.downloadCover);if(!g)continue;
  cover.classList.toggle('is-downloading',downloadInProgress(g));
  cover.style.setProperty('--download-fill',`${coverProgress(g)}%`);
 }
}
function refreshLibraryDownloads(){
 if(!document.querySelectorAll){renderLibrary();return;}
 for(const card of document.querySelectorAll('[data-download-game]')){
  const g=games.find(g=>gameKey(g)===card.dataset.downloadGame);if(!g||g.launcher!=='steam')continue;
  const caption=card.querySelector('[data-download-caption]');
  if(caption)caption.innerHTML=`${g.is_installed?'<span class="status-dot"></span>':''}${esc(gameState(g))}${timeCaption(g,steamAccountFilter!=='all'?steamAccountFilter:null)}`;
  const panel=card.querySelector('[data-card-download]');
  if(panel)panel.innerHTML=downloadInProgress(g)?downloadMetrics(g,true):'';
  const button=card.querySelector('.card-play');
  if(button){button.innerHTML=icon('play')+esc(playLabel(g));button.disabled=busy||!canRequestLaunch(g)||downloadState(g)?.phase==='requested';}
 }
 // The details layout has a single status panel and no card wrappers.
 if(!document.querySelector('[data-download-game]'))renderLibrary();
 refreshDownloadVisuals();
}
function refreshGameModalDownload(){
 const modal=$('game-modal');
 if(!modal?.open)return;
 const g=games.find(g=>gameKey(g)===modal.dataset.game);
 if(!g||g.launcher!=='steam')return;
 const status=modal.querySelector('[data-game-update]');
 const html=renderUpdate(g);
 // Keep the modal, scroll position and account controls intact while polling.
 if(status&&status.innerHTML!==html)status.innerHTML=html;
 const launch=modal.querySelector('.game-modal-launch-btn');
 if(launch){launch.innerHTML=icon('play')+esc(playLabel(g));launch.disabled=busy||!canRequestLaunch(g)||downloadState(g)?.phase==='requested';}
 const badge=modal.querySelector('[data-game-install-status]');
 if(badge){
  const updating=downloadInProgress(g)||g.update?.required;
  badge.textContent=gameInstallBadge(g);
  badge.style.background=updating?'rgba(245,158,11,0.15)':g.is_installed?'rgba(46,204,113,0.18)':'rgba(255,255,255,0.06)';
  badge.style.color=updating?'#e5ae60':g.is_installed?'#2ecc71':'var(--muted)';
 }
 hydrateIcons(modal);
}
async function pollDownloads(){
 if(demo||downloadPolling||document.hidden)return;downloadPolling=true;
 try {const data=await api('/api/steam/downloads');if(!data.games)throw new Error('Steam progress unavailable');let changed=downloadsStale;downloadsStale=false;
 for(const [key,expires] of pendingDownloads)if(expires<=Date.now()){pendingDownloads.delete(key);changed=true;}
 for(const g of [...games,...steamRows].filter(g=>g.launcher==='steam')){
  const intent=data.launch_after_update?.app_id===g.app_id?data.launch_after_update:null;
  if(JSON.stringify(g.launch_after_update||null)!==JSON.stringify(intent)){g.launch_after_update=intent;changed=true;}
 }
 for(const [id,state] of Object.entries(data.games)){
  if(state.is_running||['downloading','installing','verifying','preparing'].includes(state.update?.phase)
     ||('launch_after_update' in data&&!data.launch_after_update&&Number.isFinite(pendingDownloads.get(`steam:${id}`))&&state.update?.phase==='ready'))pendingDownloads.delete(`steam:${id}`);
  for(const g of [...games,...steamRows].filter(g=>g.launcher==='steam'&&g.app_id===id))if(JSON.stringify(g.update)!==JSON.stringify(state.update)||g.is_installed!==state.is_installed){g.update=state.update;g.is_installed=state.is_installed;changed=true;}
 }if(changed&&['library','favorites'].includes(page)){refreshLibraryDownloads();hydrateIcons();}refreshGameModalDownload();refreshDownloadVisuals();}
 catch {downloadsStale=true;refreshLibraryDownloads();refreshGameModalDownload();} finally {downloadPolling=false;}
}
async function cancelSteamLaunch(key){
 const g=games.find(g=>gameKey(g)===key);if(!g||g.launcher!=='steam')return;
 try{
  await api('/api/steam/cancel-launch',{app_id:g.app_id});
  pendingDownloads.delete(key);
  for(const row of [...games,...steamRows].filter(row=>row.launcher==='steam'&&row.app_id===g.app_id))row.launch_after_update=null;
  refreshLibraryDownloads();refreshGameModalDownload();refreshDownloadVisuals();
  toast('Автозапуск отменён. Обновление продолжится в Steam.');
 }catch(e){toast(e.message);}
}
 document.addEventListener('click',async e=>{
  if(e.target.id==='custom-accent-color'){updateCustomAccentColor(e.target.value);return;}
  const b=e.target.closest('button');if(!b)return;
  if(b.hasAttribute('data-close')||b.dataset.action==='finish-connect'){const dialog=b.closest('dialog');if(dialog.id==='connect-dialog'&&!await cancelUbisoftAutoLogin())return;dialog.close();return;}
  if(b.dataset.tab){
    switchConnectTab(b.dataset.tab);
    return;
  }
  if(b.id==='open-epic-browser-btn'){
    await startEpicBrowserLogin();
    return;
  }
  if(b.id==='cancel-epic-browser-btn'){
    await cancelEpicBrowserLogin();
    return;
  }
  if(b.id==='epic-manual-browser-btn'){
    b.disabled=true;
    try {
      toast('Открываем страницу входа Epic Games в браузере...');
      await api('/api/epic/auth/open',{});
      toast('Страница открыта в браузере. Авторизуйся и скопируй полученный код или JSON.');
    } catch(err) {
      toast('Не удалось открыть браузер: ' + (err?.message || err));
      $('epic-auth-link')?.click();
    } finally {
      b.disabled=false;
    }
    return;
  }
  if(b.dataset.designChoice){
   const requested=b.dataset.designChoice;
   const isMac=isMacPlatform();
   if(DESIGNS[requested]?.macOnly && !isMac){
     toast('Дизайн в стиле Apple доступен только на macOS');return;
   }
   savedDesign=requested;design=resolveDesign(savedDesign,isMac);
   save('swch-design',savedDesign);
   applyStyle();renderColorOptions();showStyles();render();
   toast(design==='apple'?'Дизайн Apple (macOS) применён':'Классический дизайн SWCH применён');return;
  }
  if(b.dataset.colorChoice){
   hasSavedAccent=true;
   accentColor=b.dataset.colorChoice;save('swch-accent-color',accentColor);
   applyStyle();renderColorOptions();toast('Цвет интерфейса обновлён');return;
 }
 if(b.dataset.layoutChoice){
   const newLayout=b.dataset.layoutChoice;
   layout=newLayout;style=newLayout==='cards'?'epic':'steam';
   save('swch-layout',layout);save('swch-style',style);
   applyStyle();showStyles();render();
   toast(newLayout==='cards'?'Режим «Карточки» включён':'Режим «Список» включён');return;
 }
 if(b.dataset.styleChoice){style=b.dataset.styleChoice;layout=STYLES[style].layout;save('swch-style',style);save('swch-layout',layout);applyStyle();$('styles-dialog').close();render();toast(`Оформление ${STYLES[style].name} применено`);return;}
 if(b.dataset.layout){layout=b.dataset.layout;save('swch-layout',layout);render();return;}
 if(b.dataset.inventoryTag !== undefined){
   inventoryTag=b.dataset.inventoryTag;
   renderInventoryItems();
   document.querySelectorAll('[data-inventory-tag]').forEach(chip => {
     chip.classList.toggle('active', chip.dataset.inventoryTag === inventoryTag);
   });
   return;
 }
  if(b.dataset.inventoryLauncher){
    inventoryLauncher=b.dataset.inventoryLauncher;
    save('swch-inventory-launcher',inventoryLauncher);
    if(inventoryLauncher==='epic'){
      const epicList=accounts.filter(a=>a.launcher==='epic');
      if(epicList.length&&(!currentFortniteSummary||currentFortniteSummary.account_id!==(currentEpicInventoryAccount||epicList[0].id))){
        void ensureFortniteSummary(currentEpicInventoryAccount||epicList[0].id);
      }
    }
    renderInventory();
    return;
  }
  if(b.dataset.fortniteTab){
    currentFortniteTab=b.dataset.fortniteTab;
    document.querySelectorAll('[data-fortnite-tab]').forEach(btn => {
      btn.classList.toggle('active', btn.dataset.fortniteTab === currentFortniteTab);
    });
    updateFortniteItems();
    return;
  }
  if(b.dataset.page){
    page=b.dataset.page;
    if(page==='accounts'||page==='library') platform='all';
    render();
    if(page==='inventory'){
      if(inventoryLauncher==='epic'){
        const epicList=accounts.filter(a=>a.launcher==='epic');
        const accId=currentEpicInventoryAccount||epicList[0]?.id||(demo?'demo-epic':'');
        if(accId) void ensureFortniteSummary(accId);
      } else {
        void loadInventory();
      }
    }
    return;
  }
  if(b.dataset.platform){
    gogAccountFilter='all';
    const targetPlat = b.dataset.platform;
    if(page==='accounts'){
      platform = targetPlat;
    } else {
      platform = targetPlat;
      page = 'library';

    }
    render();
    return;
  }
  if(b.dataset.filter){installed=b.dataset.filter;render();return;}
  const action=b.dataset.action,key=b.dataset.game;
  if(action==='scan-steam-accounts'){
    if(busy||demo)return;
    busy=true;
    b.disabled=true;
    $('steam-scan-status').textContent='Ищем сохранённые аккаунты…';
    try{
      const data=await api('/api/steam/library');
      applySteam(data);
      persistLibraryCache();
      renderSteamConnect();
      updateConnectTabBadges();
      $('steam-scan-status').textContent=data.warnings?.length?data.warnings.join(' '):accounts.some(a=>a.launcher==='steam')?'Поиск завершён. Найденные профили доступны в разделе «Аккаунты».':'Аккаунты не найдены. Войди в Steam с «Запомнить меня» и повтори поиск.';
    }catch(error){$('steam-scan-status').textContent=error.message;}
    finally{busy=false;b.disabled=false;render();}
    return;
  }
  if(action==='reset-account-filter'){platform='all';render();return;}
  if(action==='account-games'){
 const account=accounts.find(a=>a.id===b.dataset.account);if(!account)return;
 if(!games.some(g=>g.launcher===account.launcher)){toast('В библиотеке аккаунта пока нет игр. Синхронизация доступна в «Управлении».');return;}
 platform=account.launcher;page='library';query='';installed='all';steamAccessFilter='all';$('search-input').value='';
 steamAccountFilter=account.launcher==='steam'?account.id:'all';
 gogAccountFilter=account.launcher==='gog'?account.id:'all';
 const related=games.filter(g=>g.launcher===account.launcher&&(g.launcher==='riot'||availableAccounts(g).some(a=>a.id===account.id)));
 related.forEach(g=>{selectedAccounts[gameKey(g)]=account.id;});selectedGame=related.length?gameKey(related[0]):null;
 if(account.launcher==='riot'){layout='detail';}render();return;
 }
 if(action==='select-riot-game-account'){selectedAccounts[b.dataset.game]=b.dataset.account;if($('game-modal')?.open)openGameModal(b.dataset.game);render();return;}
  if(action==='sync-riot'){void syncRiotProfile(b.dataset.app,b.dataset.account||null);return;}
  if(action==='sync-all-riot'){void syncAllRiotProfiles();return;}
  if(action==='select-riot-inventory-account'){riotInventoryAccount=b.dataset.account;save('swch-riot-inventory-account',riotInventoryAccount);riotInventorySearch='';renderInventory();return;}
  if(action==='riot-inventory'){openRiotInventory(b.dataset.account,b.dataset.app);return;}
  if(action==='riot-collection'){openRiotCollection(b.dataset.account,b.dataset.app);return;}
  if(action==='open-riot'){void launchRiot();return;}
  if(action==='open-ubisoft'){await launchUbisoft();return;}
  if(action==='open-gog'){await launchGog();return;}
  if(action==='sync-gog'){await syncGog(b.dataset.account||(gogAccountFilter==='all'?null:gogAccountFilter));return;}
  if(action==='gog-auth'){
    if(demo){toast('В демо вход отключён.');return;}
    b.disabled=true;$('gog-login-error').textContent='';
    try{toast((await api('/api/gog/auth/open',{})).message);}catch(e){$('gog-login-error').textContent=e.message;}finally{b.disabled=false;}
    return;
  }
  if(action==='remove-gog'){
    if(demo||busy)return;busy=true;render();
    try{toast((await api('/api/gog/accounts/remove',{account_id:b.dataset.account})).message);}
    catch(e){toast(e.message);}finally{busy=false;await refresh();}return;
  }
  if(action==='prepare-ubisoft'){await prepareUbisoftLogin();return;}
  if(action==='cancel-ubisoft-login'){await cancelUbisoftAutoLogin();return;}
  if(action==='remove-ubisoft'){
    if(demo||busy)return;
    busy=true;render();
    try{const result=await api('/api/ubisoft/accounts/remove',{account_id:b.dataset.account});toast(result.message);}
    catch(e){toast(e.message);}finally{busy=false;await refresh();}
    return;
  }
  if(action==='open-hoyoverse'){await launchHoyoverse();return;}
  if(action==='connect-hoyoverse'){await connect('hoyoverse');return;}
  if(action==='save-hoyoverse-session'){
    if(demo||busy)return;busy=true;
    const game = $('hoyo-game-select')?.value || 'genshin';
    const name = $('hoyoverse-account-name')?.value?.trim() || null;
    try {
      const res = await api('/api/hoyoverse/sessions/save', {game, display_name: name});
      toast(res.message);
      if ($('hoyoverse-account-name')) $('hoyoverse-account-name').value = '';
    } catch(e){ toast('Ошибка сохранения: ' + e.message); }
    finally{ busy=false; await refresh(); }
    return;
  }
  if(action==='prepare-hoyoverse-login'){
    if(demo||busy)return;busy=true;
    const game = $('hoyo-game-select')?.value || 'genshin';
    try {
      const res = await api('/api/hoyoverse/accounts/prepare', {game});
      toast(res.message);
    } catch(e){ toast('Ошибка: ' + e.message); }
    finally{ busy=false; await refresh(); }
    return;
  }
  if(action==='remove-hoyoverse'){
    if(demo||busy)return;busy=true;render();
    try {
      await api('/api/hoyoverse/accounts/remove', {account_id: b.dataset.account});
      toast('Аккаунт HoYoverse удален');
    } catch(e){ toast(e.message); }
    finally{ busy=false; await refresh(); }
    return;
  }
  if(action==='select-inventory-app'){inventoryApp=b.dataset.app;inventoryContext='all';inventorySearch='';inventoryTag='';inventoryFacets={};heroSearchQuery='';renderInventory();void loadInventory();return;}
  if(action==='riot-weapon-category'){riotInventoryKind='skins';riotWeaponCategory=b.dataset.value;riotWeapon='';renderInventory();return;}
  if(action==='cs-inventory-view'){csInventoryView=b.dataset.view==='details'?'details':'visual';save('swch-cs-inventory-view',csInventoryView);renderInventory();return;}
  if(action==='cs-inventory-category'){
    for(const key of Object.keys(inventoryFacets))if(key.startsWith('730:')&&['type','weapon'].includes(key.split(':')[1].toLowerCase()))delete inventoryFacets[key];
    if(b.dataset.value)inventoryFacets[b.dataset.facetKey]=[b.dataset.value];
    inventoryTag='';renderInventory();return;
  }
  if(action==='select-hero-facet'){
    resetDotaDependentFilters();
    const heroVal = b.dataset.hero;
    if(heroVal==='all'){
      for(const k of Object.keys(inventoryFacets)){
        if(k.toLowerCase().endsWith(':hero'))delete inventoryFacets[k];
      }
    } else {
      const facetKey=b.dataset.facetKey;
      if(!inventoryFacets[facetKey])inventoryFacets[facetKey]=[];
      const idx=inventoryFacets[facetKey].indexOf(heroVal);
      if(idx!==-1){
        inventoryFacets[facetKey].splice(idx,1);
        if(!inventoryFacets[facetKey].length)delete inventoryFacets[facetKey];
      } else {
        if(e.ctrlKey||e.metaKey||e.shiftKey){
          inventoryFacets[facetKey].push(heroVal);
        } else {
          inventoryFacets[facetKey]=[heroVal];
        }
      }
    }
    renderInventory();
    return;
  }
  if(action==='reset-hero-facet'){
    resetDotaDependentFilters();
    for(const k of Object.keys(inventoryFacets)){
      if(k.toLowerCase().endsWith(':hero'))delete inventoryFacets[k];
    }
    renderInventory();
    return;
  }
  if(action==='currency-refresh'){await loadCurrencies();return;}
  if(action==='library-value'){await estimateLibrary(b.dataset.account);return;}
  if(action==='library-value-stop'){if(libraryPriceJob)libraryPriceJob.cancelled=true;return;}
  if(action==='inventory'){openInventory(b.dataset.account,b.dataset.app);return;}
  if(action==='inventory-reset'){
    inventorySearch='';inventoryTag='';inventoryAvailability='all';inventoryMarket='all';inventoryFacets={};inventoryMinPrice='';inventoryMaxPrice='';heroSearchQuery='';
    currentFortniteSearch='';currentFortniteRarity='all';currentFortniteTab='all';
    renderInventory();
    return;
  }
  if(action==='inventory-load'){await loadInventory(false,false);return;}
  if(action==='inventory-refresh'){await loadInventory(true,false);return;}
  if(action==='inventory-more'){await loadInventory(false,true);return;}
  if(action==='inventory-all'){await loadInventory(false,false,true);return;}
  if(action==='inventory-stop'){if(inventoryJob)inventoryJob.cancelled=true;return;}
  if(action==='inventory-value'){await estimateInventory();return;}
  if(action==='inventory-value-stop'){if(pricingJob)pricingJob.cancelled=true;return;}
  if(action==='steam-api-page'){
    const id=b.dataset.account;
    if(!id||busy)return;
    if(demo){toast('Демо: переключение на выбранный аккаунт и открытие ключа в Steam.');return;}
    const account=accounts.find(acc=>acc.launcher==='steam'&&acc.id===id);
    if(!account){toast('Выбранный аккаунт Steam не найден. Обнови библиотеку.');return;}
    busy=true;b.disabled=true;
    toast(`Переключаем Steam на ${account.display_name||account.username||id} и открываем страницу Steam Web API…`);
    try{
      const result=await api('/api/steam/switch-account',{account_id:id,open_url:'https://steamcommunity.com/dev/apikey'});
      if(result.steam_id&&result.steam_id!==id)throw new Error('Steam выбрал другой аккаунт. Страницу ключа не используй.');
      accounts.forEach(acc=>{if(acc.launcher==='steam')acc.is_active=acc.id===id;});
      toast(`${result.message||'Запрос на открытие страницы отправлен.'} Сверь аккаунт ${account.display_name||account.username||id} в Steam перед копированием ключа.`);
    }catch(e){toast(e.message);}
    finally{busy=false;b.disabled=false;await refresh();}
    return;
  }
  if(action==='steam-ban-page'){
    const id=b.dataset.account;
    if(!id||busy)return;
    busy=true;b.disabled=true;
    toast('Переключаем Steam на выбранный аккаунт и открываем страницу блокировок…');
    try{
      await api('/api/steam/switch-account',{account_id:id,open_url:'https://help.steampowered.com/en/wizard/VacBans'});
      accounts.forEach(acc=>{if(acc.launcher==='steam')acc.is_active=acc.id===id;});
      toast(`Страница блокировок открывается в Steam. Сверь аккаунт ${id} в клиенте.`);
    }catch(e){toast(e.message);}
    finally{busy=false;b.disabled=false;await refresh();}
    return;
  }
  if(action==='game-stats-refresh'||action==='game-stats-key'){
    const input=b.closest('[data-game-stat-panel]')?.querySelector('[data-game-stats-key]');
    const key=action==='game-stats-key'?input?.value.trim():undefined;
    if(action==='game-stats-key'&&!key){toast('Введи ключ Steam Web API.');return;}
    if(input)input.value='';
    await loadGameStats(b.dataset.app,b.dataset.account,true,key);return;
  }
  if(action==='steam-connect'){await connect('steam');return;}
  if(action==='open-fortnite'){
    $('game-modal')?.close();
    inventoryLauncher='epic';
    save('swch-inventory-launcher',inventoryLauncher);
    if(b.dataset.account) {
      currentEpicInventoryAccount=b.dataset.account;
      save('swch-epic-inventory-account', currentEpicInventoryAccount);
      if(!demo) void api('/api/epic/switch', { account_id: currentEpicInventoryAccount }).catch(()=>{});
    }
    page='inventory';
    render();
    void ensureFortniteSummary(currentEpicInventoryAccount);
    return;
  }
  if(action==='fortnite-refresh'){
    const accId=b.dataset.account||currentFortniteSummary?.account_id;
    if(page==='inventory'&&inventoryLauncher==='epic'){
      await ensureFortniteSummary(accId,true);
    } else {
      await openFortniteLocker(accId,true);
    }
    return;
  }
 if(action==='styles'||action==='settings'){showStyles();return;}
 if(action==='refresh'){if(platform==='gog')await syncGog(gogAccountFilter==='all'?null:gogAccountFilter);else await refresh(true);}
 if(action==='connect'){await connect(b.dataset.connectTab||b.dataset.tab||(PLATFORMS[platform]&&!isModPlatform(platform)?platform:'steam'));return;}
 if(action==='demo'){const u=new URL(location.href);u.searchParams.set('demo','1');location.href=u.href;}
 if(action==='reset'){gogAccountFilter='all';platform='all';installed='all';steamAccountFilter='all';steamAccessFilter='all';query='';page='library';$('search-input').value='';render();}
 if(action==='open-game'||action==='select-game'){
   selectedGame=key;save(`${namespace}-selected-game`,key);recent[key]=Date.now();save(`${namespace}-recent`,recent);
   if(action==='open-game'){
     if(layout==='cards'||style==='epic'){
       openGameModal(key);
       render();
       return;
     }
     layout='detail';
   }
   render();
   return;
 }
 if(action==='favorite'){
   favorites=favorites.includes(key)?favorites.filter(k=>k!==key):[...favorites,key];
   save(`${namespace}-favorites`,favorites);
   if($('game-modal')?.open)openGameModal(selectedGame);
   render();
   return;
 }
 if(action==='select-account'){
   selectedAccounts[selectedGame]=b.dataset.account;
   if($('game-modal')?.open)openGameModal(selectedGame);
   render();
   return;
 }
 if(action==='metrics')editMetrics();
  if(b.dataset.removeBan!=null){
    const idx=parseInt(b.dataset.removeBan,10);
    if(!Number.isNaN(idx)&&idx>=0&&idx<currentEditingGameBans.length){
      if(isVerifiedGameBan(currentEditingRestrictions, currentEditingGameBans[idx])){toast('Подтверждённый Steam бан удалить нельзя.');return;}
      currentEditingGameBans.splice(idx,1);
      renderEditingGameBans();
    }
    return;
  }
  if(action==='card-ban'){
    const g=games.find(x=>gameKey(x)===key);
    if(!g)return;
    if(steamAccountFilter!=='all'){
      await toggleAccountGameBan(steamAccountFilter, g.app_id, g.name);
    } else {
      openQuickBanDialog(g);
    }
    return;
  }
  if(action==='toggle-game-ban'){
    const accountId = b.dataset.account;
    const g = games.find(x => gameKey(x) === key);
    if (!g || !accountId) return;
    await toggleAccountGameBan(accountId, g.app_id, g.name);
    if($('game-modal')?.open)openGameModal(selectedGame);
    return;
  }
  if(action==='quick-dialog-toggle-ban'){
    const accountId = b.dataset.account;
    const g = games.find(x => gameKey(x) === key);
    if (!g || !accountId) return;
    await toggleAccountGameBan(accountId, g.app_id, g.name);
    openQuickBanDialog(g);
    return;
  }
  if(action==='manage-restrictions'){openRestrictionsDialog(b.dataset.account);return;}
  if(action==='check-restrictions-key'){
    const input=b.closest('details')?.querySelector('[data-ban-api-key]');
    const key=input?.value.trim();
    if(!key){toast('Введи ключ Steam Web API.');return;}
    input.value='';
    await checkAccountRestrictions(b.dataset.account,key);
    return;
  }
  if(action==='check-restrictions'){await checkAccountRestrictions(b.dataset.account);return;}
  if(action==='switch')await switchAccount(b.dataset.account);
  if(action==='open-launcher')await openLauncher(b.dataset.account, b.dataset.client || null);
  if(action==='sell-steam-item'){await sellSteamItem(b.dataset.account,b.dataset.url,b.dataset.name);return;}
  if(action==='sync-epic'||action==='sync-epic-active'){const targetId=b.dataset.account||accounts.find(a=>a.launcher==='epic')?.id;if(targetId)await syncEpic(targetId);return;}
  if(action==='remove-epic'){const targetId=b.dataset.account;if(targetId)await removeEpic(targetId);return;}
  if(action==='switch-roblox')await switchAccount(b.dataset.account);
  if(action==='remove-roblox')await removeRoblox(b.dataset.account);
  if(action==='remove-riot'){await removeRiot(b.dataset.account);return;}
  if(action==='switch-minecraft')await switchAccount(b.dataset.account);
  if(action==='remove-minecraft')await removeMinecraft(b.dataset.account);
  if(action==='switch-riot')await switchAccount(b.dataset.account);
  if(action==='save-riot-session')await saveRiotSession();
  if(action==='scan-minecraft')await scanMinecraft();
  if(action==='open-minecraft-account-dialog')openMinecraftAccountDialog();
  if(action==='open-minecraft-server-dialog')openMinecraftServerDialog();
  if(action==='import-minecraft-servers')await importMinecraftServers();
  if(action==='minecraft-launch-account')openMinecraftLaunchModal(null, b.dataset.account);
  if(action==='roblox-inventory')await openRobloxInventory(b.dataset.account);
  if(action==='roblox-refresh-inventory')await openRobloxInventory(b.dataset.account,true);
  if(action==='scan-roblox')await scanRoblox();
  if(action==='open-roblox-place-dialog')openRobloxPlaceDialog();
  if(action==='roblox-tab-refresh')await refreshRobloxTab(b.dataset.account);
  if(action==='cancel-steam-launch')await cancelSteamLaunch(key);
  if(action==='launch')await launch(key);
  if(action==='quick-launch')chooseLaunchAccount(key);
  if(action==='update-game')chooseLaunchAccount(key,true);
  if(action==='launch-account'){$('launch-account-dialog').close();await launch(key,b.dataset.account,b.dataset.update==='true');}
  if(action==='back-cards'){layout='cards';style='epic';save('swch-layout',layout);save('swch-style',style);applyStyle();render();return;}
});
async function syncEpic(accountId){
  if(busy)return;busy=true;render();toast('Синхронизируем библиотеку Epic Games…');
  try{const res=await api('/api/epic/library/sync',{account_id:accountId});toast(`Синхронизировано ${res.base_games_count||res.total_entitlements||0} игр Epic Games`);await refresh();}
  catch(err){toast('Ошибка синхронизации: '+err.message);}finally{busy=false;render();}
}
async function removeEpic(accountId){
  if(!confirm('Отключить этот аккаунт Epic Games?'))return;
  if(busy)return;busy=true;render();
  try{await api('/api/epic/remove-account',{account_id:accountId});toast('Аккаунт Epic Games отключён');await refresh();}
  catch(err){toast('Ошибка: '+err.message);}finally{busy=false;render();}
}
async function removeRiot(accountId) {
  if (demo || busy) return;
  const account = accounts.find(a => a.launcher === 'riot' && a.id === accountId);
  if (!account || !confirm(`Удалить аккаунт ${account.display_name || account.username} из SWCH? Сохранённая сессия и профили игр будут удалены.`)) return;
  busy = true; render();
  try {
    await api('/api/riot/accounts/remove', { account_id: accountId });
    accounts = accounts.filter(a => a.launcher !== 'riot' || a.id !== accountId);
    riotProfiles = riotProfiles.filter(p => p.account_id !== accountId);
    for (const key of Object.keys(selectedAccounts)) if (selectedAccounts[key] === accountId) delete selectedAccounts[key];
    updateConnectTabBadges();
    toast('Аккаунт Riot удалён из SWCH');
    await refresh(true);
  } catch (error) { toast('Ошибка удаления Riot: ' + error.message); }
  finally { busy = false; render(); }
}
async function scanRoblox() {
  if (demo) { toast('В демо сканирование недоступно.'); return; }
  const statusEl = $('roblox-scan-status');
  if (statusEl) statusEl.textContent = 'Поиск сессий в браузерах и клиенте Roblox...';
  try {
    const res = await api('/api/roblox/accounts/scan', {});
    const count = res.newly_discovered || 0;
    const total = res.total_accounts || 0;
    if (statusEl) statusEl.textContent = `Найдено новых: ${count}. Всего аккаунтов: ${total}.`;
    toast(`Поиск завершён. Новых аккаунтов: ${count}, всего: ${total}`);
    await refresh();
  } catch(e) {
    if (statusEl) statusEl.textContent = 'Ошибка сканирования: ' + e.message;
    toast('Ошибка поиска Roblox: ' + e.message);
  }
}
async function removeRoblox(accountId) {
  if (!confirm('Отключить этот аккаунт Roblox?')) return;
  if (busy) return; busy = true; render();
  try {
    await api('/api/roblox/accounts/remove', { account_id: accountId });
    toast('Аккаунт Roblox отключён');
    await refresh();
  } catch(err) {
    toast('Ошибка: ' + err.message);
  } finally {
    busy = false; render();
  }
}
async function scanMinecraft() {
  if (demo) { toast('В демо сканирование недоступно.'); return; }
  const statusEl = $('minecraft-scan-status');
  if (statusEl) statusEl.textContent = 'Поиск аккаунтов, профилей и серверов Minecraft...';
  try {
    const res = await api('/api/minecraft/accounts/scan', {});
    const msg = `Найдено: ${res.accounts_found} аккаунтов, ${res.profiles_found} профилей, ${res.servers_found} серверов, ${res.worlds_found} миров`;
    if (statusEl) statusEl.textContent = msg;
    toast(msg);
    await refresh();
  } catch(e) {
    if (statusEl) statusEl.textContent = 'Ошибка сканирования: ' + e.message;
    toast('Ошибка поиска Minecraft: ' + e.message);
  }
}
async function removeMinecraft(accountId) {
  if (!confirm('Отключить этот аккаунт Minecraft от SWCH?')) return;
  if (busy) return; busy = true; render();
  try {
    await api('/api/minecraft/accounts/remove', { account_id: accountId });
    toast('Аккаунт Minecraft отключён');
    await refresh();
  } catch(err) {
    toast('Ошибка: ' + err.message);
  } finally {
    busy = false; render();
  }
}
async function importMinecraftServers() {
  if (demo) { toast('В демо импорт недоступен.'); return; }
  try {
    const res = await api('/api/minecraft/servers/import', {});
    toast(res.message || `Импортировано серверов: ${res.imported}`);
    await refresh();
  } catch(err) {
    toast('Ошибка импорта серверов: ' + (err.message || err));
  }
}
function openMinecraftLaunchModal(serverIp = null, accountId = null) {
  const dialog = $('minecraft-launch-dialog');
  if (!dialog) return;

  const launcherSelect = $('minecraft-launch-launcher-select');
  const accountSelect = $('minecraft-launch-account-select');
  const profileSelect = $('minecraft-launch-profile-select');
  const serverInput = $('minecraft-launch-server-ip');
  const banner = $('minecraft-launch-quickplay-banner');
  const bannerDest = $('minecraft-launch-quickplay-dest');
  const errEl = $('minecraft-launch-error');
  if (errEl) errEl.textContent = '';

  const mcAccounts = accounts.filter(a => a.launcher === 'minecraft');
  if (accountSelect) {
    if (mcAccounts.length === 0) {
      accountSelect.innerHTML = '<option value="">Нет аккаунтов (нажми «Авто-поиск»)</option>';
    } else {
      accountSelect.innerHTML = mcAccounts.map(a => {
        const isSel = accountId ? a.id === accountId : a.is_active;
        const isTl = (a.account_type || '').startsWith('tlauncher') || a.account_type === 'offline';
        const typeBadge = isTl ? 'T-Launcher' : (a.account_type || 'msa');
        return `<option value="${esc(a.id)}" data-type="${esc(a.account_type || '')}" ${isSel ? 'selected' : ''}>${esc(a.username || a.display_name)} (${typeBadge})${a.is_active ? ' [Активен]' : ''}</option>`;
      }).join('');
    }
  }

  const selectedAccId = accountSelect?.value || accountId;
  const selectedAcc = mcAccounts.find(a => a.id === selectedAccId);
  if (launcherSelect) {
    const hasOfficial = minecraftStatus ? minecraftStatus.official_installed : true;
    const hasTl = minecraftStatus ? minecraftStatus.tlauncher_installed : true;
    launcherSelect.innerHTML = `
      <option value="auto">Автовыбор (по типу аккаунта)</option>
      <option value="official">Официальный Minecraft Launcher${hasOfficial ? '' : ' (не найден)'}</option>
      <option value="tlauncher">T-Launcher${hasTl ? '' : ' (не найден)'}</option>
    `;
    const isTl = selectedAcc && ((selectedAcc.account_type || '').startsWith('tlauncher') || selectedAcc.account_type === 'offline');
    if (isTl && hasTl) {
      launcherSelect.value = 'tlauncher';
    } else if (!isTl && hasOfficial) {
      launcherSelect.value = 'official';
    } else if (hasTl) {
      launcherSelect.value = 'tlauncher';
    } else if (hasOfficial) {
      launcherSelect.value = 'official';
    } else {
      launcherSelect.value = 'auto';
    }
  }

  if (profileSelect) {
    if (minecraftProfiles.length === 0) {
      profileSelect.innerHTML = '<option value="">Последняя версия (Latest Release)</option>';
    } else {
      profileSelect.innerHTML = minecraftProfiles.map(p => {
        return `<option value="${esc(p.id)}">${esc(p.name)} (${esc(p.version_id || p.profile_type)})</option>`;
      }).join('');
    }
  }

  if (serverIp) {
    if (serverInput) serverInput.value = serverIp;
    if (banner && bannerDest) {
      bannerDest.textContent = serverIp;
      banner.style.display = 'block';
    }
  } else {
    if (serverInput) serverInput.value = '';
    if (banner) banner.style.display = 'none';
  }

  dialog.showModal();
}
function openMinecraftAccountDialog() {
  const dlg = $('minecraft-account-dialog');
  if (!dlg) return;
  const input = $('minecraft-account-username-input');
  if (input) input.value = '';
  const errEl = $('minecraft-account-error');
  if (errEl) errEl.textContent = '';
  dlg.showModal();
}
function openMinecraftServerDialog(server = null) {
  const dialog = $('minecraft-server-dialog');
  if (!dialog) return;
  const idInput = $('minecraft-server-id');
  const nameInput = $('minecraft-server-name-input');
  const ipInput = $('minecraft-server-ip-input');
  const favInput = $('minecraft-server-fav-input');
  const errEl = $('minecraft-server-error');
  if (errEl) errEl.textContent = '';

  if (server) {
    if (idInput) idInput.value = server.id;
    if (nameInput) nameInput.value = server.name;
    if (ipInput) ipInput.value = server.ip;
    if (favInput) favInput.checked = Boolean(server.is_favorite);
  } else {
    if (idInput) idInput.value = '';
    if (nameInput) nameInput.value = '';
    if (ipInput) ipInput.value = '';
    if (favInput) favInput.checked = false;
  }
  dialog.showModal();
}
function openRobloxPlaceDialog() {
  const dlg = $('roblox-place-dialog');
  if (dlg) {
    $('roblox-place-input').value = '';
    $('roblox-place-error').textContent = '';
    dlg.showModal();
  }
}
async function openRobloxInventory(accountId, forceRefresh = false) {
  const dlg = $('roblox-inventory-dialog');
  if (!dlg) return;
  const acc = accounts.find(a => a.id === accountId && a.launcher === 'roblox') || accounts.find(a => a.launcher === 'roblox');
  if (!acc) return;

  $('roblox-inv-title').textContent = `Инвентарь @${acc.username}`;
  $('roblox-inv-subtitle').textContent = `Коллекционные предметы, надетый скин и RAP`;
  const body = $('roblox-inv-body');
  body.innerHTML = '<p class="muted">Загрузка предметов и оценка RAP...</p>';
  dlg.showModal();

  try {
    const data = forceRefresh
      ? await api('/api/roblox/inventory/refresh', { account_id: acc.id })
      : await api(`/api/roblox/inventory?account_id=${encodeURIComponent(acc.id)}`);

    const totalRap = data.total_rap != null ? new Intl.NumberFormat(getLocale()).format(data.total_rap) : '0';
    const collectibles = data.collectibles || [];
    const wearing = data.wearing_asset_ids || [];

    body.innerHTML = `
      <div style="display:flex;justify-content:space-between;align-items:center;padding:12px;background:var(--bg-elevated);border-radius:8px;">
        <div>
          <span style="font-size:13px;color:var(--text-muted);">Общая стоимость (RAP):</span>
          <strong style="display:block;font-size:20px;color:var(--accent,#f05a22);">💎 ${totalRap} R$</strong>
        </div>
        <button class="secondary" data-action="roblox-refresh-inventory" data-account="${esc(acc.id)}">
          ${icon('refresh')}Обновить с сервера
        </button>
      </div>

      <h4 style="margin-top:10px;">Коллекционные предметы (Collectibles / Limiteds) · ${collectibles.length}</h4>
      <div class="roblox-inv-grid">
        ${collectibles.length ? collectibles.map(item => `
          <div class="roblox-item-card">
            ${item.thumbnail_url ? `<img src="${esc(item.thumbnail_url)}" alt="" loading="lazy">` : '<div style="width:80px;height:80px;background:var(--bg-card);border-radius:6px;"></div>'}
            <div class="roblox-item-name" title="${esc(item.name)}">${esc(item.name)}</div>
            <div class="roblox-item-rap">💎 RAP: ${new Intl.NumberFormat(getLocale()).format(item.recent_average_price || 0)} R$</div>
            ${item.serial_number ? `<small class="muted">#${item.serial_number}</small>` : ''}
          </div>
        `).join('') : '<p class="muted small-text">Коллекционных предметов (Limiteds) в инвентаре нет.</p>'}
      </div>

      <h4 style="margin-top:14px;">Надетые предметы аватара (Asset IDs) · ${wearing.length}</h4>
      <div style="display:flex;flex-wrap:wrap;gap:6px;">
        ${wearing.length ? wearing.map(id => `
          <a href="https://www.roblox.com/catalog/${id}" target="_blank" rel="noopener noreferrer" class="badge" style="text-decoration:none;padding:4px 8px;background:var(--bg-elevated);border-radius:4px;font-size:11px;">
            ID: ${id} ↗
          </a>
        `).join('') : '<p class="muted small-text">Надетые предметы не загружены.</p>'}
      </div>
    `;
    hydrateIcons(body);
  } catch(e) {
    body.innerHTML = `<p class="error-text">Не удалось загрузить инвентарь: ${esc(e.message)}</p>`;
  }
}
let currentRobloxInventoryAccount = read('swch-roblox-inventory-account', null);
const robloxInventories = new Map();

async function refreshRobloxTab(accountId) {
  const accId = accountId || currentRobloxInventoryAccount;
  if (!accId) return;
  busy = true; render();
  try {
    const data = await api('/api/roblox/inventory/refresh', { account_id: accId });
    robloxInventories.set(accId, data);
    toast('Инвентарь Roblox обновлён');
  } catch(e) {
    toast('Ошибка: ' + e.message);
  } finally {
    busy = false; render();
  }
}

async function renderRobloxInventoryTab(robloxList, root, launcherBarHtml, generation) {
  if (!robloxList.length && !demo) {
    root.innerHTML = `${launcherBarHtml}${empty('Подключи Roblox', 'Добавь аккаунт Roblox через авто-поиск сессий или вручную.', 'connect', 'Подключить аккаунт')}`;
    return;
  }
  if (!currentRobloxInventoryAccount || !robloxList.some(a => a.id === currentRobloxInventoryAccount)) {
    const active = robloxList.find(a => a.is_active);
    currentRobloxInventoryAccount = active?.id || robloxList[0]?.id || (demo ? 'demo-roblox' : '');
  }
  const acc = robloxList.find(a => a.id === currentRobloxInventoryAccount) || robloxList[0];
  const robloxAccountSelector = robloxList.length > 1 ? `
    <div style="margin-bottom:16px;display:flex;align-items:center;gap:10px">
      <label style="font-size:12px;font-weight:600">Аккаунт Roblox:
        <select id="roblox-inventory-account-select" style="margin-left:8px;padding:6px 12px;border:1px solid var(--line);border-radius:6px;background:var(--bg);color:var(--text)">
          ${robloxList.map(a => `<option value="${esc(a.id)}" ${a.id === currentRobloxInventoryAccount ? 'selected' : ''}>${esc(a.display_name || a.username)} (@${esc(a.username)})</option>`).join('')}
        </select>
      </label>
    </div>
  ` : '';

  if (demo) {
    const demoInv = {
      total_rap: 28500,
      collectibles: [
        { asset_id: 1, name: 'Valkyrie Helm', recent_average_price: 18500, thumbnail_url: 'https://images.rbxcdn.com/e2e50cf605d15c83258c8ec6d81e05d0.png', serial_number: 412 },
        { asset_id: 2, name: 'Super Happy Face', recent_average_price: 10000, thumbnail_url: 'https://images.rbxcdn.com/e2e50cf605d15c83258c8ec6d81e05d0.png', serial_number: null }
      ],
      wearing_asset_ids: [1, 2, 12345]
    };
    root.innerHTML = `${launcherBarHtml}${robloxAccountSelector}${renderRobloxInventoryBodyHtml(acc, demoInv)}`;
    hydrateIcons(root);
    return;
  }

  const cached = robloxInventories.get(acc.id);
  if (cached) {
    root.innerHTML = `${launcherBarHtml}${robloxAccountSelector}${renderRobloxInventoryBodyHtml(acc, cached)}`;
    hydrateIcons(root);
    return;
  }

  root.innerHTML = `
    ${launcherBarHtml}
    ${robloxAccountSelector}
    <div style="padding:24px;text-align:center;">
      <p style="font-size:16px;font-weight:600;margin-bottom:8px">Синхронизация инвентаря Roblox @${esc(acc.username)}…</p>
      <p class="muted">Запрашиваем баланс, надетые вещи и коллекционные Limiteds с оценкой RAP</p>
    </div>
  `;
  try {
    const data = await api(`/api/roblox/inventory?account_id=${encodeURIComponent(acc.id)}`);
    if(generation!==inventoryRenderGeneration||page!=='inventory')return;
    robloxInventories.set(acc.id, data);
    root.innerHTML = `${launcherBarHtml}${robloxAccountSelector}${renderRobloxInventoryBodyHtml(acc, data)}`;
    hydrateIcons(root);
  } catch(e) {
    if(generation!==inventoryRenderGeneration||page!=='inventory')return;
    root.innerHTML = `${launcherBarHtml}${robloxAccountSelector}<p class="error-text">Ошибка загрузки: ${esc(e.message)}</p>`;
  }
}

function renderRobloxInventoryBodyHtml(acc, data) {
  const robux = acc.robux_balance != null ? new Intl.NumberFormat(getLocale()).format(acc.robux_balance) : '—';
  const totalRap = data.total_rap != null ? new Intl.NumberFormat(getLocale()).format(data.total_rap) : '0';
  const collectibles = data.collectibles || [];
  const wearing = data.wearing_asset_ids || [];

  return `
    <div style="display:flex;justify-content:space-between;align-items:center;padding:16px;background:var(--bg-elevated);border-radius:10px;margin-bottom:16px;border:1px solid var(--line);">
      <div style="display:flex;gap:16px;align-items:center;">
        ${accountAvatar(acc)}
        <div>
          <h3 style="margin:0 0 4px">${esc(acc.display_name || acc.username)} <span class="muted" style="font-size:13px;font-weight:normal;">(@${esc(acc.username)})</span></h3>
          <div style="display:flex;gap:8px;align-items:center;">
            <span class="roblox-robux-badge">🪙 ${robux} R$</span>
            ${acc.is_premium ? '<span class="roblox-premium-badge">★ Premium</span>' : ''}
            <span class="roblox-rap-badge">💎 Всего RAP: ${totalRap} R$</span>
          </div>
        </div>
      </div>
      <div style="display:flex;gap:8px;">
        <button class="secondary" data-action="roblox-tab-refresh" data-account="${esc(acc.id)}">${icon('refresh')}Обновить с сервера</button>
      </div>
    </div>

    <div style="margin-bottom:20px;">
      <h3 style="margin-bottom:10px;">Коллекционные предметы (Collectibles / Limiteds) · ${collectibles.length}</h3>
      <div class="roblox-inv-grid">
        ${collectibles.length ? collectibles.map(item => `
          <div class="roblox-item-card">
            ${item.thumbnail_url ? `<img src="${esc(item.thumbnail_url)}" alt="" loading="lazy">` : '<div style="width:80px;height:80px;background:var(--bg-card);border-radius:6px;"></div>'}
            <div class="roblox-item-name" title="${esc(item.name)}">${esc(item.name)}</div>
            <div class="roblox-item-rap">💎 RAP: ${new Intl.NumberFormat(getLocale()).format(item.recent_average_price || 0)} R$</div>
            ${item.serial_number ? `<small class="muted">#${item.serial_number}</small>` : ''}
          </div>
        `).join('') : '<p class="muted">Коллекционных предметов (Limiteds) в инвентаре не обнаружено.</p>'}
      </div>
    </div>

    <div>
      <h3 style="margin-bottom:10px;">Надетые вещи аватара (Asset IDs) · ${wearing.length}</h3>
      <div style="display:flex;flex-wrap:wrap;gap:8px;">
        ${wearing.length ? wearing.map(id => `
          <a href="https://www.roblox.com/catalog/${id}" target="_blank" rel="noopener noreferrer" class="badge" style="text-decoration:none;padding:6px 10px;background:var(--bg-elevated);border-radius:6px;font-size:12px;border:1px solid var(--line);">
            Предмет #${id} ↗
          </a>
        `).join('') : '<p class="muted">Надетые предметы не загружены.</p>'}
      </div>
    </div>
  `;
}
function quickBanAccounts(g) {
  const platformAccs = accounts.filter(a => a.launcher === g.launcher);
  const eligible = availableAccounts(g);
  const eligibleIds = new Set(eligible.map(a => a.id));
  const relevant = platformAccs.filter(a => eligibleIds.has(a.id) || accountHasGameBan(a, g.app_id, g.name, accountRestrictions));
  return relevant.length ? relevant : platformAccs;
}
function openQuickBanDialog(g) {
  const dialog = $('quick-ban-dialog');
  if (!dialog) return;
  const accList = quickBanAccounts(g);
  $('quick-ban-title').textContent = `Блокировка · ${g.name}`;
  $('quick-ban-subtitle').textContent = `Назначь игру в бан для аккаунта, чтобы скрыть её`;
  const body = $('quick-ban-body');
  if (body) {
    if (!accList.length) {
      body.innerHTML = '<p class="muted">Нет подключённых аккаунтов для этой игры.</p>';
    } else {
      body.innerHTML = `<div style="display:flex;flex-direction:column;gap:8px;margin-top:12px;width:100%">${accList.map(acc => {
        const isBanned = accountHasGameBan(acc, g.app_id, g.name, accountRestrictions);
        return `<div class="quick-ban-row">
          <div class="account-info">
            ${accountAvatar(acc)}
            <div>
              <strong>${esc(acc.display_name || acc.username)}</strong>
              <small class="muted" style="display:block;color:${isBanned ? '#ff8373' : 'var(--muted)'}">${isBanned ? '⚠️ В бане (скрыт из библиотеки)' : (acc.is_active ? 'Активен в Steam' : 'Доступен')}</small>
            </div>
          </div>
          <button class="secondary account-ban-btn ${isBanned ? 'is-banned' : ''}" data-action="quick-dialog-toggle-ban" data-account="${esc(acc.id)}" data-game="${esc(gameKey(g))}" ${isBanned&&isVerifiedGameBan(accountRestrictions.get(acc.id),{app_id:g.app_id,name:g.name})?'disabled':''}>
            ${isBanned ? (isVerifiedGameBan(accountRestrictions.get(acc.id),{app_id:g.app_id,name:g.name})?'Подтверждено Steam':'✓ Снять бан') : `${icon('ban')}В бан`}
          </button>
        </div>`;
      }).join('')}</div>`;
    }
  }
  dialog.showModal();
}
async function toggleAccountGameBan(accountId, appId, gameName) {
  const a = accounts.find(acc => acc.id === accountId);
  if (!a) return;
  const restr = accountRestrictions.get(accountId) || {
    account_id: accountId,
    launcher: a.launcher,
    community_banned: false,
    trade_banned: false,
    market_banned: false,
    vac_banned: false,
    vac_bans_count: 0,
    game_bans_count: 0,
    economy_ban: 'none',
    is_limited: false,
    game_bans: [],
    details: ''
  };
  const currentBans = Array.isArray(restr.game_bans) ? [...restr.game_bans] : [];
  const strAppId = appId != null ? String(appId) : '';
  const lowerName = (gameName || '').trim().toLowerCase();
  const existingIndex = currentBans.findIndex(b => {
    if (strAppId && b.app_id && String(b.app_id) === strAppId) return true;
    if (lowerName && b.name && b.name.trim().toLowerCase() === lowerName) return true;
    return false;
  });
  if(existingIndex>=0 && isVerifiedGameBan(restr,currentBans[existingIndex])) {
    toast('Подтверждённый Steam бан удалить нельзя.');
    return;
  }
  let willBan = false;
  if (existingIndex >= 0) {
    currentBans.splice(existingIndex, 1);
    willBan = false;
  } else {
    currentBans.push({
      app_id: strAppId || undefined,
      name: gameName || (strAppId ? `App #${strAppId}` : 'Игра'),
      ban_type: 'Game Ban',
      ban_date: new Date().toISOString().split('T')[0]
    });
    willBan = true;
  }
  const payload = {
    ...restr,
    account_id: accountId,
    launcher: a.launcher,
    game_bans: currentBans,
    game_bans_count: currentBans.length,
    currency: restr.currency || getAccountCurrency(a),
    country: restr.country || getAccountCountry(a),
    updated_at: new Date().toISOString()
  };
  const accName = a.display_name || a.username || 'Аккаунт';
  const gTitle = gameName || (strAppId ? `App #${strAppId}` : 'Игра');
  if (demo) {
    accountRestrictions.set(accountId, payload);
    render();
    toast(willBan ? `🚫 ${gTitle}: добавлен бан для ${accName}` : `✓ ${gTitle}: бан снят для ${accName}`);
    return;
  }
  try {
    const res = await api('/api/accounts/restrictions', payload);
    const saved = res?.account_id ? res : payload;
    accountRestrictions.set(saved.account_id || accountId, saved);
    render();
    toast(willBan ? `🚫 ${gTitle}: добавлен бан для ${accName}` : `✓ ${gTitle}: бан снят для ${accName}`);
  } catch (err) {
    toast(`Ошибка: ${err.message}`);
  }
}
function openRestrictionsDialog(accountId) {
  const a = accounts.find(acc => acc.id === accountId);
  if (!a) return;
  const restr = accountRestrictions.get(accountId) || {
    account_id: accountId,
    launcher: a.launcher,
    community_banned: false,
    trade_banned: false,
    market_banned: false,
    vac_banned: false,
    vac_bans_count: 0,
    game_bans_count: 0,
    economy_ban: 'none',
    is_limited: false,
    game_bans: [],
    details: ''
  };
  currentEditingRestrictions = { ...restr };
  currentEditingGameBans = Array.isArray(restr.game_bans) ? restr.game_bans.map(b => ({ ...b })) : [];
  $('restrictions-title').textContent = `Ограничения · ${a.display_name || a.username}`;
  $('restrictions-account-name').textContent = `Платформа: ${platformName(a.launcher)} (${a.id})`;
  $('restr-account-id').value = a.id;
  $('restr-launcher').value = a.launcher;
  $('restr-market').checked = Boolean(restr.market_banned);
  $('restr-market').disabled = Boolean(restr.verified?.market_banned);
  $('restr-trade').checked = Boolean(restr.trade_banned);
  $('restr-trade').disabled = Boolean(restr.verified?.trade_banned);
  $('restr-vac').checked = Boolean(restr.vac_banned);
  $('restr-vac').disabled = Boolean(restr.verified?.vac_bans_count);
  $('restr-community').checked = Boolean(restr.community_banned);
  $('restr-community').disabled = Boolean(restr.verified?.community_banned);
  if ($('restr-currency')) $('restr-currency').value = getAccountCurrency(a);
  $('restrictions-error').textContent = '';

  const select = $('restr-add-game-select');
  if (select) {
    const seenApps = new Set();
    const available = [];
    for (const pg of POPULAR_STEAM_GAMES) {
      if (!seenApps.has(pg.app_id)) {
        seenApps.add(pg.app_id);
        available.push(pg);
      }
    }
    for (const g of games) {
      if (g.launcher === a.launcher && availableAccounts(g).some(owner=>owner.id===a.id)
          && g.app_id && !seenApps.has(String(g.app_id)) && !isIgnoredSteamApp(g)) {
        seenApps.add(String(g.app_id));
        available.push({ app_id: String(g.app_id), name: g.name });
      }
    }
    available.sort((left,right)=>left.name.localeCompare(right.name,getLocale()));
    select.innerHTML = '<option value="">Выбери игру из библиотеки аккаунта...</option>' + available.map(g => `<option value="${esc(g.app_id)}" data-name="${esc(g.name)}">${esc(g.name)} (${esc(g.app_id)})</option>`).join('');
    $('restr-account-games').innerHTML = available.map(g=>`<option value="${esc(g.name)}" label="AppID ${esc(g.app_id)}"></option>`).join('');
    $('restr-custom-name').value='';
    $('restr-custom-appid').value='';
    delete $('restr-custom-appid').dataset.suggestedId;
  }

  renderEditingGameBans();
  $('restrictions-dialog').showModal();
}
function renderEditingGameBans() {
  const container = $('restr-game-bans-list');
  if (!container) return;
  if (!currentEditingGameBans.length) {
    container.innerHTML = '<p class="muted small-text" style="padding:6px 0">Нет отмеченных игровых блокировок.</p>';
    return;
  }
  container.innerHTML = currentEditingGameBans.map((ban, idx) => {
    const verified=isVerifiedGameBan(currentEditingRestrictions,ban);
    return `
    <div class="game-ban-item">
      <span>
        <strong>⚠️ ${esc(ban.name || ('App #' + ban.app_id))}</strong>
        <small class="muted">${ban.app_id ? `ID: ${esc(ban.app_id)} · ` : ''}${verified?'Подтверждено Steam · изменить нельзя':esc(ban.ban_type || 'Добавлено вручную')}</small>
      </span>
      ${verified?'':`<button type="button" class="text-button" data-remove-ban="${idx}" style="color:#ff8373;padding:2px 6px">Удалить</button>`}
    </div>
  `;}).join('');
}
function isVerifiedGameBan(restr, ban) {
 return Boolean(restr?.verified?.game_bans?.some(item =>
   (item.app_id && ban.app_id && String(item.app_id)===String(ban.app_id)) ||
   (!item.app_id && !ban.app_id && item.name===ban.name)));
}
async function checkAccountRestrictions(accountId, apiKey) {
  if (busy) return;
  busy = true;
  render();
  toast('Проверяем статус ограничений в Steam…');
  try {
    const res = await api('/api/steam/check-restrictions', apiKey ? { account_id: accountId, api_key: apiKey } : { account_id: accountId });
    const list = Array.isArray(res?.restrictions) ? res.restrictions : (res?.restrictions ? [res.restrictions] : (res?.account_id ? [res] : []));
    for (const item of list) {
      if (item?.account_id) {
        accountRestrictions.set(item.account_id, item);
      }
    }
    const r = list.find(x => x?.account_id === accountId) || list[0];
    if (r) {
      const parts = [];
      if (r.vac_banned) parts.push(`VAC-банов: ${r.vac_bans_count || 1}`);
      if (r.game_bans_count > 0) parts.push(`Игровых банов: ${r.game_bans_count}`);
      if (r.economy_ban && r.economy_ban !== 'none') parts.push(`Трейд: ${r.economy_ban}`);
      if (r.community_banned) parts.push('Бан в сообществе');
      if (r.is_limited) parts.push('Ограниченный ($5)');
      if (r.game_bans && r.game_bans.length > 0) parts.push(`Баны в играх: ${r.game_bans.map(b => b.name).join(', ')}`);
      const checked = Array.isArray(res?.checked_account_ids) && res.checked_account_ids.includes(r.account_id);
      if(!checked) {
        toast(apiKey?'Не удалось проверить бан с этим ключом Steam. Сохранённые сведения не изменены.':'Не удалось получить новый статус Steam. Сохранённые сведения оставлены без изменений.');
      } else if(apiKey&&(r.vac_banned||r.game_bans_count>0)&&!r.game_bans?.length) {
        toast('Steam подтвердил бан, но не передал точный AppID игры. Проверь её в Steam Support.');
      } else {
        toast(parts.length ? `Обнаружены ограничения: ${parts.join(', ')}` : 'Публичных блокировок не обнаружено.');
      }
    } else {
      toast('Steam не передал сведения о блокировках. Повтори проверку позже.');
    }
  } catch (err) {
    toast('Ошибка проверки: ' + err.message);
  } finally {
    busy = false;
    render();
  }
}
async function refreshSteamRestrictions() {
 if(demo || restrictionsRefreshPending || !accounts.some(a => a.launcher === 'steam')) return;
 restrictionsRefreshPending = true;
 try {
   const res = await api('/api/steam/check-restrictions', {account_ids:accounts.filter(a=>a.launcher==='steam').map(a=>a.id)});
   for(const item of res?.restrictions || []) {
     if(item?.account_id) accountRestrictions.set(item.account_id, item);
   }
   render();
 } catch { /* Keep the last confirmed account status when Steam is unavailable. */ }
 finally { restrictionsRefreshPending = false; }
}
$('restr-add-game-btn')?.addEventListener('click', () => {
  const sel = $('restr-add-game-select');
  const appId = sel.value;
  if (!appId) return;
  const opt = sel.options[sel.selectedIndex];
  const name = opt?.dataset.name || opt?.text || `App #${appId}`;
  if (!currentEditingGameBans.some(b => String(b.app_id) === String(appId))) {
    currentEditingGameBans.push({ app_id: String(appId), name, ban_type: 'Game Ban' });
    renderEditingGameBans();
  }
  sel.value = '';
});
$('restr-add-custom-btn')?.addEventListener('click', () => {
  const nameInput = $('restr-custom-name');
  const appInput = $('restr-custom-appid');
  const name = nameInput.value.trim();
  const appId = appInput.value.trim();
  if (!name && !appId) return;
  currentEditingGameBans.push({
    app_id: appId || null,
    name: name || `Игра #${appId}`,
    ban_type: 'Game Ban'
  });
  nameInput.value = '';
  appInput.value = '';
  renderEditingGameBans();
});
$('restr-custom-name')?.addEventListener('input',()=>{
  const name=$('restr-custom-name').value.trim().toLocaleLowerCase(getLocale());
  const matches=[...$('restr-add-game-select').options].filter(option=>option.value&&option.dataset.name?.toLocaleLowerCase(getLocale())===name);
  const input=$('restr-custom-appid');
  if(matches.length===1){input.value=matches[0].value;input.dataset.suggestedId=matches[0].value;}
  else if(input.value===input.dataset.suggestedId){input.value='';delete input.dataset.suggestedId;}
});
$('restrictions-form')?.addEventListener('submit', async e => {
  e.preventDefault();
  const accountId = $('restr-account-id').value;
  const launcher = $('restr-launcher').value || 'steam';
  const marketBanned = $('restr-market').checked;
  const tradeBanned = $('restr-trade').checked;
  const vacBanned = $('restr-vac').checked;
  const communityBanned = $('restr-community').checked;
  const selectedCurrency = $('restr-currency')?.value || 'USD';
  const selectedCountry = currencyToCountry(selectedCurrency);

  const payload = {
    account_id: accountId,
    launcher,
    community_banned: communityBanned,
    trade_banned: tradeBanned,
    market_banned: marketBanned,
    vac_banned: vacBanned,
    vac_bans_count: vacBanned ? Math.max(1, currentEditingRestrictions?.vac_bans_count || 1) : 0,
    game_bans_count: currentEditingGameBans.length,
    economy_ban: tradeBanned ? 'banned' : (currentEditingRestrictions?.economy_ban || 'none'),
    is_limited: Boolean(currentEditingRestrictions?.is_limited),
    game_bans: currentEditingGameBans,
    currency: selectedCurrency,
    country: selectedCountry,
    details: currentEditingRestrictions?.details || '',
    updated_at: new Date().toISOString()
  };

  if (demo) {
    accountRestrictions.set(accountId, payload);
    $('restrictions-dialog').close();
    render();
    toast('Ограничения сохранены (демо)');
    return;
  }

  const submitBtn = e.target.querySelector('button[type="submit"]');
  if (submitBtn) submitBtn.disabled = true;
  $('restrictions-error').textContent = '';
  try {
    const res = await api('/api/accounts/restrictions', payload);
    const saved = res?.account_id ? res : payload;
    accountRestrictions.set(saved.account_id || accountId, saved);
    $('restrictions-dialog').close();
    render();
    toast('Ограничения аккаунта сохранены');
  } catch (err) {
    $('restrictions-error').textContent = err.message;
  } finally {
    if (submitBtn) submitBtn.disabled = false;
  }
});
$('launch-warning-confirm')?.addEventListener('click', () => {
  $('launch-warning-dialog').close();
  if (launchWarningResolver) {
    launchWarningResolver(true);
    launchWarningResolver = null;
  }
});
$('launch-warning-dialog')?.addEventListener('close', () => {
  if (launchWarningResolver) {
    launchWarningResolver(false);
    launchWarningResolver = null;
  }
});
$('search-input').addEventListener('input',e=>{query=e.target.value;renderLibrary();hydrateIcons();});
$('sort-select').addEventListener('change',e=>{sort=e.target.value;renderLibrary();hydrateIcons();});
$('metrics-form').addEventListener('submit',e=>{e.preventDefault();if(!metricTarget)return;try{const value=normalizeMetrics(Object.fromEntries(new FormData(e.target)));const next={...metrics,[metricKey(metricTarget.g,metricTarget.a)]:value};if(!save(`${namespace}-metrics`,next))return;metrics=next;$('metrics-dialog').close();render();toast('Статистика сохранена на этом устройстве');}catch(err){toast(err.message);}});
$('login-form').addEventListener('submit',async e=>{
 e.preventDefault();const button=e.submitter;button.disabled=true;$('login-error').textContent='';
 try{
  if (!await cancelEpicBrowserLogin()) return;
  button.disabled=true;
  const loginRes=await api('/api/epic/auth/login',{code:$('oauth-code-input').value.trim()});
  $('oauth-code-input').value='';$('connect-dialog').close();
  toast(`Аккаунт ${loginRes.account?.display_name || 'Epic Games'} подключён!`);
  await refresh();
 }catch(err){$('login-error').textContent=err.message;}finally{button.disabled=false;}});
$('connect-dialog').addEventListener('close', () => {
  if (epicBrowserSession && !epicBrowserSession.connected) cancelEpicBrowserLogin();
});
$('gog-login-form')?.addEventListener('submit',async e=>{
 e.preventDefault();if(demo){toast('В демо вход отключён.');return;}if(busy)return;
 const button=e.submitter||e.target.querySelector('button[type="submit"]');button.disabled=true;busy=true;$('gog-login-error').textContent='';
 try {
  const result=await api('/api/gog/auth/login',{code:$('gog-code').value.trim()});
  $('gog-code').value='';$('connect-dialog').close();
  toast(result.warnings?.length?result.warnings.join(' '):result.message);
 }catch(e){$('gog-login-error').textContent=e.message;}finally{busy=false;button.disabled=false;await refresh();}
});
$('gog-account-filter')?.addEventListener('change',e=>{gogAccountFilter=e.target.value;selectedGame=null;render();});
$('roblox-place-form')?.addEventListener('submit', async e => {
  e.preventDefault();
  const input = $('roblox-place-input');
  const errEl = $('roblox-place-error');
  errEl.textContent = '';
  const raw = input.value.trim();
  const match = raw.match(/\d+/);
  const placeId = match ? match[0] : raw;
  if (!placeId) { errEl.textContent = 'Укажи корректный Place ID'; return; }
  try {
    const res = await api('/api/roblox/places/add', { place_id: placeId });
    $('roblox-place-dialog').close();
    toast(`Плейс «${res.place?.name || placeId}» добавлен в библиотеку!`);
    await refresh();
  } catch(err) {
    errEl.textContent = err.message;
  }
});
$('roblox-manual-form')?.addEventListener('submit', async e => {
  e.preventDefault();
  const input = $('roblox-cookie-input');
  const errEl = $('roblox-manual-error');
  errEl.textContent = '';
  const cookie = input.value.trim();
  if (!cookie) { errEl.textContent = 'Вставь cookie .ROBLOSECURITY'; return; }
  try {
    const res = await api('/api/roblox/accounts/add', { cookie });
    input.value = '';
    $('connect-dialog').close();
    toast(`Аккаунт @${res.username} (${res.display_name}) успешно добавлен!`);
    await refresh();
  } catch(err) {
    errEl.textContent = err.message;
  }
});
$('roblox-multi-instance-toggle')?.addEventListener('change', async e => {
  const enabled = e.target.checked;
  try {
    await api('/api/roblox/settings', { key: 'multi_instance', value: String(enabled) });
    toast(enabled ? 'Multi-Roblox включен: несколько окон одновременно' : 'Multi-Roblox выключен: один клиент за раз');
  } catch(err) {
    toast('Не удалось сохранить настройку: ' + err.message);
  }
});
$('ubisoft-session-form')?.addEventListener('submit', async e => {
 e.preventDefault();if(busy)return;
 if(demo){toast('Демо: сессии Ubisoft не сохраняются.');return;}
 const error=$('ubisoft-session-error');error.textContent='';busy=true;render();
 const button=e.currentTarget.querySelector('button[type="submit"]');button.disabled=true;
 try{
  const input=$('ubisoft-account-name');
  const customName=input.value===input.dataset.autoName?null:input.value.trim()||null;
  const result=await api('/api/ubisoft/sessions/save',{display_name:customName});
  toast(result.message);input.value='';delete input.dataset.autoName;
 }catch(e){error.textContent=e.message;}finally{busy=false;button.disabled=false;await refresh();}
});
$('minecraft-launch-account-select')?.addEventListener('change', e => {
  const selAcc = accounts.find(a => a.launcher === 'minecraft' && a.id === e.target.value);
  const launcherSelect = $('minecraft-launch-launcher-select');
  if (launcherSelect && selAcc) {
    const hasOfficial = minecraftStatus ? minecraftStatus.official_installed : true;
    const hasTl = minecraftStatus ? minecraftStatus.tlauncher_installed : true;
    const isTl = (selAcc.account_type || '').startsWith('tlauncher') || selAcc.account_type === 'offline';
    if (isTl && hasTl) {
      launcherSelect.value = 'tlauncher';
    } else if (!isTl && hasOfficial) {
      launcherSelect.value = 'official';
    }
  }
});
$('minecraft-launch-form')?.addEventListener('submit', async e => {
  e.preventDefault();
  const launcherClient = $('minecraft-launch-launcher-select')?.value || null;
  const accountId = $('minecraft-launch-account-select')?.value || null;
  const profileId = $('minecraft-launch-profile-select')?.value || null;
  const mode = $('minecraft-launch-mode-select')?.value || 'official';
  const serverIp = $('minecraft-launch-server-ip')?.value || null;
  const dialog = $('minecraft-launch-dialog');
  if (dialog) dialog.close();
  await launchMinecraft({
    accountId,
    profileId,
    mode,
    serverIp,
    launcherClient: launcherClient === 'auto' ? null : launcherClient
  });
});
$('minecraft-account-form')?.addEventListener('submit', async e => {
  e.preventDefault();
  const username = $('minecraft-account-username-input')?.value?.trim();
  const account_type = $('minecraft-account-type-select')?.value || 'tlauncher_free';
  const errEl = $('minecraft-account-error');
  if (errEl) errEl.textContent = '';
  if (!username) return;

  try {
    if (demo) {
      const newAcc = {
        id: 'demo-tl-' + Date.now(),
        account_id: 'demo-tl-' + Date.now(),
        username,
        display_name: username,
        launcher: 'minecraft',
        is_active: true,
        account_type,
        avatar_url: `https://mc-heads.net/avatar/${encodeURIComponent(username)}/64`,
      };
      accounts.forEach(a => { if (a.launcher === 'minecraft') a.is_active = false; });
      accounts.push(newAcc);
      $('minecraft-account-dialog')?.close();
      toast(`Аккаунт ${username} добавлен и активирован (демо)`);
      render();
      return;
    }

    const res = await api('/api/minecraft/accounts/add', { username, account_type });
    $('minecraft-account-dialog')?.close();
    toast(`Аккаунт ${res.username || username} добавлен и активирован!`);
    await refresh();
  } catch (err) {
    if (errEl) errEl.textContent = err.message || String(err);
  }
});
$('minecraft-server-form')?.addEventListener('submit', async e => {
  e.preventDefault();
  const id = $('minecraft-server-id')?.value || undefined;
  const name = $('minecraft-server-name-input')?.value?.trim();
  const ip = $('minecraft-server-ip-input')?.value?.trim();
  const is_favorite = $('minecraft-server-fav-input')?.checked || false;
  if (!name || !ip) return;
  try {
    if (demo) {
      toast('Демо: сервер сохранён');
      $('minecraft-server-dialog')?.close();
      return;
    }
    const result = await api('/api/minecraft/servers/add', { id, name, ip, is_favorite });
    toast(result.message || 'Сервер сохранён');
    $('minecraft-server-dialog')?.close();
    await refresh();
  } catch (err) {
    const errEl = $('minecraft-server-error');
    if (errEl) errEl.textContent = err.message || String(err);
  }
});
document.addEventListener('change', e => {
  if (e.target.id === 'roblox-inventory-account-select') {
    currentRobloxInventoryAccount = e.target.value;
    save('swch-roblox-inventory-account', currentRobloxInventoryAccount);
    renderInventory();
  }
});
document.addEventListener('keydown',e=>{if(e.key==='/'&&!document.querySelector('dialog[open]')&&!['INPUT','TEXTAREA','SELECT'].includes(document.activeElement.tagName)){e.preventDefault();const input=page==='accounts'?$('account-search-input'):page==='inventory'?($('cs-inventory-search')||$('inventory-search')||$('riot-inventory-search')||$('inventory-fortnite-search')):$('search-input');input?.focus();}});
for(const d of document.querySelectorAll('dialog'))d.addEventListener('click',e=>{if(e.target===d){const rect=d.getBoundingClientRect();if(e.clientX<rect.left||e.clientX>rect.right||e.clientY<rect.top||e.clientY>rect.bottom)d.close();}});
const inventoryPages = new Map();
const inventoryPending = new Set();
const inventoryErrors = new Map();
let inventoryAccount = 'all', inventoryApp = 'all', inventoryContext = 'all', inventorySearch = '', inventoryTag = '', inventoryAvailability = 'all', heroSearchQuery = '';
const commonInventories = [['730','Counter-Strike 2'],['570','Dota 2'],['440','Team Fortress 2'],['252490','Rust'],['753','Steam: карточки и предметы']];
const accessLabel = access => ({owned:'Своя игра',family:'Семейный доступ',local:'Локальная история · владение не подтверждено',history:'Ранее запускалась · доступ не подтверждён'}[access] || 'Нет данных об игре');
const gameDetailsPending=new Set();
function applySteam(data) {
 steamRows=(data.games || []).filter(g=>!isIgnoredSteamApp(g));
 steamLibrarySources=data.synced_sources||{};
 games=[...normalizeSteam(steamRows),...games.filter(g=>g.launcher!=='steam'&&!isIgnoredSteamApp(g))];
 accounts=[...(data.accounts || []),...accounts.filter(a=>a.launcher!=='steam')];
 void hydrateMissingSteamNames();
}
function renderSteamFilters() {
 $('steam-filters').hidden=demo || !['library','favorites'].includes(page) || !['all','steam'].includes(platform);
 const list=accounts.filter(a=>a.launcher==='steam');
 if(steamAccountFilter!=='all'&&!list.some(a=>a.id===steamAccountFilter))steamAccountFilter='all';
 $('steam-account-filter').innerHTML='<option value="all">Все аккаунты</option>'+list.map(a=>`<option value="${esc(a.id)}" translate="no">${esc(a.display_name||a.username)}</option>`).join('');
 $('steam-account-filter').value=steamAccountFilter;$('steam-access-filter').value=steamAccessFilter;
}
function renderSteamConnect() {
 const list=accounts.filter(a=>a.launcher==='steam');
 $('steam-sync-account').innerHTML=list.length?list.map(a=>`<option value="${esc(a.id)}" ${a.is_active?'selected':''}>${esc(a.display_name||a.username)}</option>`).join(''):'<option value="">Сначала войди в Steam и обнови библиотеку</option>';
 $('steam-api-key').value='';$('steam-access-token').value='';$('steam-sync-error').textContent='';
 $('steam-status-dot').className='status-indicator-dot '+(list.length?'online':'');
 $('steam-status-desc').textContent=list.length?`Подключено аккаунтов: ${list.length}. Можно добавить ещё один или обновить игры.`:'Пока нет подключённых аккаунтов. Начни со входа в Steam.';
 $('steam-sync-account').disabled=!list.length;
 $('steam-sync-form').querySelector('button[type="submit"]').disabled=!list.length;
}
function accountAvatar(a) {
 const src=a.launcher==='steam'&&!demo?`/api/steam/avatar?account_id=${encodeURIComponent(a.id)}`:safeImage(a.headshot_url || a.avatar_url || a.avatar);
 return `<span class="avatar">${esc((a.display_name||a.username||'SW').slice(0,2).toUpperCase())}${src?`<img src="${esc(src)}" alt="" loading="lazy" data-avatar>`:''}</span>`;
}
document.addEventListener('error',e=>{if(e.target.matches?.('img[data-avatar]'))e.target.remove();},true);
async function ensureGameDetails(g) {
 if(isIgnoredSteamApp(g)||gameDetailsPending.has(g.app_id))return;
 gameDetailsPending.add(g.app_id);
 try {
  const data=await api(`/api/steam/game-details?app_id=${encodeURIComponent(g.app_id)}`);
  if(isIgnoredSteamApp({app_id:g.app_id,name:data.name})){
   games=games.filter(x=>x.app_id!==g.app_id);
   steamRows=steamRows.filter(x=>x.app_id!==g.app_id);
   if(page==='library'||page==='favorites')renderLibrary();
   return;
  }
  if(data.name && typeof data.name==='string' && !data.name.startsWith('Steam #')){
   g.name=data.name;for(const r of steamRows.filter(r=>r.app_id===g.app_id))r.name=data.name;
  }
  for(const field of ['is_free','has_inventory'])if(typeof data[field]==='boolean'){g[field]=data[field];for(const r of steamRows.filter(r=>r.app_id===g.app_id))r[field]=data[field];}
  if(page==='library'||page==='favorites')renderLibrary();
 } catch {} // Keep unknown ownership conservative when Steam is unavailable.
}
let missingSteamNamesJob = null;
async function hydrateMissingSteamNames() {
 if(demo||missingSteamNamesJob)return;
 const missing=games.filter(g=>g.launcher==='steam'&&typeof g.name==='string'&&g.name.startsWith('Steam #')&&!isIgnoredSteamApp(g));
 if(!missing.length)return;
 const job={active:true,cancelled:false};missingSteamNamesJob=job;
 try {
  for(const g of missing) {
   if(job.cancelled)break;
   if(!g.name.startsWith('Steam #')||isIgnoredSteamApp(g))continue;
   try {
    const data=await api(`/api/steam/game-details?app_id=${encodeURIComponent(g.app_id)}`);
    if(isIgnoredSteamApp({app_id:g.app_id,name:data.name})){
     games=games.filter(x=>x.app_id!==g.app_id);
     steamRows=steamRows.filter(x=>x.app_id!==g.app_id);
     if(page==='library'||page==='favorites')renderLibrary();
     continue;
    }
    let changed=false;
    if(data.name && typeof data.name==='string' && !data.name.startsWith('Steam #')){
     g.name=data.name;for(const r of steamRows.filter(r=>r.app_id===g.app_id))r.name=data.name;
     changed=true;
    }
    for(const field of ['is_free','has_inventory'])if(typeof data[field]==='boolean'){
     g[field]=data[field];for(const r of steamRows.filter(r=>r.app_id===g.app_id))r[field]=data[field];
     changed=true;
    }
    if(changed&&(page==='library'||page==='favorites'))renderLibrary();
   } catch {
    await new Promise(r=>setTimeout(r,1500));
   }
   await new Promise(r=>setTimeout(r,350));
  }
 } finally {
  missingSteamNamesJob=null;
 }
}
function hasInventory(g) {
 if(g.has_inventory!=null)return g.has_inventory;
 return commonInventories.some(([id])=>String(g.app_id)===id) || [...inventoryPages.entries()].some(([key,p])=>JSON.parse(key)[1]===String(g.app_id)&&p.total_count>0);
}
const gameStatsCache = new Map(), gameStatsPending = new Set();
const deadlockHeroSelection = new Map();
const gameStatsKey = (app, account) => JSON.stringify([String(app), account]);
function deadlockStatsControls(data,a) {
 const detail=data?.deadlock,overview=deadlockOverview(detail),requirements=detail?.requirements;
 const selected=deadlockHeroSelection.get(gameStatsKey('1422450',a.id))||'all';
 const hero=detail?.heroes?.find(h=>String(h.hero_id)===selected);
 const list=[...DEADLOCK_HEROES,...(detail?.heroes||[]).filter(h=>!DEADLOCK_HEROES.some(c=>c.id===h.hero_id)).map(h=>({id:h.hero_id,name:deadlockHeroName(h.hero_id)}))].sort((a,b)=>a.name.localeCompare(b.name));
 const candidates=overview.eligible.filter(h=>!overview.played.some(p=>p.hero_id===h.hero_id));
 return `<div class="deadlock-ranked-summary"><h4>Доступ к рейтингу</h4><strong data-deadlock-status>${esc(overview.status)}</strong>${requirements?`<p class="muted">${esc(requirements.season)} · Победы Standard: ${detail.standard_wins==null?'Нет данных':format.format(detail.standard_wins)} / ${requirements.total_wins}. Персонажи с ${requirements.hero_wins}+ победами: ${overview.eligible.length} / ${requirements.heroes}.</p>`:'<p class="muted">Требования текущего сезона пока не получены.</p>'}${overview.played.length?`<p>Открыт по рейтинговым матчам: <strong>${esc(overview.played.map(h=>deadlockHeroName(h.hero_id)).join(', '))}</strong>.</p>`:''}${candidates.length?`<p>Требование по победам выполнено: <strong>${esc(candidates.map(h=>deadlockHeroName(h.hero_id)).join(', '))}</strong>.</p>`:''}<p class="muted small-text">История источника может быть неполной. Недостающие победы не подтверждают закрытый рейтинг. Точный допуск проверяет игра.</p></div>
 <label class="deadlock-hero-select">Персонаж<select data-deadlock-hero-select data-account="${esc(a.id)}"><option value="all" ${selected==='all'?'selected':''}>Все персонажи · общий обзор</option>${list.map(c=>{const stats=detail?.heroes?.find(h=>h.hero_id===c.id);return `<option value="${c.id}" ${selected===String(c.id)?'selected':''}>${esc(c.name)}${stats?.ranked_matches>0?' · рейтинг открыт':requirements&&stats?.standard_wins!=null?' · '+stats.standard_wins+'/'+requirements.hero_wins+' побед Standard':''}</option>`;}).join('')}</select></label>
 ${selected!=='all'?`<div class="deadlock-selected-hero">${safeImage(list.find(h=>String(h.id)===selected)?.image)?`<img src="${esc(safeImage(list.find(h=>String(h.id)===selected).image))}" alt="" loading="lazy">`:''}<div><h4>Статистика · ${esc(deadlockHeroName(selected))}</h4><p data-deadlock-hero-access>${esc(deadlockHeroAccess(hero,requirements))}${requirements&&hero?.standard_wins!=null?' · '+hero.standard_wins+'/'+requirements.hero_wins+' побед Standard':''}</p><small class="muted">Standard и Ranked · ${hero?.ranked_matches==null?'Нет данных о рейтинговых матчах':hero.ranked_matches+' рейтинговых матчей в истории текущего сезона'}${hero?.last_rank?' · Последний ранг: '+esc(deadlockRankLabel(hero.last_rank)):''}</small></div></div>${!hero?.metrics?.some(m=>m.value!=null)?'<p class="muted">Нет статистики этого персонажа в доступной истории.</p>':''}`:''}`;
}
function gameStatsBody(g, a) {
 const key=gameStatsKey(g.app_id,a.id), data=gameStatsCache.get(key), pending=gameStatsPending.has(key);
 const labels={available:'Загружено',partial:'Частичные данные',unavailable:'Нет данных',needs_key:'Нужен ключ Steam'};
 const selected=String(g.app_id)==='1422450'?(deadlockHeroSelection.get(key)||'all'):'all';
 const metrics=selected==='all'?(data?.metrics||[]):(data?.deadlock?.heroes?.find(h=>String(h.hero_id)===selected)?.metrics||[]);
 const restriction=accountRestrictions.get(a.id);
 const gameBanned=restriction&&accountHasGameBan(a,g.app_id,g.name,accountRestrictions);
 const banWarning=gameBanned?`<p class="error-text">⚠️ Блокировка в ${esc(g.name)} на выбранном аккаунте.</p>`:restriction&&(restriction.vac_banned||restriction.game_bans_count>0)?`<p class="error-text">На аккаунте есть блокировка. Игра указана не для каждого бана; проверь список в разделе аккаунта.</p>`:'';
 return `<div class="account-heading"><div><h3>Статистика ${esc(g.name)}</h3><small>${esc(a.display_name||a.username||a.id)} · ${pending?'Загрузка…':data?.stale?'Сохранённые данные':esc(labels[data?.status]||'Нет данных')}</small></div><button class="text-button" data-action="game-stats-refresh" data-app="${esc(g.app_id)}" data-account="${esc(a.id)}" ${pending?'disabled':''}>Обновить ↻</button></div>
 ${banWarning}${String(g.app_id)==='1422450'?deadlockStatsControls(data,a):''}${metrics.length?`<div class="game-stat-grid">${metrics.map(m=>`<div class="metric"><span class="metric-label">${esc(String(g.app_id)==='730'&&m.label==='MVP'?'MVP за раунды':m.label)}</span><strong>${m.value==null?'—':typeof m.value==='number'?esc(format.format(m.value)):esc(m.value)}${m.value!=null&&m.unit?' '+esc(m.unit):''}</strong></div>`).join('')}</div>`:''}
 <div class="game-stat-notes" role="status">${String(g.app_id)==='730'&&metrics.some(m=>m.label==='MVP')?'<p>MVP — число наград за лучший результат в раунде. За один матч можно получить несколько; счётчик Steam может включать CS:GO.</p>':''}${pending&&!data?'<p>Загружаем статистику выбранного аккаунта…</p>':''}${(data?.notes||[]).map(n=>`<p>${esc(n)}</p>`).join('')}</div>
 ${data?.source?`<p class="muted small-text">Источник: ${esc(data.source)}${data.fetched_at?' · Получено: '+esc(dateLabel(data.fetched_at)):''}${data.cached?' · Из кэша':''}</p>`:''}
 ${String(g.app_id)==='730'&&!pending?`<details ${!data||['needs_key','unavailable'].includes(data.status)?'open':''}><summary>Загрузить с ключом Steam Web API</summary><button class="secondary game-stats-api-link" data-action="steam-api-page" data-account="${esc(a.id)}">Открыть ключ в Steam · ${esc(a.display_name||a.username||a.id)}</button><p class="muted small-text">Steam переключится на выбранный аккаунт и откроет страницу ключа в своём браузере. Скопируй ключ в поле ниже.</p><div class="game-stats-key"><label>Ключ Steam Web API<input type="password" autocomplete="off" placeholder="Используется один раз" data-game-stats-key></label><button class="secondary" data-action="game-stats-key" data-app="730" data-account="${esc(a.id)}">Загрузить статистику</button></div><a class="game-stats-api-link" href="https://steamcommunity.com/dev/apikey" target="_blank" rel="noopener noreferrer">Открыть страницу в браузере</a></details>`:''}`;
}
function paintGameStats() {
 document.querySelectorAll('[data-game-stat-panel]').forEach(panel=>{
  const g=games.find(g=>g.launcher==='steam'&&String(g.app_id)===panel.dataset.app);
  const a=accounts.find(a=>a.id===panel.dataset.account);
  if(g&&a)panel.innerHTML=gameStatsBody(g,a);
 });
}
async function loadGameStats(app, account, refresh=false, apiKey) {
 if(demo)return;
 const key=gameStatsKey(app,account);
 if(gameStatsPending.has(key))return;
 gameStatsPending.add(key);paintGameStats();
 try {
  const data=await api(`/api/steam/game-stats?app_id=${encodeURIComponent(app)}&account_id=${encodeURIComponent(account)}&refresh=${refresh}`,apiKey?{api_key:apiKey}:undefined);
  if(data.account_id!==account||String(data.app_id)!==String(app)||!Array.isArray(data.metrics))throw new Error('Получен некорректный ответ статистики.');
  gameStatsCache.set(key,data);
  if(apiKey){
    try{
      const restrictions=await api('/api/accounts/restrictions');
      if(Array.isArray(restrictions.restrictions))accountRestrictions=new Map(restrictions.restrictions.map(r=>[r.account_id,r]));
    }catch{ /* Statistics remain usable if the restriction list refresh fails. */ }
  }
 } catch(e) {
  const old=gameStatsCache.get(key);
  gameStatsCache.set(key,old?.metrics?.some(m=>m.value!=null)?{...old,stale:true,notes:[...old.notes.filter(n=>!n.startsWith('Обновить не удалось.')),'Обновить не удалось. '+e.message]}:{status:'unavailable',metrics:[],notes:[e.message]});
 } finally {gameStatsPending.delete(key);paintGameStats();}
}
function renderGameStats(g,a) {
 if(demo||!a||!['730','570','1422450'].includes(String(g.app_id)))return '';
 const key=gameStatsKey(g.app_id,a.id);
 if(!gameStatsCache.has(key)&&!gameStatsPending.has(key))void loadGameStats(g.app_id,a.id);
 return `<section class="game-stat-panel" data-game-stat-panel data-app="${esc(g.app_id)}" data-account="${esc(a.id)}" aria-label="Статистика игры">${gameStatsBody(g,a)}</section>`;
}
function renderSteamAccounts(g) {
 const list=availableAccounts(g);
 const activeList=list.filter(acc=>!accountHasGameBan(acc, g.app_id, g.name, accountRestrictions));
 const bannedList=list.filter(acc=>accountHasGameBan(acc, g.app_id, g.name, accountRestrictions));
 const a=chosenAccount(g),row=steamStats(steamRows,g.app_id,a?.id);
 const total=aggregateSteamStats(steamRows,accounts.filter(a=>a.launcher==='steam').map(a=>a.id),g.app_id);
 const hours=n=>n==null?'—':format.format(n/60)+' ч';
 const isCurrentBanned = a && accountHasGameBan(a, g.app_id, g.name, accountRestrictions);
 const banAlert = isCurrentBanned ? `<div class="game-ban-alert"><strong>⚠️ Внимание: на выбранном аккаунте блокировка в ${esc(g.name)}!</strong><p>На профиле ${esc(a.display_name||a.username)} обнаружен бан в этой игре. При запуске не будет доступа к сетевой игре или серверам с защитой VAC.</p></div>` : '';
 const bannedDetails = bannedList.length ? `<details class="banned-accounts-toggle"><summary>Скрытые аккаунты с баном (${bannedList.length})</summary><div style="display:flex;flex-direction:column;gap:6px;margin-top:8px">${bannedList.map(acc=>`<div class="quick-ban-row"><div class="account-info">${accountAvatar(acc)}<div><strong>${esc(acc.display_name||acc.username)}</strong><small class="muted" style="display:block;color:#ff8373">⚠️ Заблокирован в этой игре</small></div></div>${isVerifiedGameBan(accountRestrictions.get(acc.id),{app_id:g.app_id,name:g.name})?'<span class="muted small-text">Подтверждено Steam</span>':`<button class="secondary account-ban-btn is-banned" data-action="toggle-game-ban" data-account="${esc(acc.id)}" data-game="${esc(gameKey(g))}">✓ Снять бан</button>`}</div>`).join('')}</div></details>` : '';
 return `<section class="account-section"><div class="account-heading"><div><h3>Общая статистика</h3><small>По всем подключённым аккаунтам Steam</small></div><button class="text-button" data-action="steam-connect">Обновить онлайн ↗</button></div><div class="aggregate-metrics"><div><small>Всего в игре</small><strong>${hours(total.minutes)}</strong></div><div><small>За две недели</small><strong>${hours(total.recent)}</strong></div>${hasInventory(g)?`<div><small>Инвентарь · все аккаунты</small><strong>${valueText(inventoryView('all',g.app_id,'all'))}</strong><small>Рыночная оценка загруженных предметов</small></div>`:''}<p>Часы известны для ${total.known_accounts} из ${total.total_accounts} аккаунтов. Неизвестные значения не считаются нулём.</p></div><div class="account-heading"><div><h3>По аккаунтам</h3><small>${g.is_free===true?'Бесплатная игра — доступны все аккаунты':'Показаны владельцы, семейный доступ и локальная история игры'}</small></div></div>${activeList.length?`<div class="account-selector">${activeList.map(acc=>{
   return `<button class="account-chip ${a?.id===acc.id?'selected':''}" data-action="select-account" data-account="${esc(acc.id)}" aria-pressed="${a?.id===acc.id}">${accountAvatar(acc)}<span>${esc(acc.display_name||acc.username)}<small>${hours(steamStats(steamRows,g.app_id,acc.id)?.playtime_minutes)}${acc.is_active?' · Выбран в Steam':''}</small><small>${esc(g.is_free&&!steamStats(steamRows,g.app_id,acc.id)?'Бесплатная игра':accessLabel(steamStats(steamRows,g.app_id,acc.id)?.access))}</small>${hasInventory(g)?`<small>Инвентарь: ${valueText(inventoryView(acc.id,g.app_id,'all'))}</small>`:''}</span></button>`;
 }).join('')}</div>`:(bannedList.length?'<p class="muted">Все аккаунты с доступом к игре скрыты из-за блокировок.</p>':'<p class="muted">Нет аккаунтов с подтверждённым доступом или локальной историей этой игры. Обнови онлайн-библиотеку, чтобы проверить владение.</p>')}${bannedDetails}${banAlert}${!list.length?'':`<div class="metrics"><div class="metric"><span class="metric-label">${icon('clock')}Время аккаунта</span><strong>${hours(row?.playtime_minutes)}</strong><small>${esc(a?.display_name||a?.username||'—')}</small></div><div class="metric"><span class="metric-label">За две недели</span><strong>${hours(row?.playtime_2weeks)}</strong><small>${row?.last_played?'Последний запуск: '+esc(dateLabel(row.last_played*1000)):'Последний запуск неизвестен'}</small></div><div class="metric"><span class="metric-label">Доступ</span><strong class="access-value">${g.is_free===true&&!row?'Бесплатная игра':esc(accessLabel(row?.access))}</strong><small>${row?.updated_at?'Данные: '+esc(dateLabel(row.updated_at)):'Нет данных о времени'}</small></div></div>`}${renderGameStats(g,a)}${hasInventory(g)&&a?`<div class="aggregate-metrics"><div><small>Инвентарь · ${esc(a.display_name||a.username)}</small><strong>${valueText(inventoryView(a.id,g.app_id,'all'))}</strong><small>Оценка доступных предметов этого аккаунта</small></div></div>`:''}${row?.access==='family'?`<p class="muted">Владельцы: ${(row.owner_steamids||[]).map(id=>esc(accounts.find(x=>x.id===id)?.display_name||id)).join(', ')}.</p>`:''}<div class="account-actions">${hasInventory(g)?`<button class="primary" data-action="inventory" data-account="${esc(a?.id||'all')}" data-app="${esc(g.app_id)}">${icon('diamond')}Предметы и скины</button><button class="secondary" data-action="inventory" data-account="all" data-app="${esc(g.app_id)}">Инвентари всех аккаунтов</button>`:''}${a&&!a.is_active?`<button class="secondary" data-action="switch" data-account="${esc(a.id)}" ${busy?'disabled':''}>Сделать активным</button>`:''}${a?(isCurrentBanned?(isVerifiedGameBan(accountRestrictions.get(a.id),{app_id:g.app_id,name:g.name})?'<span class="muted small-text">Бан подтверждён Steam</span>':`<button class="secondary btn-ban-action" data-action="toggle-game-ban" data-account="${esc(a.id)}" data-game="${esc(gameKey(g))}">✓ Снять бан с ${esc(a.display_name||a.username)}</button>`):`<button class="secondary btn-ban-action" data-action="toggle-game-ban" data-account="${esc(a.id)}" data-game="${esc(gameKey(g))}">${icon('ban')}Назначить в бан для ${esc(a.display_name||a.username)}</button>`):''}</div><p class="muted small-text">Выбор профиля показывает его статистику. При нажатии «Играть» аккаунт переключается автоматически. Чтобы скрыть профиль для этой игры, используй кнопку «Назначить в бан».</p></section>`;
}
const priceQuotes=new Map();
let inventoryJob=null,pricingJob=null;
function money(cents) {const value=convertUsdCents(cents,currency,fx.rates);return value==null?'—':new Intl.NumberFormat(getLocale(),{style:'currency',currency}).format(value/100);}
function currentInventoryKey() { return inventoryKey(inventoryAccount,inventoryApp,inventoryContext); }
const inventoryContexts=new Map(),contextErrors=new Map();
let csInventoryView=read('swch-cs-inventory-view','visual');
let inventorySort='original',inventoryMarket='all',inventoryFacets={},inventoryMinPrice='',inventoryMaxPrice='';
function inventoryAccounts(account=inventoryAccount) {return accounts.filter(a=>a.launcher==='steam'&&(account==='all'||a.id===account));}
function inventoryScopes(account,app=inventoryApp,context=inventoryContext) {
 if(app!=='all'&&context!=='all')return [{app_id:app,context_id:context,name:games.find(g=>g.launcher==='steam'&&g.app_id===app)?.name||commonInventories.find(([id])=>id===app)?.[1]||`Steam #${app}`}];
 const scopes=new Map((inventoryContexts.get(account.id)||[]).map(c=>[`${c.app_id}:${c.context_id}`,c]));
 for(const key of inventoryPages.keys()){const [owner,id,ctx]=JSON.parse(key);if(owner===account.id&&!scopes.has(`${id}:${ctx}`))scopes.set(`${id}:${ctx}`,{app_id:id,context_id:ctx,name:games.find(g=>g.app_id===id)?.name||`Steam #${id}`});}
 const result=[...scopes.values()].filter(c=>app==='all'||c.app_id===app);
 if(app!=='all'&&!result.length)return inventoryScopes(account,app,app==='753'?'6':'2');
 return result;
}
function inventoryView(account=inventoryAccount,app=inventoryApp,context=inventoryContext) {
 const list=inventoryAccounts(account),items=[],statuses=[];let total_count=0,loaded=0,next=false,cached=false,updated_at=null;
 for(const a of list) {
  const scopes=inventoryScopes(a,app,context);
  if(app==='all'&&(!inventoryContexts.has(a.id)||contextErrors.has(a.id)))statuses.push({account:a,name:'Список игр',error:contextErrors.get(a.id)||'Список инвентарей ещё не загружен'});
  for(const c of scopes){
   const key=inventoryKey(a.id,c.app_id,c.context_id),p=inventoryPages.get(key),error=inventoryErrors.get(key);
   if(p){loaded++;total_count+=p.total_count;next ||= Boolean(p.next_cursor);cached ||= p.cached;updated_at=!updated_at||p.updated_at<updated_at?p.updated_at:updated_at;items.push(...p.items.map(i=>({...i,account_id:a.id,app_id:c.app_id,context_id:c.context_id})));}
   statuses.push({account:a,name:c.name,app_id:c.app_id,page:p,error,pending:inventoryPending.has(key)});
  }
 }
 return {items,total_count,next_cursor:next?'more':null,cached,updated_at,statuses,loaded,complete:list.length>0&&(context!=='all'||list.every(a=>inventoryContexts.has(a.id)&&!contextErrors.has(a.id)))&&loaded===statuses.length&&!next&&!statuses.some(s=>s.error||s.page?.warning)};
}
function valueText(data) {const v=inventoryValue(data.items,priceQuotes,inventoryApp,accountRestrictions,{excludeUnsellable:excludeUnsellableBanned});return `${v.priced||data.complete&&!v.unpriced?money(v.cents):'—'}${!data.complete||v.unpriced?' · частично':''}`;}
async function hydrateInventoryCache() {
 try {const data=await api('/api/steam/inventory-cache');for(const entry of data.pages||[]){const key=inventoryKey(entry.account_id,entry.app_id,entry.context_id);if(!inventoryPages.has(key))inventoryPages.set(key,entry.page);}for(const entry of data.quotes||[])priceQuotes.set(priceKey(entry.app_id,entry.name),entry.quote);for(const entry of data.contexts||[]){inventoryContexts.set(entry.account_id,entry.data.contexts||[]);if(entry.data.warning)contextErrors.set(entry.account_id,entry.data.warning);}render();} catch {}
}
function renderSteamGameLogo(appId, name, short) {
  if (String(appId) === '570') {
    return `<span class="game-logo-icon dota2-badge" title="Dota 2">${DOTA2_LOGO_SVG}</span>`;
  }
  if (appId === 'all') {
    return `<span class="game-logo-icon all-games-badge" title="Все игры" style="background:#232634;border-radius:6px;display:inline-flex;align-items:center;justify-content:center;color:#b2b5c2"><svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true" style="width:14px;height:14px;display:block"><path fill="currentColor" d="M3 3h7v7H3zm11 0h7v7h-7zm-11 11h7v7H3zm11 0h7v7h-7z"/></svg></span>`;
  }
  if (String(appId) === '730') {
    return `<span class="game-logo-icon cs2-badge" title="CS2" style="background:linear-gradient(135deg,#de9b26,#f5af19);border-radius:6px;display:inline-flex;align-items:center;justify-content:center;color:#0b0e14;font-size:9px;font-weight:900;letter-spacing:-0.5px">CS2</span>`;
  }
  if (String(appId) === '440') {
    return `<span class="game-logo-icon tf2-badge" title="TF2" style="background:#b85c18;border-radius:6px;display:inline-flex;align-items:center;justify-content:center;color:#ffffff;font-size:9px;font-weight:900">TF2</span>`;
  }
  if (String(appId) === '252490') {
    return `<span class="game-logo-icon rust-badge" title="Rust" style="background:#cd412b;border-radius:6px;display:inline-flex;align-items:center;justify-content:center;color:#ffffff;font-size:8px;font-weight:900">RUST</span>`;
  }
  if (String(appId) === '753') {
    return `<span class="game-logo-icon steam-cards-badge" title="Steam" style="background:#171a21;border-radius:6px;display:inline-flex;align-items:center;justify-content:center;color:#66c0f4;font-size:11px">✦</span>`;
  }
  return `<span class="platform-mark" style="width:20px;height:20px;font-size:9px;border-radius:5px">${(short?.[0] || name?.[0] || '?').toUpperCase()}</span>`;
}
function renderHeroFilter(data) {
  if (inventoryApp !== '570') return '';
  const heroMap = new Map();
  for (const item of data.items) {
    for (const t of (item.tag_details || [])) {
      if (t.category && t.category.toLowerCase() === 'hero') {
        const facetKey = `${item.app_id}:${t.category}`;
        let h = heroMap.get(t.value);
        if (!h) {
          h = {
            key: facetKey,
            value: t.value,
            name: t.name,
            count: 0,
            iconUrl: getDota2HeroIconUrl(t.value),
          };
          heroMap.set(t.value, h);
        }
        h.count += (item.amount || 1);
      }
    }
  }

  if (!heroMap.size) return '';

  const heroes = [...heroMap.values()];
  heroes.sort((a, b) => {
    if (a.value === 'DOTA_OtherType') return 1;
    if (b.value === 'DOTA_OtherType') return -1;
    if (b.count !== a.count) return b.count - a.count;
    return a.name.localeCompare(b.name, getLocale());
  });

  const totalHeroItems = heroes.reduce((acc, h) => acc + h.count, 0);
  const selectedHeroes = [];
  for (const h of heroes) {
    if ((inventoryFacets[h.key] || []).includes(h.value)) {
      selectedHeroes.push(h.value);
    }
  }

  return `
    <div class="hero-filter-box">
      <div class="hero-filter-header">
        <div class="hero-filter-title">
          <span class="game-logo-icon dota2-badge hero-mark" style="width:26px;height:26px;border-radius:6px;display:inline-flex;align-items:center;justify-content:center;background:#181920!important;border:1px solid rgba(226,59,36,0.4);box-shadow:0 2px 8px rgba(226,59,36,0.3);flex-shrink:0" title="Dota 2">
            ${DOTA2_LOGO_SVG}
          </span>
          <div>
            <strong>Фильтр по героям Dota 2</strong>
            <small class="muted">${heroes.length} ${heroes.length === 1 ? 'герой' : heroes.length < 5 ? 'героя' : 'героев'} · ${totalHeroItems} предметов</small>
          </div>
        </div>
        <div class="hero-filter-actions">
          <input type="search" class="hero-search-input" id="hero-filter-search" placeholder="Поиск героя…" value="${esc(heroSearchQuery)}">
          ${selectedHeroes.length ? `<button type="button" class="text-button hero-reset-btn" data-action="reset-hero-facet">Сбросить (${selectedHeroes.length})</button>` : ''}
        </div>
      </div>
      <div class="hero-chips-container" id="hero-chips-container">
        <button type="button" class="hero-chip ${!selectedHeroes.length ? 'active' : ''}" data-action="select-hero-facet" data-hero="all">
          <span class="hero-chip-avatar all-avatar">✦</span>
          <span class="hero-chip-name">Все герои</span>
          <span class="hero-chip-badge">${totalHeroItems}</span>
        </button>
        ${heroes.map(h => {
          const isSelected = selectedHeroes.includes(h.value);
          const slug = (h.value || '').replace(/^npc_dota_hero_/, '').replace(/_persona\d*$/, '').toLowerCase();
          const isHidden = heroSearchQuery && !h.name.toLowerCase().includes(heroSearchQuery) && !slug.includes(heroSearchQuery);
          return `
            <button type="button" class="hero-chip ${isSelected ? 'active' : ''}" ${isHidden ? 'hidden' : ''} data-action="select-hero-facet" data-facet-key="${esc(h.key)}" data-hero="${esc(h.value)}" data-hero-slug="${esc(slug)}" title="${esc(h.name)}">
              ${h.iconUrl ? `
                <img class="hero-chip-icon" src="${esc(h.iconUrl)}" alt="${esc(h.name)}" loading="lazy" onerror="this.style.display='none';if(this.nextElementSibling)this.nextElementSibling.style.display='inline-flex'">
                <span class="hero-chip-avatar fallback" style="display:none">${esc(h.name.slice(0, 2))}</span>
              ` : `
                <span class="hero-chip-avatar other">${h.value === 'DOTA_OtherType' ? '★' : esc(h.name.slice(0, 2))}</span>
              `}
              <span class="hero-chip-name">${esc(h.name)}</span>
              <span class="hero-chip-badge">${h.count}</span>
            </button>
          `;
        }).join('')}
      </div>
    </div>
  `;
}
function inventoryFacetControls(data) {
  const groups = new Map();
  for (const i of data.items) {
    for (const t of (i.tag_details || [])) {
      if (t.category && t.category.toLowerCase() === 'hero') continue;
      const key = `${i.app_id}:${t.category}`;
      if (!groups.has(key)) groups.set(key, { label: t.category_name, app: i.app_id, values: new Map() });
      groups.get(key).values.set(t.value, t.name);
    }
  }
  if (!groups.size) return '';
  return `<details class="inventory-facets"><summary>Остальные категории предметов · ${groups.size} категорий</summary><div class="facet-grid">${[...groups].map(([key,g])=>`<fieldset><legend>${esc(g.label)}${inventoryApp==='all'?' · '+esc(games.find(x=>x.app_id===g.app)?.name||KNOWN_STEAM_APP_NAMES[g.app]||g.app):''}</legend>${[...g.values].sort((a,b)=>a[1].localeCompare(b[1],getLocale())).map(([v,label])=>`<label><input type="checkbox" data-inventory-facet="${esc(key)}" value="${esc(v)}" ${(inventoryFacets[key]||[]).includes(v)?'checked':''}>${esc(label)}</label>`).join('')}</fieldset>`).join('')}</div><p class="muted">Фильтры по категориям (редкость, качество, слот). Несколько значений внутри категории — «или», разные категории — «и».</p></details>`;
}
function openInventory(account,app) {
 if(inventoryJob)inventoryJob.cancelled=true;
 inventoryLauncher='steam';
 save('swch-inventory-launcher',inventoryLauncher);
 inventoryAccount=account||'all';inventoryApp=app||'all';inventoryContext='all';inventorySearch='';inventoryTag='';inventoryFacets={};heroSearchQuery='';page='inventory';render();void loadInventory();
}
function openRiotInventory(account='all',app='valorant') {
 inventoryLauncher='riot';riotInventoryAccount=account;riotInventoryGame=app;riotInventoryKind='all';riotInventorySearch='';riotWeaponCategory='';riotWeapon='';page='inventory';
 save('swch-inventory-launcher',inventoryLauncher);save('swch-riot-inventory-account',account);save('swch-riot-inventory-game',app);render();
}
const riotInventoryKindLabel=()=>riotKindLabel(riotInventoryKind,riotInventoryGame);
function riotInventoryState(){return riotInventoryData(riotProfiles,{accounts,app:riotInventoryGame,account:riotInventoryAccount,kind:riotInventoryKind,search:riotInventorySearch,category:riotWeaponCategory,weapon:riotWeapon});}
function riotWeaponFilters(data){
 if(riotInventoryGame!=='valorant'||!['all','skins'].includes(riotInventoryKind)||!data.skins.length)return '';
 return `<section class="riot-weapon-filters" aria-label="Тип оружия VALORANT"><div class="cs-category-tabs"><button type="button" class="cs-category ${!riotWeaponCategory?'active':''}" data-action="riot-weapon-category" data-value="" aria-pressed="${!riotWeaponCategory}"><span class="cs-category-image">${icon('grid')}</span><strong>Всё оружие</strong><small>${data.skins.length}</small></button>${data.categories.map(c=>`<button type="button" class="cs-category ${riotWeaponCategory===c.id?'active':''}" data-action="riot-weapon-category" data-value="${c.id}" aria-pressed="${riotWeaponCategory===c.id}"><span class="cs-category-image">${safeImage(c.image)?`<img src="${esc(safeImage(c.image))}" alt="" loading="lazy">`:icon('game')}</span><strong>${esc(c.name)}</strong><small>${c.count}</small></button>`).join('')}</div></section>`;
}
function riotInventoryGrid(data) {
 const card=item=>`<article data-riot-owner="${esc(item.account_id)}" data-riot-accounts="${esc(item.owners.map(a=>a.id).join(' '))}" data-riot-kind="${item.kind}" data-riot-item="${esc(item.id)}"><span class="riot-item-kind">${esc(riotKindLabel(item.kind,riotInventoryGame))}${item.weapon?' · '+esc(item.weapon.name):''}</span>${safeImage(item.image_url)?`<img loading="lazy" src="${esc(safeImage(item.image_url))}" alt="${esc(item.name)}">`:'<div class="inventory-placeholder">'+icon(item.kind==='champions'?'users':'diamond')+'</div>'}<strong>${esc(item.name)}</strong><div class="riot-item-owners">${item.owners.map(a=>`<small class="muted">${esc(a.name||accounts.find(acc=>acc.id===a.id)?.display_name||a.id)}</small>`).join('')}</div></article>`;
 const grid=items=>`<div class="riot-items riot-inventory-items">${items.map(card).join('')}</div>`;
 return riotInventoryKind==='all'?data.sections.map(section=>`<section class="riot-collection-section" data-riot-section="${section.kind}" aria-label="${esc(section.name)}"><h3>${esc(section.name)}<span>${section.items.length}</span></h3>${grid(section.items)}</section>`).join(''):grid(data.filtered);
}
function riotInventoryCards() {
 const data=riotInventoryState(),label=riotInventoryKindLabel();
 const missing=riotKinds(riotInventoryGame).map(kind=>({kind,count:data.pending.filter(p=>p.kind===kind).length})).filter(p=>p.count);
 return `${riotWeaponFilters(data)}<p class="muted">${label} в сохранённых коллекциях: ${data.rows.length}${data.filtered.length!==data.rows.length?' · Показано: '+data.filtered.length:''}</p>${missing.length?`<p class="inventory-warning" role="status">Ещё не загружено: ${missing.map(p=>esc(riotKindLabel(p.kind,riotInventoryGame))+' · '+p.count+' акк.').join('; ')}. Обнови коллекцию нужного аккаунта.</p>`:''}${data.errors.map(e=>`<p class="error-text">${esc(e.account)} · ${esc(riotKindLabel(e.kind,riotInventoryGame))}: ${esc(e.message)}${e.cached?' Показаны сохранённые данные.':''}</p>`).join('')}${riotInventoryGrid(data)}${!data.rows.length?`<div class="empty-state"><h3>${!data.known?'Коллекция ещё не загружена':data.pending.length?'Коллекция загружена не полностью':'В этой категории нет предметов'}</h3>${!data.known||data.pending.length?'<p>Нажми «Обновить коллекцию», чтобы получить данные выбранного аккаунта.</p>':''}</div>`:!data.filtered.length?'<p class="muted">По выбранным фильтрам ничего не найдено.</p>':''}`;
}
function riotInventoryAccountCards(list) {
 return `<div class="riot-inventory-accounts">${list.map(a=>{
  const profile=riotProfiles.find(p=>p.account_id===a.id&&p.app_id===riotInventoryGame);
  const error=riotProfileErrors.get(`${a.id}:${riotInventoryGame}`);
  const skins=profile?.skins?.value;
  const rank=profile?.ranked?.value?.rank;
  return `<article class="riot-inventory-account ${riotInventoryAccount===a.id?'selected':''}" data-riot-inventory-account="${esc(a.id)}"><header><h3>${esc(a.display_name||a.username)}</h3>${a.is_active?'<span class="riot-active-badge">В Riot Client</span>':''}</header><p class="muted small-text">${riotGameName(riotInventoryGame)}</p>${profile?`<div class="riot-account-brief"><span>Уровень <strong>${profile.level??'—'}</strong></span><span>Скины <strong>${skins==null?'—':skins.length}</strong></span>${rank?`<span class="riot-brief-rank">${esc(rank)}</span>`:''}</div><details class="riot-inventory-profile"><summary>Подробнее о профиле</summary>${riotProfileSummary(profile)}<p class="muted small-text">Сохранено: ${esc(dateLabel(profile.synced_at))}</p></details>`:'<p class="riot-account-pending muted">Данные ещё не загружены</p>'}${error?`<p class="error-text">${esc(error)}</p>`:''}<div class="account-actions"><button class="secondary" data-action="select-riot-inventory-account" data-account="${esc(a.id)}">Показать предметы</button><button class="secondary" data-action="sync-riot" data-app="${esc(riotInventoryGame)}" data-account="${esc(a.id)}" ${busy||riotSyncBusy?'disabled':''}>${icon('refresh')}Обновить</button></div></article>`;
 }).join('')}</div>`;
}
function renderRiotInventory(list,root,bar) {
 if(!list.length){root.innerHTML=bar+empty('Подключи Riot Games','Сохрани вход в Riot Client, чтобы загрузить скины.','connect','Подключить аккаунт');return;}
 if(riotInventoryAccount!=='all'&&!list.some(a=>a.id===riotInventoryAccount))riotInventoryAccount='all';
 const current=list.find(a=>a.id===riotInventoryAccount)||list.find(a=>a.is_active)||list[0];
 if(!['all',...riotKinds(riotInventoryGame)].includes(riotInventoryKind))riotInventoryKind='all';
 if(riotInventoryGame!=='valorant'||!['all','skins'].includes(riotInventoryKind)){riotWeaponCategory='';riotWeapon='';}
 let data=riotInventoryState();
 if(riotWeaponCategory&&!data.categories.some(c=>c.id===riotWeaponCategory)){riotWeaponCategory='';riotWeapon='';data=riotInventoryState();}
 if(riotWeapon&&!data.weapons.some(w=>w.id===riotWeapon))riotWeapon='';
 root.innerHTML=`${bar}<div class="inventory-toolbar"><label>Аккаунт Riot<select id="riot-inventory-account"><option value="all" ${riotInventoryAccount==='all'?'selected':''}>Все аккаунты</option>${list.map(a=>`<option value="${esc(a.id)}" ${a.id===riotInventoryAccount?'selected':''}>${esc(a.display_name||a.username)}</option>`).join('')}</select></label><label>Игра<select id="riot-inventory-game">${['valorant','league_of_legends'].map(id=>`<option value="${id}" ${id===riotInventoryGame?'selected':''}>${riotGameName(id)}</option>`).join('')}</select></label><label>Категория<select id="riot-inventory-kind">${['all',...riotKinds(riotInventoryGame)].map(kind=>`<option value="${kind}" ${kind===riotInventoryKind?'selected':''}>${esc(riotKindLabel(kind,riotInventoryGame))}</option>`).join('')}</select></label><button class="secondary" data-action="sync-riot" data-app="${esc(riotInventoryGame)}" data-account="${esc(current.id)}" ${busy||riotSyncBusy?'disabled':''}>${icon('refresh')}${riotSyncBusy?'Загружаем…':'Обновить коллекцию'}</button>${riotInventoryGame==='valorant'?`<button class="secondary" data-action="sync-all-riot" ${busy||riotSyncBusy?'disabled':''}>Обновить все аккаунты</button>`:''}</div><p class="muted small-text">${riotInventoryGame==='valorant'?'Устанавливать и запускать VALORANT не нужно. Для обновления используется Riot Client.':'SWCH автоматически запустит установленный клиент League и дождётся профиля.'} Сохранённые коллекции видны при закрытом клиенте.</p>${riotSyncProgress?`<p role="status">${esc(riotSyncProgress)}</p>`:''}<h2 class="riot-inventory-heading">Подключённые аккаунты · ${list.length}</h2>${riotInventoryAccountCards(list)}<p class="muted small-text">${riotInventoryAccount==='all'?'«Обновить коллекцию» обновляет текущий аккаунт: '+esc(current.display_name||current.username)+'. ':''}Коллекции сохранены отдельно для каждой игры и аккаунта.</p><div class="riot-inventory-tools"><label class="riot-inventory-search">Найти · ${riotInventoryKindLabel()}<input id="riot-inventory-search" type="search" value="${esc(riotInventorySearch)}" placeholder="Название"></label>${riotInventoryGame==='valorant'&&['all','skins'].includes(riotInventoryKind)&&data.skins.length?`<label class="riot-inventory-weapon">Оружие<select id="riot-inventory-weapon"><option value="">Любое оружие</option>${data.weapons.map(w=>`<option value="${w.id}" ${riotWeapon===w.id?'selected':''}>${esc(w.name)}</option>`).join('')}</select></label>`:''}</div><div id="riot-inventory-content">${riotInventoryCards()}</div>`;
 simplifyRiotInventory(root);hydrateIcons(root);
}

function renderInventory() {
 const root=$('inventory-view'),list=accounts.filter(a=>a.launcher==='steam'),epicList=accounts.filter(a=>a.launcher==='epic'),robloxList=accounts.filter(a=>a.launcher==='roblox'),riotList=accounts.filter(a=>a.launcher==='riot');
 const generation=++inventoryRenderGeneration;
 const platforms=inventoryPlatforms(accounts);
 if(!platforms.length){root.innerHTML=empty('Подключи игровой аккаунт','Добавь аккаунт платформы с инвентарём, затем обнови библиотеку.','connect','Подключить аккаунт');return;}
 if(!platforms.includes(inventoryLauncher)){inventoryLauncher=platforms[0];save('swch-inventory-launcher',inventoryLauncher);}
 const activeFortniteSummary=getActiveFortniteSummary();
 const launcherBarHtml=`<div class="inventory-launcher-bar"><div class="inventory-launcher-tabs">${platforms.map(id=>`<button type="button" class="launcher-tab-btn ${inventoryLauncher===id?'active':''}" data-inventory-launcher="${id}" aria-pressed="${inventoryLauncher===id}">${platformLogo(id)}<div class="launcher-tab-info"><strong>${esc(PLATFORMS[id].name)}</strong></div></button>`).join('')}</div>${inventoryLauncher==='epic'&&activeFortniteSummary?`<button class="secondary" data-action="fortnite-refresh" data-account="${esc(activeFortniteSummary.account_id)}">${icon('refresh')}Обновить шкафчик</button>`:''}</div>`;

  if(inventoryLauncher==='riot'){renderRiotInventory(riotList,root,launcherBarHtml);return;}
  if (inventoryLauncher === 'roblox') {
    void renderRobloxInventoryTab(robloxList, root, launcherBarHtml, generation);
    return;
  }

  if (inventoryLauncher === 'epic') {
    if (!epicList.length && !demo) {
      root.innerHTML = `${launcherBarHtml}${empty('Подключи Epic Games', 'Войди в аккаунт Epic Games, чтобы открыть шкафчик и скины Fortnite.', 'connect', 'Подключить аккаунт')}`;
      return;
    }

    const savedEpicAcc = read('swch-epic-inventory-account', null);
    if (!currentEpicInventoryAccount || !epicList.some(a => a.id === currentEpicInventoryAccount)) {
      if (savedEpicAcc && epicList.some(a => a.id === savedEpicAcc)) {
        currentEpicInventoryAccount = savedEpicAcc;
      } else {
        const activeEpic = epicList.find(a => a.is_active);
        currentEpicInventoryAccount = activeEpic?.id || epicList[0]?.id || (demo ? 'demo-epic' : '');
      }
    }

    const epicAccountSelector = epicList.length > 1 ? `
      <div style="margin-bottom:16px;display:flex;align-items:center;gap:10px">
        <label style="font-size:12px;font-weight:600">Аккаунт Epic Games:
          <select id="epic-inventory-account" style="margin-left:8px;padding:6px 12px;border:1px solid var(--line);border-radius:6px;background:var(--bg);color:var(--text)">
            ${epicList.map(a => `<option value="${esc(a.id)}" ${a.id === currentEpicInventoryAccount ? 'selected' : ''}>${esc(a.display_name || a.username)}</option>`).join('')}
          </select>
        </label>
      </div>
    ` : '';

    const summary = getActiveFortniteSummary();

    if (fortniteLoading && !summary) {
      root.innerHTML = `
        ${launcherBarHtml}
        ${epicAccountSelector}
        <div class="fortnite-loading">
          <p style="font-size:16px;font-weight:600;margin-bottom:8px">Синхронизация шкафчика Fortnite…</p>
          <p class="muted">Загружаем профиль, баланс V-Bucks и косметику из Microservice Client Platform (MCP)</p>
        </div>
      `;
      return;
    }

    if (fortniteError && !summary) {
      root.innerHTML = `
        ${launcherBarHtml}
        ${epicAccountSelector}
        <div class="fortnite-loading" style="color:var(--danger,#ef4444)">
          <p style="font-size:16px;font-weight:700">Не удалось загрузить шкафчик Fortnite</p>
          <p class="muted">${esc(fortniteError)}</p>
          <button class="secondary" style="margin-top:14px" data-action="fortnite-refresh" data-account="${esc(currentEpicInventoryAccount)}">Попробовать снова ↻</button>
        </div>
      `;
      return;
    }

    if (!summary && !demo) {
      if (!fortniteLoading) {
        void ensureFortniteSummary(currentEpicInventoryAccount);
      }
      root.innerHTML = `
        ${launcherBarHtml}
        ${epicAccountSelector}
        <div class="fortnite-loading">
          <p style="font-size:16px;font-weight:600;margin-bottom:8px">Загрузка шкафчика Fortnite…</p>
        </div>
      `;
      return;
    }

    root.innerHTML = `
      ${launcherBarHtml}
      ${epicAccountSelector}
      ${renderFortniteLockerBodyHtml(summary, { isModal: false, accountId: currentEpicInventoryAccount })}
    `;
    hydrateIcons(root);
    return;
  }

 if (!list.length) {
   root.innerHTML = `${launcherBarHtml}${empty('Подключи Steam', 'Войди в аккаунт Steam на этом компьютере, затем обнови библиотеку.', 'connect', 'Подключить аккаунт')}`;
   return;
 }

  if(inventoryAccount!=='all'&&!list.some(a=>a.id===inventoryAccount))inventoryAccount='all';
  const gamesWithItems = getInventoryGamesWithItems({ accounts, inventoryAccount, inventoryContexts, inventoryPages, games });

  const isSelectedGameValid = inventoryApp === 'all'
    || gamesWithItems.some(g => g.app_id === inventoryApp)
    || commonInventories.some(([app_id]) => app_id === inventoryApp)
    || games.some(g => String(g.app_id) === String(inventoryApp));
  if (!isSelectedGameValid && (!inventoryJob || !inventoryJob.active)) {
    inventoryApp = 'all';
    inventoryContext = 'all';
  }

  const options = new Map([
    ['all', 'Все игры'],
    ...commonInventories.map(([id, name]) => [id, name]),
    ...gamesWithItems.map(g => [g.app_id, g.name])
  ]);
  if (!options.has(inventoryApp)) {
    const customGame = games.find(g => String(g.app_id) === String(inventoryApp));
    options.set(inventoryApp, customGame?.name || KNOWN_STEAM_APP_NAMES[inventoryApp] || `Steam #${inventoryApp}`);
  }

  const popularSteamGames = [
    { app_id: 'all', name: 'Все игры', short: 'Все' },
    ...gamesWithItems.filter(g => (g.count || 0) > 0)
  ];
  if (inventoryApp !== 'all' && !popularSteamGames.some(g => g.app_id === inventoryApp)) {
    const customGame = games.find(g => String(g.app_id) === String(inventoryApp));
    const name = customGame?.name || KNOWN_STEAM_APP_NAMES[inventoryApp] || `Steam #${inventoryApp}`;
    const short = KNOWN_GAME_SHORTS[inventoryApp] || name.slice(0, 3).toUpperCase();
    popularSteamGames.push({ app_id: inventoryApp, name, short, count: 0 });
  }

  const data = inventoryView(), scope = currentInventoryKey(), pending = inventoryJob?.active && inventoryJob.scope === scope;
  const pricing = pricingJob?.active && pricingJob.scope === scope;

  const gameLogosBarHtml = `
    <div class="steam-game-logos-bar">
      ${popularSteamGames.map(g => `
        <button type="button" class="steam-game-logo-btn ${inventoryApp === g.app_id ? 'active' : ''}" data-action="select-inventory-app" data-app="${esc(g.app_id)}" title="${esc(g.name)}">
          ${renderSteamGameLogo(g.app_id, g.name, g.short)}
          <span class="game-name">${esc(g.name)}</span>
          ${g.count ? `<span class="tag-chip-count" style="margin-left:2px;font-size:10px;opacity:0.75">${format.format(g.count)}</span>` : ''}
        </button>
      `).join('')}
    </div>
  `;

  const steamCacheNote = `
    <small class="muted" style="display:block;margin-bottom:12px;font-size:11px">🕒 Инвентарь Steam актуален на: <strong>${data.updated_at ? esc(dateLabel(data.updated_at)) : 'сохранено в кэше'}</strong>${data.complete ? ' · Все страницы' : ' · Частично'}</small>
  `;

  const heroFilterHtml = renderHeroFilter(data);
  const scopedItems = inventoryHeroItems(data.items, inventoryFacets);
  const scopedData = {...data, items: scopedItems};
  const tagCounts = inventoryTagCounts(scopedItems);
  if (inventoryTag && !tagCounts.has(inventoryTag)) inventoryTag = '';
  const tagPills = [...tagCounts.entries()].sort((a, b) => a[0].localeCompare(b[0], getLocale()));

  const tagFilterHtml = data.items.length ? (inventoryApp === 'all' ? `
    <div class="inventory-tag-hint">
      <small class="muted">💡 Сначала выберите конкретную игру выше (например, <button type="button" class="text-button" data-action="select-inventory-app" data-app="570" style="text-decoration:underline;cursor:pointer;color:var(--accent);font-weight:600">Dota 2</button> с логотипом для фильтра по героям), чтобы открыть специальные фильтры.</small>
    </div>` : `
    <div class="inventory-tag-filter-box">
      <span class="inventory-tag-label">Тип / качество предметов:</span>
      <div class="inventory-tag-chips">
        <button type="button" class="tag-chip ${inventoryTag === '' ? 'active' : ''}" data-inventory-tag="">
          Все типы <span class="tag-chip-count">${scopedItems.reduce((n,i)=>n+(i.amount||1),0)}</span>
        </button>
        ${tagPills.map(([t, count]) => `
          <button type="button" class="tag-chip ${inventoryTag === t ? 'active' : ''}" data-inventory-tag="${esc(t)}">
            ${esc(t)} <span class="tag-chip-count">${count}</span>
          </button>
        `).join('')}
      </div>
    </div>`) : '';

  root.innerHTML=`${launcherBarHtml}${gameLogosBarHtml}${steamCacheNote}<div class="inventory-toolbar"><label>Аккаунт<select id="inventory-account"><option value="all" ${inventoryAccount==='all'?'selected':''}>Все аккаунты</option>${list.map(a=>`<option value="${esc(a.id)}" ${inventoryAccount===a.id?'selected':''}>${esc(a.display_name||a.username)}</option>`).join('')}</select></label><label>Игра<select id="inventory-app">${[...options].map(([id,name])=>`<option value="${esc(id)}" ${inventoryApp===id?'selected':''}>${esc(name)}</option>`).join('')}</select></label><button class="secondary" data-action="inventory-refresh" ${pending?'disabled':''}>${icon('refresh')}Обновить предметы</button></div><div class="inventory-actions">${!data.loaded?`<button class="primary" data-action="inventory-load" ${pending?'disabled':''}>Загрузить инвентарь</button>`:''}${!data.complete?`<button class="secondary" data-action="inventory-all" ${pending?'disabled':''}>Загрузить все страницы</button>`:''}${data.next_cursor?`<button class="secondary" data-action="inventory-more" ${pending?'disabled':''}>Загрузить ещё</button>`:''}${pending?'<button class="secondary" data-action="inventory-stop">Остановить загрузку</button>':''}<button class="primary" data-action="inventory-value" ${pricing||!data.items.length?'disabled':''}>Оценить стоимость</button>${pricing?'<button class="secondary" data-action="inventory-value-stop">Остановить оценку</button>':''}</div>${pricing?`<p class="muted" role="status">Получаем цены: ${pricingJob.done} / ${pricingJob.total} типов предметов…</p>`:''}${pricingJob?.scope===scope&&pricingJob.message?`<p class="inventory-warning" role="status">${esc(pricingJob.message)}</p>`:''}<div class="inventory-setting-box"><label class="inventory-toggle-label" title="Исключать из подсчёта стоимости предметы, которые невозможно продать из-за блокировок аккаунта (КТ, запрет ТП, трейд-бан) или игры (VAC-бан, блокировка в игре)"><input type="checkbox" id="inventory-exclude-banned" ${excludeUnsellableBanned?'checked':''}><span><strong>Не учитывать в стоимости заблокированные предметы (VAC, КТ)</strong></span></label><small class="muted">${excludeUnsellableBanned?'Включено: заблокированные предметы исключены из общей суммы':'Выключено: заблокированные предметы учитываются в сумме'}</small></div>${inventoryApp==='730'?'<details class="cs-valuation-details"><summary>Стоимость и подробная оценка</summary>':''}<div id="inventory-valuation"></div>${inventoryApp==='730'?'</details>':''}${heroFilterHtml}${csInventoryToolbar(data)}${data.items.length?`${inventoryApp==='730'?'<details class="cs-advanced-filters" '+(csInventoryView==='details'?'open':'')+'><summary>Все фильтры и категории</summary>':''}<div class="inventory-toolbar"><label>Поиск по загруженным предметам<input id="inventory-search" type="search" value="${esc(inventorySearch)}" placeholder="Название, оружие, редкость"></label><label>Доступность<select id="inventory-availability">${[['all','Все предметы'],['tradable','Можно обменять'],['marketable','Можно продать'],['locked','Без обмена'],['banned','Заблокированы (баны)']].map(([v,t])=>`<option value="${v}" ${inventoryAvailability===v?'selected':''}>${t}</option>`).join('')}</select></label><label>Сортировка<select id="inventory-sort">${[['original','Порядок Steam'],['newest','Новые предметы'],['name','По названию'],['price_desc','Цена: сначала дороже'],['price_asc','Цена: сначала дешевле'],['amount','По количеству']].map(([v,n])=>`<option value="${v}" ${inventorySort===v?'selected':''}>${n}</option>`).join('')}</select></label><label>Торговая площадка<select id="inventory-market">${[['all','Все'],['yes','Можно продать'],['no','Нельзя продать'],['banned','Заблокированы (баны)']].map(([v,n])=>`<option value="${v}" ${inventoryMarket===v?'selected':''}>${n}</option>`).join('')}</select></label><label>Цена за штуку от, ${esc(currency)}<input id="inventory-price-min" type="number" min="0" step="0.01" value="${esc(inventoryMinPrice)}"></label><label>до, ${esc(currency)}<input id="inventory-price-max" type="number" min="0" step="0.01" value="${esc(inventoryMaxPrice)}"></label><button class="secondary" data-action="inventory-reset">Сбросить фильтры</button></div>${inventoryFacetControls(scopedData)}${tagFilterHtml}${inventoryApp==='730'?'</details>':''}`:''}<div id="inventory-items"></div><div class="inventory-bottom-info"><div class="inventory-summary"><h3>${data.loaded?`${format.format(data.total_count)} предметов в доступных инвентарях`:'Предметы и скины'}</h3><p class="muted">Загружено ${data.items.length} записей · инвентари ${data.loaded} / ${data.statuses.length}${data.updated_at?' · '+esc(dateLabel(data.updated_at)):''}${data.complete?' · Все страницы загружены':' · Неполные данные'}</p>${inventoryAccount!=='all'&&inventoryApp!=='all'&&inventoryContext!=='all'?`<a class="text-button" href="https://steamcommunity.com/profiles/${encodeURIComponent(inventoryAccount)}/inventory/#${encodeURIComponent(inventoryApp)}_${encodeURIComponent(inventoryContext)}" target="_blank" rel="noopener noreferrer">Открыть этот инвентарь в Steam ↗</a>`:''}</div><div class="inventory-account-status">${data.statuses.map(s=>`<div>${accountAvatar(s.account)}<span><strong>${esc(s.account.display_name||s.account.username)} · ${esc(s.name)}</strong><small ${s.error?'role="alert"':''}>${s.pending?'Загружается…':s.error?esc(s.error):s.page?`${s.page.items.length} из ${s.page.total_count} · ${s.page.cached?'сохранено':'Steam'}`:'Ещё не загружен'}</small>${s.page?.warning?`<small class="error-text">${esc(s.page.warning)}</small>`:''}</span></div>`).join('')}</div><details class="inventory-custom"><summary>Другая игра или раздел инвентаря</summary><form id="inventory-custom-form"><label>AppID<input name="app_id" inputmode="numeric" pattern="[0-9]+" required value="${esc(inventoryApp==='all'?'730':inventoryApp)}"></label><label>Context ID<input name="context_id" inputmode="numeric" pattern="[0-9]+" required value="${esc(inventoryContext==='all'?'2':inventoryContext)}"></label><button class="secondary" type="submit">Открыть</button></form></details><p class="muted small-text inventory-note">Закрытые и незагруженные инвентари не считаются пустыми. Оценка использует минимальные предложения Steam Market в USD за единицу × количество и пересчитывается в выбранную валюту. Это ориентир стоимости покупки, а не сумма после комиссий или гарантия продажи. Предметы без рыночной цены не считаются бесплатными.</p></div>`;
 simplifyInventory(root,{hasItems:data.items.length>0,complete:data.complete});
 renderInventoryItems(data);
}
function csInventoryToolbar(data) {
 if(inventoryApp!=='730')return '';
 const categories=csInventoryCategories(data.items);
 const selected=categories.find(c=>(inventoryFacets[c.key]||[]).includes(c.value));
 return `<section class="cs-inventory-browser"><header><div><span class="cs-eyebrow">COUNTER-STRIKE 2</span><h3>Counter-Strike 2</h3></div><div class="cs-view-switch" role="group" aria-label="Вид инвентаря CS2"><button type="button" data-action="cs-inventory-view" data-view="visual" aria-pressed="${csInventoryView==='visual'}" class="${csInventoryView==='visual'?'active':''}">${icon('grid')}Вид CS2</button><button type="button" data-action="cs-inventory-view" data-view="details" aria-pressed="${csInventoryView==='details'}" class="${csInventoryView==='details'?'active':''}">${icon('list')}Подробный список</button></div></header><label class="cs-inventory-search">Найти предмет<input id="cs-inventory-search" type="search" value="${esc(inventorySearch)}" placeholder="Название или оружие"></label><div class="cs-category-tabs"><button type="button" class="cs-category ${!selected?'active':''}" data-action="cs-inventory-category" data-value=""><span class="cs-category-image">${icon('grid')}</span><strong>Все предметы</strong><small>${format.format(data.items.reduce((n,i)=>n+(i.amount||1),0))}</small></button>${categories.map(c=>`<button type="button" class="cs-category ${selected===c?'active':''}" data-action="cs-inventory-category" data-facet-key="${esc(c.key)}" data-value="${esc(c.value)}"><span class="cs-category-image">${c.image?`<img src="${esc(c.image)}" alt="" loading="lazy">`:icon('diamond')}</span><strong>${esc(c.name)}</strong><small>${format.format(c.count)}</small></button>`).join('')}</div></section>`;
}
function renderCsInventoryItems(items) {
 return `<div class="cs-inventory-grid">${items.map(i=>{
  const owner=accounts.find(a=>a.id===i.account_id);
  const quote=priceQuotes.get(priceKey(i.app_id,i.market_hash_name));
  const ban=isItemUnsellable(i,accountRestrictions);
  const rarity=(i.tag_details||[]).find(t=>t.category?.toLowerCase()==='rarity');
  const colors={rarity_common:'#b0c3d9',rarity_uncommon:'#5e98d9',rarity_rare:'#4b69ff',rarity_mythical:'#8847ff',rarity_legendary:'#d32ce6',rarity_ancient:'#eb4b4b',rarity_contraband:'#e4ae39'};
  const rarityColor=Object.entries(colors).find(([key])=>String(rarity?.value||'').toLowerCase().startsWith(key))?.[1]||'#718398';
  const url=`https://steamcommunity.com/profiles/${encodeURIComponent(i.account_id)}/inventory/#730_${encodeURIComponent(i.context_id||'2')}_${encodeURIComponent(i.asset_id)}`;
  return `<article class="cs-inventory-item ${ban?'is-banned-item':''}" data-owner="${esc(i.account_id)}" style="--cs-rarity:${rarityColor}"><div class="cs-item-art">${safeImage(i.icon_url)?`<img src="${esc(safeImage(i.icon_url))}" alt="${esc(i.name)}" loading="lazy">`:`<span class="inventory-placeholder">${icon('diamond')}</span>`}${i.amount>1?`<span class="cs-item-count">× ${format.format(i.amount)}</span>`:''}</div><div class="cs-item-info"><small>${esc(rarity?.name||i.item_type)}</small><h3>${esc(i.name)}</h3><div class="cs-item-footer"><span title="${esc(owner?.display_name||owner?.username||i.account_id)}">${esc(owner?.display_name||owner?.username||i.account_id)}</span><strong>${!i.marketable?'Не продаётся':ban&&excludeUnsellableBanned?'Не учитывается':quote?.unit_price_cents!=null?money(quote.unit_price_cents*i.amount):'—'}</strong></div>${ban?`<small class="error-text">${esc(ban.label)} · Нельзя продать</small>`:''}<details class="cs-item-details"><summary>Подробности</summary><p>${esc((i.tags||[]).join(' · '))}</p><p>${i.tradable?'Можно обменять':'Без обмена'}${i.marketable?' · Торговая площадка':''}</p>${i.marketable&&!ban?`<button class="secondary" data-action="sell-steam-item" data-account="${esc(i.account_id)}" data-url="${esc(url)}" data-name="${esc(i.name)}" ${busy?'disabled':''}>${icon('wallet')}Продать на ТП</button>`:''}<a class="text-button" href="${esc(url)}" target="_blank" rel="noopener noreferrer">Открыть в Steam ↗</a>${i.marketable&&i.market_hash_name?`<a class="text-button" href="https://steamcommunity.com/market/listings/730/${encodeURIComponent(i.market_hash_name)}" target="_blank" rel="noopener noreferrer">На площадке ↗</a>`:''}</details></div></article>`;
 }).join('')}</div>`;
}
function resetDotaDependentFilters() {
 inventoryTag='';
 for(const key of Object.keys(inventoryFacets))if(key.startsWith('570:')&&!key.toLowerCase().endsWith(':hero'))delete inventoryFacets[key];
}
function visibleInventoryItems(data) {return filterInventory(data.items,{query:inventorySearch,tag:inventoryTag,availability:inventoryAvailability,sort:inventorySort,market:inventoryMarket,facets:inventoryFacets,quotes:priceQuotes,minPrice:inventoryMinPrice,maxPrice:inventoryMaxPrice,exchangeRate:currency==='USD'?1:fx.rates[currency],restrictionsMap:accountRestrictions});}
function renderValuation(data,items) {
 const root=$('inventory-valuation');if(!root)return;
 const total=inventoryValue(data.items,priceQuotes,inventoryApp,accountRestrictions,{excludeUnsellable:excludeUnsellableBanned}),shown=inventoryValue(items,priceQuotes,inventoryApp,accountRestrictions,{excludeUnsellable:excludeUnsellableBanned});
 const used=[...new Set(data.items.filter(i=>i.marketable&&i.market_hash_name).map(i=>priceKey(i.app_id,i.market_hash_name)))].map(k=>priceQuotes.get(k)).filter(Boolean);
 const times=used.map(q=>q.updated_at).filter(Boolean).sort();
 root.innerHTML=`<div class="valuation-card global-valuation"><div><small>Все игры · все аккаунты</small><strong>${valueText(inventoryView('all','all'))}</strong><p>Из сохранённых и загруженных инвентарей. Для полной оценки выбери «Все игры» и «Все аккаунты» и загрузи все страницы.</p></div></div><div class="valuation-card"><div><small>Выбранные игры и аккаунты · оценка загруженных предметов${!data.complete||total.unpriced?' · частичная':''}</small><strong>${total.priced||data.complete&&!total.unpriced?money(total.cents):'—'}</strong><p>Оценено ${total.priced} шт. · без цены ${total.unpriced} · не продаётся ${total.not_marketable}${total.unsellable_banned?` · заблокировано банами ${total.unsellable_banned} шт.${excludeUnsellableBanned?' (не учтено)':''}`:''}</p></div><div><small>С учётом фильтров</small><strong>${shown.priced?money(shown.cents):'—'}</strong><p>${items.length} записей${times.length?' · цены от '+esc(dateLabel(times[0])):''}${shown.unsellable_banned?` · баны: ${shown.unsellable_banned} шт.`:''}</p></div>${used.some(q=>q.warning)?'<p class="error-text">Часть цен взята из сохранённой копии: Steam не смог обновить их.</p>':''}</div>`;
}
function renderInventoryItems(data=inventoryView()) {
 const root=$('inventory-items');if(!root)return;
 const items=visibleInventoryItems(data);renderValuation(data,items);
 if(inventoryApp==='730'&&csInventoryView==='visual'&&items.length){root.innerHTML=renderCsInventoryItems(items);return;}
 root.innerHTML=items.length?`<div class="inventory-grid">${items.map(i=>{
 const a=accounts.find(a=>a.id===i.account_id),quote=priceQuotes.get(priceKey(i.app_id,i.market_hash_name));
 const ban=isItemUnsellable(i, accountRestrictions);
 const itemSellUrl=`https://steamcommunity.com/profiles/${encodeURIComponent(i.account_id)}/inventory/#${encodeURIComponent(i.app_id)}_${encodeURIComponent(i.context_id||'2')}_${encodeURIComponent(i.asset_id)}`;
 return `<article class="inventory-item ${ban?'is-banned-item':''}" data-owner="${esc(i.account_id)}">${safeImage(i.icon_url)?`<img src="${esc(safeImage(i.icon_url))}" loading="lazy" alt="">`:`<div class="inventory-placeholder">${icon('diamond')}</div>`}<small>${esc(games.find(g=>g.app_id===i.app_id)?.name||commonInventories.find(([id])=>id===i.app_id)?.[1]||`Steam #${i.app_id}`)} · ${esc(i.item_type)}</small><h3>${esc(i.name)}</h3>${ban?`<div class="item-ban-badge" title="${esc(ban.reason||ban.label)}">⚠️ ${esc(ban.label)} · Нельзя продать</div>`:''}<p>${esc(i.tags.join(' · '))}</p><div class="item-owner">${a?accountAvatar(a):''}<span>${esc(a?.display_name||a?.username||i.account_id)}</span></div><div class="item-flags"><span>× ${format.format(i.amount)}</span><span>${i.tradable?'Обменивается':'Без обмена'}</span>${i.marketable?'<span>Торговая площадка</span>':''}${ban?`<span class="flag-banned" title="${esc(ban.reason||ban.label)}">⛔ ${esc(ban.label)}</span>`:''}</div><strong class="item-price">${!i.marketable?'Не продаётся':quote?.unit_price_cents!=null?(ban&&excludeUnsellableBanned?`<s style="color:var(--muted);margin-right:4px">${money(quote.unit_price_cents*i.amount)}</s><span class="price-excluded-badge" title="Исключено из стоимости: ${esc(ban.label)}">не учитывается</span>`:money(quote.unit_price_cents*i.amount)):'Цена неизвестна'}</strong>${ban?`<small class="item-ban-subtext">⛔ Нельзя продать: ${esc(ban.label)}${!excludeUnsellableBanned?' (учтено в сумме)':''}</small>`:''}${i.amount>1&&quote?.unit_price_cents!=null?`<small>${money(quote.unit_price_cents)} / шт.</small>`:''}${i.marketable?`<div class="item-card-actions">${ban?`<button type="button" class="secondary btn-sell-item" disabled title="Нельзя продать: ${esc(ban.label)}">${icon('ban')}Нельзя продать</button>`:`<button type="button" class="primary btn-sell-item" data-action="sell-steam-item" data-account="${esc(i.account_id)}" data-url="${esc(itemSellUrl)}" data-name="${esc(i.name)}" ${busy?'disabled':''} title="Войти в аккаунт и открыть продажу предмета в клиенте Steam">${icon('wallet')}Продать на ТП</button>`}<div class="item-card-actions-row">${i.market_hash_name?`<a class="text-button" target="_blank" rel="noopener noreferrer" href="https://steamcommunity.com/market/listings/${encodeURIComponent(i.app_id)}/${encodeURIComponent(i.market_hash_name)}" title="Смотреть лоты на Торговой площадке">На площадке ↗</a>`:''}<a class="text-button" target="_blank" rel="noopener noreferrer" href="${esc(itemSellUrl)}" title="Открыть страницу предмета в браузере">В браузере ↗</a></div></div>`:''}</article>`;
 }).join('')}</div>`:data.loaded?`<div class="inventory-empty"><h3>${data.complete&&data.total_count===0?'Инвентарь пуст':'Нет предметов для показа'}</h3><p class="muted">${data.complete&&data.total_count===0?'Steam подтвердил отсутствие предметов.':'Проверь фильтры и статус загрузки аккаунтов.'}</p></div>`:'';
}
async function loadInventory(force=false,more=false,allPages=false) {
 if(inventoryLauncher!=='steam'||demo||!inventoryAccounts().length)return;
 if(inventoryJob)inventoryJob.cancelled=true;
 const job={scope:currentInventoryKey(),active:true,cancelled:false},app=inventoryApp,context=inventoryContext,list=inventoryAccounts();inventoryJob=job;
 const update=()=>{if(page==='inventory')renderInventory();};update();
 try {
  for(const account of list) {
   if(job.cancelled)break;
   if((app==='all'||context==='all')&&(force||!inventoryContexts.has(account.id))){
    try {const data=await api('/api/steam/inventory-contexts?'+new URLSearchParams({account_id:account.id,refresh:String(force)}));if(!Array.isArray(data.contexts))throw new Error('Steam не предоставил список инвентарей');inventoryContexts.set(account.id,data.contexts);if(data.warning)contextErrors.set(account.id,data.warning);else contextErrors.delete(account.id);}
    catch(e){contextErrors.set(account.id,e.message);}update();
   }
   for(const scope of inventoryScopes(account,app,context)){
   if(job.cancelled)break;
   const {app_id:targetApp,context_id:targetContext}=scope;
   const key=inventoryKey(account.id,targetApp,targetContext);let previous=inventoryPages.get(key),first=true;
   if(inventoryPending.has(key))continue;
   if(!force&&!more&&!allPages&&previous)continue;
   if(!force&&(more||allPages)&&previous&&!previous.next_cursor)continue;
   const seen=new Set();
   for(let pageNumber=0;pageNumber<200;pageNumber++) {
    if(job.cancelled)break;
    const append=Boolean(previous&&(!force||!first)&&(more||allPages)),cursor=append?previous.next_cursor:null;
    if(append&&!cursor)break;
    if(cursor&&seen.has(cursor)){inventoryErrors.set(key,'Steam повторил страницу. Обнови инвентарь.');break;}
    if(cursor)seen.add(cursor);
    const params=new URLSearchParams({account_id:account.id,app_id:targetApp,context_id:targetContext});
    if(force)params.set('refresh','true');if(cursor)params.set('cursor',cursor);
    inventoryPending.add(key);inventoryErrors.delete(key);update();
    try {
     const data=await api('/api/steam/inventory?'+params);
     if(cursor&&data.next_cursor===cursor)throw new Error('Steam повторил страницу. Обнови инвентарь.');
     previous=appendInventory(append?previous:null,data);inventoryPages.set(key,previous);
    }catch(e){inventoryErrors.set(key,e.message);break;}
    finally{inventoryPending.delete(key);update();}
    first=false;
    if(!allPages||!previous.next_cursor)break;
    if(pageNumber===199)inventoryErrors.set(key,'Достигнут предел загрузки за один раз. Нажми «Загрузить ещё», чтобы продолжить.');
   }
  }
   }
 } finally {job.active=false;update();}
}
async function estimateInventory() {
 const data=inventoryView(),app=inventoryApp,scope=currentInventoryKey();
 if(pricingJob)pricingJob.cancelled=true;
 const names=[...new Set(data.items.filter(i=>i.marketable&&i.market_hash_name).map(i=>priceKey(i.app_id,i.market_hash_name)))];
 const job={scope,active:true,cancelled:false,done:0,total:names.length,message:''};pricingJob=job;
 const update=()=>{if(page==='inventory'&&currentInventoryKey()===scope)renderInventory();};update();
 try {
  for(const priceIdentity of names) {
   const [targetApp,name]=JSON.parse(priceIdentity);
   if(job.cancelled){job.message='Оценка остановлена. Полученные цены сохранены; можно продолжить.';break;}
   const key=priceKey(targetApp,name),cached=priceQuotes.get(key);
   if(!cached||cached.warning||Date.now()-new Date(cached.updated_at).valueOf()>3600000) {
    try {priceQuotes.set(key,await api('/api/steam/price?'+new URLSearchParams({app_id:targetApp,market_hash_name:name})));}
    catch(e){job.message=e.message+' Полученная часть оценки сохранена. Повтори оценку позже.';break;}
   }
   job.done++;update();
  }
  if(!job.message)job.message=names.length?'Оценка загруженных предметов завершена.':'В загруженных данных нет предметов с доступной торговой площадкой.';
 }finally{job.active=false;update();}
}
$('steam-account-filter').addEventListener('change',e=>{steamAccountFilter=e.target.value;if(steamAccountFilter!=='all')for(const g of games.filter(g=>g.launcher==='steam'))selectedAccounts[gameKey(g)]=steamAccountFilter;renderLibrary();hydrateIcons();});
$('steam-access-filter').addEventListener('change',e=>{steamAccessFilter=e.target.value;renderLibrary();hydrateIcons();});
document.addEventListener('input',e=>{
 if(e.target.id==='custom-accent-color'){updateCustomAccentColor(e.target.value);return;}
 if(e.target.id==='riot-inventory-search'){riotInventorySearch=e.target.value;$('riot-inventory-content').innerHTML=riotInventoryCards();return;}
 if(e.target.id==='inventory-search'||e.target.id==='cs-inventory-search'){
  inventorySearch=e.target.value;
  const other=$(e.target.id==='inventory-search'?'cs-inventory-search':'inventory-search');
  if(other)other.value=inventorySearch;
 }
 else if(e.target.id==='inventory-price-min')inventoryMinPrice=e.target.value;
 else if(e.target.id==='inventory-price-max')inventoryMaxPrice=e.target.value;
  else if(e.target.id==='hero-filter-search'){
    heroSearchQuery=e.target.value.trim().toLowerCase();
    const chipsContainer=$('hero-chips-container');
    if(chipsContainer){
      const chips=chipsContainer.querySelectorAll('.hero-chip[data-facet-key]');
      chips.forEach(chip=>{
        const heroName=(chip.getAttribute('title')||'').toLowerCase();
        const heroSlug=(chip.dataset.heroSlug||'').toLowerCase();
        const match=!heroSearchQuery||heroName.includes(heroSearchQuery)||heroSlug.includes(heroSearchQuery);
        chip.hidden=!match;
      });
    }
    return;
  }
 else if(e.target.id==='inventory-fortnite-search'||e.target.id==='fortnite-search-input'){
   currentFortniteSearch=e.target.value;
   const otherId=e.target.id==='inventory-fortnite-search'?'fortnite-search-input':'inventory-fortnite-search';
   const otherEl=$(otherId);
   if(otherEl&&otherEl.value!==currentFortniteSearch)otherEl.value=currentFortniteSearch;
   updateFortniteItems();
   return;
 }
 else return;
 renderInventoryItems();
});
document.addEventListener('change',e=>{
 if(e.target.id==='custom-accent-color'){updateCustomAccentColor(e.target.value);toast('Цвет интерфейса обновлён');return;}
 if(e.target.matches('[data-deadlock-hero-select]')){deadlockHeroSelection.set(gameStatsKey('1422450',e.target.dataset.account),e.target.value);paintGameStats();return;}
 if(e.target.id==='riot-inventory-kind'){riotInventoryKind=e.target.value;save('swch-riot-inventory-kind-v2',riotInventoryKind);riotInventorySearch='';riotWeaponCategory='';riotWeapon='';renderInventory();return;}
 if(e.target.id==='riot-inventory-weapon'){riotWeapon=e.target.value;if(riotWeapon)riotInventoryKind='skins';renderInventory();return;}
 if(e.target.id==='riot-inventory-account'||e.target.id==='riot-inventory-game'){
  if(e.target.id==='riot-inventory-account'){riotInventoryAccount=e.target.value;save('swch-riot-inventory-account',riotInventoryAccount);}
  else {riotInventoryGame=e.target.value;riotInventoryKind='all';save('swch-riot-inventory-game',riotInventoryGame);}
  riotWeaponCategory='';riotWeapon='';
  riotInventorySearch='';renderInventory();return;
 }
 if(e.target.id==='inventory-fortnite-rarity'||e.target.id==='fortnite-rarity-select'){
   currentFortniteRarity=e.target.value;
   const otherId=e.target.id==='inventory-fortnite-rarity'?'fortnite-rarity-select':'inventory-fortnite-rarity';
   const otherEl=$(otherId);
   if(otherEl&&otherEl.value!==currentFortniteRarity)otherEl.value=currentFortniteRarity;
   updateFortniteItems();
   return;
 }
 if(e.target.id==='epic-inventory-account'){
   currentEpicInventoryAccount=e.target.value;
   save('swch-epic-inventory-account', currentEpicInventoryAccount);
   if(!demo) void api('/api/epic/switch', { account_id: currentEpicInventoryAccount }).catch(()=>{});
   void ensureFortniteSummary(currentEpicInventoryAccount);
   renderInventory();
   return;
 }
 if(e.target.dataset.inventoryFacet){const key=e.target.dataset.inventoryFacet,values=new Set(inventoryFacets[key]||[]);e.target.checked?values.add(e.target.value):values.delete(e.target.value);inventoryFacets[key]=[...values];renderInventoryItems();return;}
 if(e.target.id==='inventory-exclude-banned'){excludeUnsellableBanned=e.target.checked;save('swch-exclude-unsellable-banned',excludeUnsellableBanned);renderInventory();return;}
 if(e.target.id==='inventory-sort'){inventorySort=e.target.value;renderInventoryItems();return;}
 if(e.target.id==='inventory-market'){inventoryMarket=e.target.value;renderInventoryItems();return;}
 if(e.target.id==='inventory-price-min'){inventoryMinPrice=e.target.value;renderInventoryItems();return;}
 if(e.target.id==='inventory-price-max'){inventoryMaxPrice=e.target.value;renderInventoryItems();return;}
 if(e.target.id==='inventory-tag'){inventoryTag=e.target.value;renderInventoryItems();return;}
 if(e.target.id==='inventory-availability'){inventoryAvailability=e.target.value;renderInventoryItems();return;}
 if(!['inventory-account','inventory-app'].includes(e.target.id))return;
 if(e.target.id==='inventory-account')inventoryAccount=e.target.value;
 else {inventoryApp=e.target.value;inventoryContext='all';}
 inventorySearch='';inventoryTag='';inventoryFacets={};heroSearchQuery='';renderInventory();void loadInventory();
});
document.addEventListener('submit',e=>{
 if(e.target.id!=='inventory-custom-form')return;e.preventDefault();
 const form=new FormData(e.target),app=String(form.get('app_id')),context=String(form.get('context_id'));
 if(!/^\d{1,10}$/.test(app)||!/^\d{1,20}$/.test(context)||Number(app)<1||Number(context)<1){toast('Введи числовые AppID и Context ID.');return;}
 inventoryApp=app;inventoryContext=context;inventorySearch='';inventoryTag='';inventoryFacets={};heroSearchQuery='';renderInventory();void loadInventory();
});
$('steam-sync-form').addEventListener('submit',async e=>{
 e.preventDefault();if(busy)return;const button=e.submitter;button.disabled=true;busy=true;$('steam-sync-error').textContent='';
 const body={account_id:$('steam-sync-account').value,api_key:$('steam-api-key').value.trim()||null,access_token:$('steam-access-token').value.trim()||null};
 $('steam-api-key').value='';$('steam-access-token').value='';
 try {const data=await api('/api/steam/library/sync',body);applySteam(data);$('connect-dialog').close();render();notice(data.warnings.join(' '));toast('Данные Steam обновлены');}
 catch(e){$('steam-sync-error').textContent=e.message;}finally{busy=false;button.disabled=false;render();}
});
$('connect-dialog').addEventListener('close',()=>{$('steam-api-key').value='';$('steam-access-token').value='';});

function renderCurrency(){
 const select=$('currency-select');if(!select)return;select.value=currency;
 const rate=currency==='USD'?1:fx.rates[currency];
 $('currency-note').textContent=currency==='USD'?'Оценки по текущим ценам игр в USD (США).':fxLoading?'Загружаем курс валюты…':rate?`Пересчёт USD → ${currency} · ${fx.source||'НБРБ'} ${fx.date||''}. Это пересчёт, а не региональная цена в магазине. ${fx.warning||fxError||''}`:`Курс ${currency} недоступен. ${fxError||'Нажми «Обновить курс».'}`;
}
async function loadCurrencies(){
 if(fxLoading)return;fxLoading=true;fxError='';renderCurrency();
 try{const data=await api('/api/currencies');if(data.base!=='USD'||!data.rates)throw new Error('Не удалось получить курсы валют');fx=data;}
 catch(e){fxError=e.message;}
 finally{fxLoading=false;render();}
}
$('currency-select')?.addEventListener('change',e=>{
 if(!Object.hasOwn(CURRENCIES,e.target.value))return;
 currency=e.target.value;save('swch-currency',currency);inventoryMinPrice='';inventoryMaxPrice='';render();
 if(currency!=='USD'&&!fx.rates[currency])void loadCurrencies();
});
async function hydrateGamePrices(){
 try{
  const [steamData, epicData] = await Promise.allSettled([
    api('/api/steam/game-prices'),
    api('/api/epic/game-prices')
  ]);
  const allPrices = [
    ...(steamData.status === 'fulfilled' && Array.isArray(steamData.value) ? steamData.value : []),
    ...(epicData.status === 'fulfilled' && Array.isArray(epicData.value) ? epicData.value : [])
  ];
  for(const quote of allPrices){
    const key=quote.currency?`${quote.currency}:${quote.app_id}`:String(quote.app_id);
    gamePriceQuotes.set(key,quote);
    if(!gamePriceQuotes.has(String(quote.app_id)))gamePriceQuotes.set(String(quote.app_id),quote);
  }
  if(page==='accounts')render();
 }catch{}
}
function renderLibraryValueToolbar(){
 return `<div class="library-value-toolbar"><button class="primary" data-action="library-value" data-account="all" ${libraryPriceJob?.active?'disabled':''}>Оценить игры на всех аккаунтах</button>${libraryPriceJob?.active?'<button class="secondary" data-action="library-value-stop">Остановить оценку</button>':''}<p class="muted" role="status">${libraryPriceJob?.active?`Получаем цены игр: ${libraryPriceJob.done} / ${libraryPriceJob.total}`:esc(libraryPriceJob?.message||'')}</p><p class="muted small-text">Цены игр Steam и Epic Games в валюте каждого аккаунта с действующими региональными скидками. Игры без отдельной цены считаются бесплатными и оцениваются в 0.</p></div>`;
}
function renderLibraryValue(a){
 const accCurrency=getAccountCurrency(a);
 const isEpic=a.launcher==='epic';
 const epicGamesForAcc=games.filter(g=>g.launcher==='epic'&&(g.owner_id===a.id||(g.owner_ids&&g.owner_ids.includes(a.id))));
 const rows=isEpic
   ? epicGamesForAcc.map(g=>({account_id:a.id,app_id:g.app_id,name:g.name,access:'owned',launcher:'epic',is_free:g.is_free}))
   : steamRows;
 const value=accountLibraryValue(rows,a.id,gamePriceQuotes,accCurrency,{treatUnpricedAsFree:true});
 const sources=steamLibrarySources[a.id]||[];
 const ownedKnown=isEpic ? true : (sources.includes('owned')||steamRows.some(r=>r.account_id===a.id&&(r.source==='steam_api'||r.access==='local'||r.access==='owned')));
 const familyKnown=isEpic ? true : (sources.includes('family')||steamRows.some(r=>r.account_id===a.id&&(r.source==='steam_family'||r.access==='family')));
 const libraryKnown=ownedKnown&&(value.family.total===0||familyKnown);
 const groupMoney=(g,confirmed)=>g.known||g.total===0&&confirmed?formatAccountMoney(g.cents,accCurrency):'—';

 const coverage=g=>{
   if(g.total===0||(g.known>=g.total&&g.unknown===0))return '';
   return `${g.known} / ${g.total} игр оценено${g.free?' · бесплатных '+g.free:''}${g.unknown?' · без цены '+g.unknown:''}`;
 };
 const totalKnown=value.owned.known+value.family.known,unknown=value.owned.unknown+value.family.unknown;

 const ownedCoverage=coverage(value.owned);
 const ownedHtml=ownedCoverage?`<small>${ownedCoverage}${!ownedKnown?' · библиотека не синхронизирована':''}</small>`:(!ownedKnown?'<small class="muted">библиотека не синхронизирована</small>':'');

 const familyCoverage=coverage(value.family);
 const familyCoverageHtml=familyCoverage?`<small>${familyCoverage}${!familyKnown?' · семейная библиотека не синхронизирована':''}</small>`:(!familyKnown?'<small class="muted">семейная библиотека не синхронизирована</small>':'');
 const familyHtml=value.family.total>0?`<div class="library-value-row"><span>Семейный доступ</span><strong>${groupMoney(value.family,familyKnown)}</strong></div>${familyCoverageHtml}`:'';

 const statusText=unknown?'Частичная оценка':'Все игры оценены';

 return `<section class="account-library-value" data-library-account="${esc(a.id)}"><h4>Стоимость игр (${esc(accCurrency)})</h4><div class="library-value-row"><span>Собственные игры</span><strong>${groupMoney(value.owned,ownedKnown)}</strong></div>${ownedHtml}${familyHtml}<div class="library-value-row total"><span>Всего доступно</span><strong>${totalKnown||libraryKnown&&unknown===0?formatAccountMoney(value.total_cents,accCurrency):'—'}</strong></div><small>${statusText}${value.unconfirmed?' · владение не подтверждено для '+value.unconfirmed+' игр':''}</small><button class="secondary" data-action="library-value" data-account="${esc(a.id)}" ${libraryPriceJob?.active?'disabled':''}>Оценить игры (${esc(accCurrency)})</button>${!isEpic&&(!libraryKnown||value.unconfirmed)?'<p class="muted small-text">Обнови Steam онлайн, чтобы подтвердить собственные и семейные игры.</p>':''}</section>`;
}
async function estimateLibrary(accountId='all'){
 if(libraryPriceJob?.active)return;
 const targetAccounts=accounts.filter(a=>(a.launcher==='steam'||a.launcher==='epic')&&(accountId==='all'||a.id===accountId));
 const targetPairs=[];
 const seen=new Set();
 let totalCandidateGames=0;
 for(const a of targetAccounts){
   const accCurrency=getAccountCurrency(a);
   const accCountry=getAccountCountry(a);
   if(a.launcher==='steam'){
     const appIds=steamRows
       .filter(r=>r.account_id===a.id&&(['owned','family','local'].includes(r.access))&&!isIgnoredSteamApp(r))
       .map(r=>String(r.app_id));
     for(const app of appIds){
       const pairKey=`${accCurrency}:${app}`;
       if(!seen.has(pairKey)){
         seen.add(pairKey);
         totalCandidateGames++;
         const cached=gamePriceQuotes.get(pairKey)||(gamePriceQuotes.get(app)?.currency===accCurrency?gamePriceQuotes.get(app):null);
         if(cached && cached.updated_at){
           continue;
         }
         targetPairs.push({app,currency:accCurrency,country:accCountry,launcher:'steam'});
       }
     }
   } else if(a.launcher==='epic'){
     const epicGamesForAcc=games.filter(g=>g.launcher==='epic'&&(g.owner_id===a.id||(g.owner_ids&&g.owner_ids.includes(a.id))));
     for(const g of epicGamesForAcc){
       const app=String(g.app_id);
       const pairKey=`${accCurrency}:${app}`;
       if(!seen.has(pairKey)){
         seen.add(pairKey);
         totalCandidateGames++;
         const cached=gamePriceQuotes.get(pairKey)||(gamePriceQuotes.get(app)?.currency===accCurrency?gamePriceQuotes.get(app):null);
         if(cached && cached.updated_at){
           continue;
         }
         targetPairs.push({app,title:g.name,currency:accCurrency,country:accCountry,launcher:'epic'});
       }
     }
   }
 }

 if(!targetPairs.length){
   const message = totalCandidateGames > 0
     ? 'Все игры уже оценены.'
     : 'Нет подтверждённых игр для оценки.';
   libraryPriceJob = {active:false,cancelled:false,done:0,total:0,failed:0,message};
   if(page==='accounts')render();
   return;
 }

 const job={active:true,cancelled:false,done:0,total:targetPairs.length,failed:0,message:''};libraryPriceJob=job;
 const update=()=>{if(page==='accounts')render();};update();
 try{
  let consecutiveFailures=0;
  for(const item of targetPairs){
   if(job.cancelled){job.message='Оценка остановлена. Полученные цены сохранены.';break;}
   const {app, currency:accCurrency, country:accCountry, launcher:itemLauncher, title:itemTitle} = item;
   const cacheKey=`${accCurrency}:${app}`;
   const cached=gamePriceQuotes.get(cacheKey)||(gamePriceQuotes.get(app)?.currency===accCurrency?gamePriceQuotes.get(app):null);
   if(cached && cached.updated_at){
     job.done++;update();
     continue;
   }
   try{
     let quote;
     if(itemLauncher === 'epic') {
       const params = new URLSearchParams({app_id: app, title: itemTitle || '', cc: accCountry, currency: accCurrency});
       quote = await api('/api/epic/game-price?' + params);
     } else {
       const params = new URLSearchParams({app_id: app, cc: accCountry, currency: accCurrency});
       quote = await api('/api/steam/game-price?' + params);
     }
     gamePriceQuotes.set(cacheKey, quote);
     if(!gamePriceQuotes.has(app)) gamePriceQuotes.set(app, quote);
     if(itemTitle) {
       gamePriceQuotes.set(`${accCurrency}:${itemTitle.toLowerCase()}`, quote);
       gamePriceQuotes.set(itemTitle.toLowerCase(), quote);
     }
     consecutiveFailures=0;
   }
   catch(e){
     const fallbackQuote = {
       app_id: app,
       currency: accCurrency,
       region: accCountry,
       price_cents: 0,
       is_free: true,
       updated_at: new Date().toISOString(),
       cached: true
     };
     gamePriceQuotes.set(cacheKey, fallbackQuote);
     if(!gamePriceQuotes.has(app)) gamePriceQuotes.set(app, fallbackQuote);
     consecutiveFailures++;
     if(e.message.includes('частот')||consecutiveFailures>=5){
       job.message=e.message+' Часть цен оценена как бесплатные.';break;
     }
   }
   job.done++;update();
  }
  if(!job.message)job.message='Все игры оценены. Игры без отдельной цены учтены как бесплатные.';
 }finally{job.active=false;update();}
}

let gamepadManager = null;
function initGamepadIntegration() {
  const enabledToggle = $('gamepad-enabled-toggle');
  const vibrationToggle = $('gamepad-vibration-toggle');
  const layoutSelect = $('gamepad-layout-select');

  gamepadManager = new GamepadManager({
    toast,
    onPageChange: (newPage) => {
      page = newPage;
      render();
    },
    onPlatformChange: (newPlatform) => {
      platform = newPlatform;
      gogAccountFilter = 'all';
      if (page !== 'accounts') page = 'library';
      render();
    },
    onSettingsToggle: () => {
      const dlg = $('styles-dialog');
      if (dlg?.open) dlg.close();
      else showStyles();
    },
    onLayoutToggle: () => {
      layout = layout === 'cards' ? 'detail' : 'cards';
      save('swch-layout', layout);
      render();
    },
  });

  if (enabledToggle) {
    enabledToggle.checked = gamepadManager.enabled;
    enabledToggle.addEventListener('change', () => {
      gamepadManager.enabled = enabledToggle.checked;
      gamepadManager.saveStorage('swch-gamepad-enabled', gamepadManager.enabled);
      gamepadManager.updateFooterIndicator();
      if (!gamepadManager.enabled) gamepadManager.deactivateGamepadMode();
    });
  }

  if (vibrationToggle) {
    vibrationToggle.checked = gamepadManager.vibration;
    vibrationToggle.addEventListener('change', () => {
      gamepadManager.vibration = vibrationToggle.checked;
      gamepadManager.saveStorage('swch-gamepad-vibration', gamepadManager.vibration);
      if (gamepadManager.vibration) gamepadManager.vibrate('confirm');
    });
  }

  if (layoutSelect) {
    layoutSelect.value = gamepadManager.layoutPreference;
    layoutSelect.addEventListener('change', () => {
      gamepadManager.layoutPreference = layoutSelect.value;
      gamepadManager.saveStorage('swch-gamepad-layout', gamepadManager.layoutPreference);
      gamepadManager.detectedStyle = gamepadManager.layoutPreference === 'auto'
        ? (gamepadManager.activeGamepadName ? detectControllerType(gamepadManager.activeGamepadName) : 'xbox')
        : gamepadManager.layoutPreference;
      gamepadManager.updateHUD();
      gamepadManager.renderSettingsDevices();
    });
  }

  const footerGamepad = $('footer-gamepad');
  if (footerGamepad) {
    footerGamepad.addEventListener('click', () => {
      showStyles();
      $('settings-gamepad-group')?.scrollIntoView({ behavior: 'smooth' });
    });
  }

  gamepadManager.updateFooterIndicator();
}

const refreshStartup = initStartup({toast, demo});
initGamepadIntegration();
$('mobile-platform').addEventListener('change',e=>{platform=e.target.value;gogAccountFilter='all';if(page!=='accounts')page='library';render();});
setupMods({api,toast,demo,changed:()=>refresh(true),render,gameKey,games:()=>games,accountForGame:chosenAccount,navigate:next=>{const modal=$('game-modal');if(modal?.open)modal.close();page=next;render();},openGame:key=>{page='library';render();openGameModal(key);}});
restoreLibraryCache();hydrateIcons();await refresh();void syncModules();setInterval(syncModules, 10000);void loadCurrencies();if(!demo){void refreshSteamRestrictions();setInterval(refreshSteamRestrictions,15*60*1000);void hydrateInventoryCache();void hydrateGamePrices();void hydrateMissingSteamNames();void pollDownloads();setInterval(pollDownloads,2000);setInterval(pollUbisoftAccounts,3000);setInterval(pollUbisoftAutoLogin,1000);setTimeout(() => void checkForUpdates(false), 1500);}

// --- Modular Launcher Engine Integration ---
let installedModules = [];

async function syncModules() {
  try {
    const res = await fetch('/api/modules');
    if (!res.ok) return;
    const data = await res.json();
    installedModules = data.modules || [];
    renderModulesUI();
  } catch (e) {
    console.warn('Failed to fetch modules:', e);
  }
}

function renderModulesUI() {
  // 1. Update Platform buttons on sidebar
  for (const m of installedModules) {
    const btn = document.querySelector(`.platform-item[data-platform="${m.id}"]`);
    if (!btn) continue;
    if (m.active) {
      btn.style.display = 'flex';
      btn.title = `${m.name}: Модуль активен (${Math.round(m.ram_bytes / 1024 / 1024)} МБ ОЗУ)`;
    } else if (m.status === 'installing') {
      btn.style.display = 'flex';
      btn.title = `${m.name}: Загрузка модуля...`;
    } else if (m.id === 'gog' && m.status !== 'disabled') {
      btn.style.display = 'flex';
      btn.title = 'GOG: подключи аккаунт для онлайн-библиотеки. Клиент GALAXY нужен для запуска игр.';
    } else {
      btn.style.display = 'none';
      if (platform === m.id) {
        platform = 'all';
        render();
      }
    }
  }

  // 2. Update Settings -> Platform Modules List
  const container = document.getElementById('modules-list');
  if (container) {
    container.innerHTML = installedModules.map(m => {
      const isDetected = m.launcher_detected;
      const isActive = m.active;
      const ramMb = Math.round(m.ram_bytes / 1024 / 1024);
      return `
        <div class="module-row" style="display:flex; justify-content:space-between; align-items:center; padding:10px; background:rgba(255,255,255,0.04); border-radius:8px; border:1px solid rgba(255,255,255,0.08);">
          <div style="flex:1; margin-right:12px;">
            <div style="display:flex; align-items:center; gap:8px;">
              <strong>${esc(m.name)}</strong>
              <span style="font-size:11px; padding:2px 6px; border-radius:4px; font-weight:600; background:${isActive ? 'rgba(74,222,128,0.2); color:#4ade80' : isDetected ? 'rgba(250,204,21,0.2); color:#facc15' : 'rgba(255,255,255,0.1); color:#999'};">
                ${isActive ? `Активен (${ramMb} МБ ОЗУ)` : m.status==='disabled'?'Отключён':m.id==='gog'?'Онлайн-библиотека доступна':isDetected ? 'Готов к подключению' : 'Лаунчер не найден (0 МБ ОЗУ)'}
              </span>
            </div>
            <small class="muted" style="display:block; margin-top:2px;">${esc(m.description)}</small>
            ${m.launcher_path ? `<small class="muted" style="display:block; font-size:10px; opacity:0.7;">Путь: ${esc(m.launcher_path)}</small>` : ''}
          </div>
          <div style="display:flex; gap:6px; flex-shrink:0;">
            ${!isActive ? `<button type="button" class="secondary" style="font-size:12px; padding:4px 8px;" onclick="window.installModule('${m.id}')">Включить</button>` : ''}
            ${isActive ? `<button type="button" class="secondary" style="font-size:12px; padding:4px 8px;" onclick="window.toggleModule('${m.id}', false)">Отключить</button>` : ''}
            <button type="button" class="secondary" style="font-size:12px; padding:4px 8px; color:#ff8373;" onclick="window.purgeModule('${m.id}')">Очистить</button>
          </div>
        </div>
      `;
    }).join('');
  }
}

window.installModule = async function(id) {
  try {
    toast(`Установка модуля ${id}...`);
    const res = await fetch(`/api/modules/${id}/install`, { method: 'POST' });
    const data = await res.json();
    if (res.ok) {
      toast(data.message || 'Модуль активирован');
      await syncModules();
      await refresh();
    } else {
      toast(data.error || 'Ошибка установки');
    }
  } catch (e) {
    toast(e.message);
  }
};

window.toggleModule = async function(id, active) {
  try {
    const res = await fetch(`/api/modules/${id}/toggle`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ active })
    });
    const data = await res.json();
    if (res.ok) {
      toast(data.message || 'Статус изменен');
      await syncModules();
      await refresh();
    } else {
      toast(data.error || 'Ошибка изменения статуса');
    }
  } catch (e) {
    toast(e.message);
  }
};

window.purgeModule = async function(id) {
  if (!confirm(`Удалить данные и кэш модуля ${id}?`)) return;
  try {
    const res = await fetch(`/api/modules/${id}/purge`, { method: 'POST' });
    const data = await res.json();
    if (res.ok) {
      toast(data.message || 'Данные модуля удалены');
      await syncModules();
      await refresh();
    } else {
      toast(data.error || 'Ошибка очистки модуля');
    }
  } catch (e) {
    toast(e.message);
  }
};

document.getElementById('btn-scan-modules')?.addEventListener('click', async () => {
  try {
    toast('Сканирование системы на наличие лаунчеров...');
    const res = await fetch('/api/modules/scan', { method: 'POST' });
    const data = await res.json();
    if (res.ok) {
      const added = data.newly_installed?.length ? ` Найдено новых: ${data.newly_installed.join(', ')}` : ' Новых лаунчеров не обнаружено.';
      toast('Сканирование завершено.' + added);
      await syncModules();
      await refresh();
    }
  } catch (e) {
    toast('Ошибка сканирования: ' + e.message);
  }
});

// -------------------------------------------------------------
// In-App Self Updater
// -------------------------------------------------------------
let updateCheckData = null;
let updatePollingActive = false;

async function checkForUpdates(manual = false) {
  const btn = $('btn-check-update');
  const badge = $('update-badge-version');
  const statusTitle = $('update-status-title');
  const statusDesc = $('update-status-desc');
  const panel = $('update-available-panel');
  const card = $('update-status-card');
  const indicator = $('update-indicator-pill');

  if (manual && btn) {
    btn.disabled = true;
    btn.innerHTML = '<span>Проверка…</span>';
  }

  try {
    const res = await fetch('/api/update/check' + (manual ? '?force=false' : ''));
    if (!res.ok) {
      throw new Error(`Ошибка проверки обновлений (${res.status})`);
    }
    const data = await res.json();
    updateCheckData = data;

    if (badge) badge.textContent = `v${data.current_version}`;

    if (data.update_available) {
      if (indicator) indicator.hidden = false;
      if (panel) panel.style.display = updatePollingActive ? 'none' : 'flex';
      if (card) card.classList.add('has-update');

      if ($('update-new-version')) $('update-new-version').textContent = `v${data.latest_version}`;
      if ($('update-release-date')) {
        $('update-release-date').textContent = data.published_at
          ? `Выпущено: ${new Date(data.published_at).toLocaleDateString()}`
          : '';
      }
      if ($('update-changelog')) {
        $('update-changelog').textContent = data.release_notes || 'Новая версия содержит исправления и улучшения.';
      }

      if (statusTitle) statusTitle.textContent = 'Доступно обновление!';
      if (statusDesc) statusDesc.textContent = `Доступна новая версия ${data.latest_version}. Обновите приложение в один клик.`;

      if (manual) {
        toast(`Доступно обновление SWCH: v${data.latest_version}!`);
      }
    } else {
      if (indicator) indicator.hidden = true;
      if (panel) panel.style.display = 'none';
      if (card) card.classList.remove('has-update');

      if (statusTitle) statusTitle.textContent = 'Установлена актуальная версия';
      if (statusDesc) statusDesc.textContent = `У вас установлена последняя версия SWCH (v${data.current_version}).`;

      if (manual) {
        toast(`У вас установлена актуальная версия SWCH (v${data.current_version})`);
      }
    }
  } catch (err) {
    if (manual) {
      toast(err.message || 'Не удалось проверить обновления');
      if (statusDesc) statusDesc.textContent = 'Ошибка проверки: ' + err.message;
    }
  } finally {
    if (manual && btn) {
      btn.disabled = false;
      btn.innerHTML = `${icon('refresh')}<span>Проверить</span>`;
      hydrateIcons(btn);
    }
  }
}

async function applyUpdate() {
  const btn = $('btn-apply-update');
  const progressPanel = $('update-progress-panel');
  const availablePanel = $('update-available-panel');

  if (btn) btn.disabled = true;
  if (availablePanel) availablePanel.style.display = 'none';
  if (progressPanel) progressPanel.style.display = 'flex';
  renderUpdateStatus({phase:'checking',message:'Проверка обновления перед загрузкой…'});

  try {
    const res = await fetch('/api/update/apply', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ force: true })
    });
    const result = await res.json();
    if (!res.ok) {
      throw new Error(result.error || 'Не удалось запустить обновление');
    }

    toast('Загрузка обновления началась...');
    startUpdateStatusPolling();
  } catch (err) {
    toast('Ошибка: ' + err.message);
    renderUpdateStatus({phase:'error',message:'Ошибка: ' + err.message});
    if (btn) btn.disabled = false;
    if (availablePanel) availablePanel.style.display = 'flex';
  }
}

function renderUpdateStatus(status) {
  const bar = $('update-progress-bar'), track = $('update-progress-track');
  const percent = $('update-progress-percent'), label = $('update-progress-label');
  const details = $('update-progress-details');
  const downloading = status.phase === 'downloading';
  const known = downloading && Number.isFinite(status.download_percent);
  const processing = ['extracting','installing','restarting'].includes(status.phase);
  const value = Math.max(0, Math.min(100, Number(status.download_percent) || 0));
  if (bar) bar.style.width = `${processing ? 100 : value}%`;
  if (track) {
    track.classList.toggle('indeterminate', status.phase === 'checking' || (downloading && !known));
    track.classList.toggle('failed', status.phase === 'error');
    track.setAttribute('aria-label', status.message || 'Обновление SWCH');
    if (known) track.setAttribute('aria-valuenow', String(value));
    else track.removeAttribute('aria-valuenow');
  }
  const stage = {checking:'Проверка',extracting:'Распаковка',installing:'Установка',restarting:'Перезапуск',error:'Ошибка'};
  if (percent) percent.textContent = known ? `${value.toLocaleString('ru-RU',{maximumFractionDigits:1})}%` : (stage[status.phase] || 'Загрузка');
  if (label) label.textContent = status.message || 'Загрузка обновления…';
  if (details) {
    const parts = [];
    const bytes = n => {
      if (n < 1024) return `${n} Б`;
      const unit = n < 1024 ** 2 ? 1 : n < 1024 ** 3 ? 2 : 3;
      return `${(n / 1024 ** unit).toLocaleString('ru-RU',{maximumFractionDigits:1})} ${['Б','КиБ','МиБ','ГиБ'][unit]}`;
    };
    if (Number.isFinite(status.downloaded_bytes)) {
      parts.push(status.total_bytes > 0 ? `${bytes(status.downloaded_bytes)} из ${bytes(status.total_bytes)}` : `${bytes(status.downloaded_bytes)} скачано`);
      if (downloading && !status.total_bytes) parts.push('Размер файла не сообщён сервером');
    }
    if (downloading && status.bytes_per_second > 0) parts.push(`${bytes(status.bytes_per_second)}/с`);
    if (downloading && status.eta_seconds > 0) {
      const seconds = Math.ceil(status.eta_seconds);
      parts.push(seconds < 60 ? `Осталось около ${seconds} с` : `Осталось около ${Math.ceil(seconds / 60)} мин`);
    }
    details.textContent = parts.join(' · ');
  }
}

function startUpdateStatusPolling() {
  if (updatePollingActive) return;
  updatePollingActive = true;
  let failures = 0;
  const poll = async () => {
    try {
      const res = await fetch('/api/update/status', {cache:'no-store',signal:AbortSignal.timeout(5000)});
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const status = await res.json();
      failures = 0;
      renderUpdateStatus(status);

      if (status.phase === 'restarting') {
        updatePollingActive = false;
        toast('Обновление установлено! Перезапуск SWCH...');
        pollForServerRestart();
      } else if (status.phase === 'error') {
        updatePollingActive = false;
        toast('Ошибка обновления: ' + (status.error || status.message));
        if ($('btn-apply-update')) $('btn-apply-update').disabled = false;
        if ($('update-available-panel')) $('update-available-panel').style.display = 'flex';
      } else if (status.phase === 'idle') {
        updatePollingActive = false;
        renderUpdateStatus({phase:'error',message:'Обновление прервано. Повторите загрузку.'});
        if ($('btn-apply-update')) $('btn-apply-update').disabled = false;
        if ($('update-available-panel')) $('update-available-panel').style.display = 'flex';
      }
    } catch {
      if (++failures >= 2 && $('update-progress-label')) {
        $('update-progress-label').textContent = 'Нет связи с SWCH. Ожидание восстановления соединения…';
        if ($('update-progress-details')) $('update-progress-details').textContent = 'Текущее состояние загрузки недоступно';
      }
    }
    if (updatePollingActive) setTimeout(poll, 500);
  };
  void poll();
}

async function resumeUpdateStatus() {
  try {
    const res = await fetch('/api/update/status', {cache:'no-store',signal:AbortSignal.timeout(5000)});
    if (!res.ok) return;
    const status = await res.json();
    if (updatePollingActive || !['downloading','extracting','installing','restarting'].includes(status.phase)) return;
    if ($('update-progress-panel')) $('update-progress-panel').style.display = 'flex';
    if ($('update-available-panel')) $('update-available-panel').style.display = 'none';
    if ($('btn-apply-update')) $('btn-apply-update').disabled = true;
    renderUpdateStatus(status);
    startUpdateStatusPolling();
  } catch { /* A later update attempt can reconnect. */ }
}

function pollForServerRestart() {
  let attempts = 0;
  const restartTimer = setInterval(async () => {
    attempts++;
    try {
      const res = await fetch('/api/health');
      if (res.ok) {
        clearInterval(restartTimer);
        toast('SWCH успешно обновлен!');
        setTimeout(() => location.reload(), 500);
      }
    } catch {
      if (attempts > 30) {
        clearInterval(restartTimer);
        toast('Перезапустите страницу вручную, когда сервер поднимется.');
      }
    }
  }, 1000);
}

$('btn-check-update')?.addEventListener('click', () => checkForUpdates(true));
$('btn-apply-update')?.addEventListener('click', () => applyUpdate());
if (!demo) void resumeUpdateStatus();


document.addEventListener("swch-language-change",()=>{applyStyle();render();});
