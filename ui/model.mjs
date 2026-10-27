import {getLocale} from './i18n.js';
export const DESIGNS = {
  apple: {
    id: 'apple',
    name: 'Apple (macOS)',
    macOnly: true,
    description: 'Нативный дизайн в стиле Apple HIG (Vibrancy, SF Pro, системные панели macOS)',
  },
  standard: {
    id: 'standard',
    name: 'Классический SWCH',
    macOnly: false,
    description: 'Фирменный игровой интерфейс SWCH',
  },
};
export function isMacPlatform(env = null) {
  const g = env || (typeof globalThis !== 'undefined' ? globalThis : {});
  const search = typeof g.location?.search === 'string' ? g.location.search : '';
  if (search) {
    try {
      const params = new URLSearchParams(search);
      const macParam = params.get('mac') ?? params.get('platform');
      if (macParam === '1' || macParam === 'true' || macParam === 'mac' || macParam === 'macos') return true;
      if (macParam === '0' || macParam === 'false' || macParam === 'windows' || macParam === 'linux') return false;
    } catch { /* ignore */ }
  }
  if (g.__SWCH_PLATFORM === 'macos' || g.__SWCH_PLATFORM === 'mac') return true;
  if (g.__SWCH_PLATFORM === 'windows' || g.__SWCH_PLATFORM === 'linux') return false;
  try {
    const forced = g.localStorage?.getItem?.('swch-force-mac');
    if (forced === '1' || forced === 'true') return true;
    if (forced === '0' || forced === 'false') return false;
  } catch { /* ignore */ }
  const nav = g.navigator || {};
  const uaPlatform = String(nav.userAgentData?.platform || nav.platform || '');
  const ua = String(nav.userAgent || '');
  return /^Mac/i.test(uaPlatform) || /Macintosh|Mac OS X/i.test(ua);
}
export function resolveDesign(savedDesign, isMac = isMacPlatform()) {
  if (!isMac) return 'standard';
  if (savedDesign && Object.hasOwn(DESIGNS, savedDesign)) return savedDesign;
  return 'apple';
}
export const STYLES = {
  gog: { name: 'GOG GALAXY', layout: 'cards', color: '#c68eff', description: 'Библиотека GOG и аккаунты' },
  ubisoft: { name: 'Ubisoft Connect', layout: 'cards', color: '#65b7ff', description: 'Установленные игры и сохранённые сессии Ubisoft Connect' },
  epic: { name: 'Epic Games', layout: 'cards', color: '#ff784f', description: 'Крупные обложки и витрина игр' },
  xbox: { name: 'Xbox', layout: 'cards', color: '#9ada70', description: 'Компактные плитки, зелёный акцент' },
  playstation: { name: 'PlayStation', layout: 'cards', color: '#74acff', description: 'Высокие карточки, синий акцент' },
  steam: { name: 'Steam', layout: 'detail', color: '#85c8e9', description: 'Список игр и подробная статистика' },
  riot: { name: 'Riot Games', layout: 'detail', color: '#ff6878', description: 'Фокус на игре и её аккаунтах' },
  roblox: { name: 'Roblox', layout: 'cards', color: '#e2231a', description: 'Мульти-аккаунты, скины и плейсы' },
  minecraft: { name: 'Minecraft', layout: 'cards', color: '#2e7d32', description: 'Официальный лаунчер, аккаунты, профили и сервера' },
};
export const gameKey = game => game.launcher?.startsWith('mod:') ? game.id : `${game.launcher}:${game.app_id}`;
export const metricKey = (game, account) => JSON.stringify([game.launcher, String(game.app_id), String(account.id)]);
export const IGNORED_STEAM_APP_IDS = new Set(['7', '760', '2371090']);
export const IGNORED_STEAM_APP_NAMES = new Set([
  'steam client',
  'steam screenshots',
  'steam game notes',
]);
export function isIgnoredSteamApp(g) {
  if (!g) return false;
  if (typeof g === 'string') {
    const s = g.trim().replace(/^steam_/, '');
    if (IGNORED_STEAM_APP_IDS.has(s)) return true;
    if (IGNORED_STEAM_APP_NAMES.has(s.toLowerCase())) return true;
    return false;
  }
  const appId = String(g.app_id ?? g.id ?? '').trim().replace(/^steam_/, '');
  if (IGNORED_STEAM_APP_IDS.has(appId)) return true;
  const name = String(g.name || '').trim().toLowerCase();
  if (IGNORED_STEAM_APP_NAMES.has(name)) return true;
  return false;
}
export function filterGames(games, {platform = 'all', installed = 'all', query = '', favorites = null, sort = 'default', recent = {}, favoriteKeys = [], playtime = {}} = {}) {
  const q = query.trim().toLocaleLowerCase('ru');
  return games.filter(g => !isIgnoredSteamApp(g) && (platform === 'all' || g.launcher === platform) && (installed !== 'installed' || g.is_installed) && (!favorites || favorites.includes(gameKey(g))) && (!q || `${g.name} ${g.app_id}`.toLocaleLowerCase('ru').includes(q)))
    .sort((a,b) => {
      const minutes = g => Math.max(0, Number(playtime[gameKey(g)]) || 0);
      const favoriteOrder = Number(favoriteKeys.includes(gameKey(b))) - Number(favoriteKeys.includes(gameKey(a)));
      if (favoriteOrder) return favoriteOrder;
      if (sort === 'default') return Number(Boolean(b.is_installed)) - Number(Boolean(a.is_installed)) || minutes(b) - minutes(a) || a.name.localeCompare(b.name, getLocale());
      return (sort === 'hours' ? minutes(b) - minutes(a) : 0) || (sort === 'recent' ? (recent[gameKey(b)] || 0) - (recent[gameKey(a)] || 0) : 0) || a.name.localeCompare(b.name, getLocale());
    });
}
export function normalizeEpic(games, accountId) {
  return games.map(g => ({...g, launcher: 'epic', app_id: g.namespace==='fn'&&g.title?.trim().toLowerCase()==='fortnite'?'Fortnite':g.app_name, name: g.title, owner_id: accountId}));
}
export function normalizeMetrics(values) {
  return Object.fromEntries(['skins','value','hours'].map(key => {
    const raw = values[key];
    const n = raw === '' || raw == null ? null : Number(raw);
    if (n !== null && (!Number.isFinite(n) || n < 0 || n > 1e9 || (key === 'skins' && !Number.isInteger(n)))) throw new Error('Введи корректные неотрицательные значения. Количество скинов должно быть целым.');
    return [key,n];
  }));
}
export function safeImage(url) {
  if (typeof url !== 'string' || !url.trim()) return '';
  if (url.startsWith('data:image/png;base64,') || url.startsWith('data:image/jpeg;base64,') || url.startsWith('data:image/webp;base64,')) {
    return url;
  }
  if (url.startsWith('/') || url.startsWith('./') || url.startsWith('ui/')) {
    return url;
  }
  try { const parsed = new URL(url); return ['http:', 'https:'].includes(parsed.protocol) ? parsed.href : ''; } catch { return ''; }
}

export function mergeLibraries(libraries) {
  const merged = new Map();
  for (const game of libraries.flat()) {
    const key = gameKey(game), existing = merged.get(key);
    if (existing) {
      if (!existing.owner_ids.includes(game.owner_id)) existing.owner_ids.push(game.owner_id);
      const banner = game.banner_url || existing.banner_url;
      const hero = game.hero_url || existing.hero_url;
      if (game.is_installed) Object.assign(existing, {...game, owner_ids: existing.owner_ids});
      if (banner) existing.banner_url = banner;
      if (hero) existing.hero_url = hero;
    } else merged.set(key, {...game, owner_ids: [game.owner_id]});
  }
  return [...merged.values()];
}

export function normalizeSteam(rows) {
  const merged = new Map();
  for (const row of rows) {
    if (isIgnoredSteamApp(row)) continue;
    const old = merged.get(String(row.app_id));
    if (!old) merged.set(String(row.app_id), {...row, steam_accounts: row.account_id ? [row.account_id] : []});
    else {
      if (row.account_id && !old.steam_accounts.includes(row.account_id)) old.steam_accounts.push(row.account_id);
      if (old.name.startsWith('Steam #') && !row.name.startsWith('Steam #')) old.name = row.name;
      if(row.is_free != null)old.is_free=row.is_free;
      if(row.has_inventory != null)old.has_inventory=row.has_inventory;
      if (row.update) old.update = row.update;
      if (row.is_installed) { old.is_installed = true; old.install_path = row.install_path; old.size_bytes = row.size_bytes; }
    }
  }
  return [...merged.values()];
}
export function steamStats(rows, appId, accountId) {
  return rows.find(g => String(g.app_id) === String(appId) && g.account_id === accountId);
}
export function inventoryKey(accountId, appId, contextId = '2') { return JSON.stringify([accountId, String(appId), String(contextId)]); }
export function appendInventory(previous, page) {
  const items = new Map((previous?.items || []).map(i => [i.asset_id,i]));
  for (const item of page.items) items.set(item.asset_id, item);
  return {...page, items:[...items.values()],
    cached: Boolean(previous?.cached || page.cached),
    updated_at: previous?.updated_at && previous.updated_at < page.updated_at ? previous.updated_at : page.updated_at,
    warning: [...new Set([previous?.warning,page.warning].filter(Boolean))].join(' ') || null,
  };
}

// A played game belongs in this account's view even if current ownership is unknown.
// Free-to-play status alone does not establish an account-game association.
export function hasPlayed(row) { return Number.isFinite(row?.playtime_minutes) && row.playtime_minutes > 0; }
export function accountPlayedGames(account, rows) {
  const found=new Map();
  for(const row of rows)if(row.account_id===account.id && (!row.launcher||row.launcher===account.launcher) && !isIgnoredSteamApp(row) && hasPlayed(row)) {
    const key=String(row.app_id),old=found.get(key);
    if(!old||row.playtime_minutes>old.playtime_minutes)found.set(key,row);
  }
  return [...found.values()];
}
export function eligibleSteamAccounts(game, accounts, rows) {
  if (isIgnoredSteamApp(game)) return [];
  return accounts.filter(a => a.launcher === 'steam' && rows.some(r => !isIgnoredSteamApp(r) && String(r.app_id) === String(game.app_id) && r.account_id === a.id && (hasPlayed(r) || ['owned','family'].includes(r.access))));
}
export function aggregateSteamStats(rows, accountIds, appId = null) {
  const map = new Map();
  for(const row of rows) {
    if (isIgnoredSteamApp(row)) continue;
    if(accountIds.includes(row.account_id) && (appId == null || String(row.app_id) === String(appId))) map.set(JSON.stringify([row.account_id,String(row.app_id)]),row);
  }
  const values = [...map.values()];
  const total = key => {const known=values.filter(r=>Number.isFinite(r[key]));return known.length ? known.reduce((n,r)=>n+r[key],0) : null;};
  return {minutes:total('playtime_minutes'),recent:total('playtime_2weeks'),known_accounts:new Set(values.filter(r=>r.playtime_minutes!=null).map(r=>r.account_id)).size,total_accounts:accountIds.length};
}
export function inventoryIdentity(item) { return JSON.stringify([item.account_id || '',item.app_id || '',item.context_id || '',item.asset_id]); }
export function inventoryHeroItems(items, facets = {}) {
  const heroes = Object.fromEntries(Object.entries(facets).filter(([key]) => key.toLowerCase() === '570:hero'));
  return filterInventory(items, {facets: heroes});
}
export function inventoryTagCounts(items) {
  const heroNames = new Set(items.flatMap(i => (i.tag_details || []).filter(t => t.category?.toLowerCase() === 'hero').map(t => t.name)));
  const counts = new Map();
  for (const item of items) for (const tag of new Set(item.tags || [])) {
    if (!heroNames.has(tag)) counts.set(tag, (counts.get(tag) || 0) + (item.amount || 1));
  }
  return counts;
}
export function csInventoryCategories(items) {
  const groups = new Map();
  for (const item of items) for (const tag of item.tag_details || []) {
    if (String(item.app_id) !== '730' || tag.category?.toLowerCase() !== 'type') continue;
    const key = `${item.app_id}:${tag.category}`;
    const id = JSON.stringify([key, tag.value]);
    const group = groups.get(id) || {key, value: tag.value, name: tag.name, count: 0, image: ''};
    group.count += item.amount || 1;
    group.image ||= safeImage(item.icon_url);
    groups.set(id, group);
  }
  const weaponTypes = new Set(['rifle','sniperrifle','pistol','smg','shotgun','machinegun','knife','knifegg','weapon','grenade','taser']);
  const isWeapon = category => weaponTypes.has(String(category.value).toLowerCase().replace(/^csgo_type_/, ''));
  return [...groups.values()].sort((a,b) => Number(isWeapon(b)) - Number(isWeapon(a)) || a.name.localeCompare(b.name, getLocale()));
}
export function filterInventory(items, {query='',tag='',availability='all',market='all',facets={},sort='original',quotes=new Map(),minPrice='',maxPrice='',exchangeRate=1,restrictionsMap=null}={}) {
  const q=query.trim().toLocaleLowerCase('ru');
  const price=i=>{const v=quotes.get(priceKey(i.app_id,i.market_hash_name));return v?.currency==='USD'&&v.unit_price_cents!=null?v.unit_price_cents:null;};
  const result=items.filter(i=>(!q||`${i.name} ${i.item_type} ${(i.tags||[]).join(' ')}`.toLocaleLowerCase('ru').includes(q))
    && (!tag||(i.tags||[]).includes(tag))
    && (availability==='all'||availability==='tradable'&&i.tradable||availability==='marketable'&&i.marketable||availability==='locked'&&!i.tradable||availability==='banned'&&Boolean(restrictionsMap&&isItemUnsellable(i,restrictionsMap)))
    && (market==='all'||market==='yes'&&i.marketable||market==='no'&&!i.marketable||market==='banned'&&Boolean(restrictionsMap&&isItemUnsellable(i,restrictionsMap)))
    && Object.entries(facets).every(([key,values])=>!values.length||(i.tag_details||[]).some(t=>`${i.app_id}:${t.category}`===key&&values.includes(t.value)))
    && (minPrice===''||(price(i)!=null&&Number.isFinite(exchangeRate)&&exchangeRate>0&&price(i)*exchangeRate>=Number(minPrice)*100))
    && (maxPrice===''||(price(i)!=null&&Number.isFinite(exchangeRate)&&exchangeRate>0&&price(i)*exchangeRate<=Number(maxPrice)*100)));
  const compare=(a,b)=>a.name.localeCompare(b.name,getLocale());
  if(sort==='name')result.sort(compare);
  if(sort==='amount')result.sort((a,b)=>b.amount-a.amount||compare(a,b));
  if(sort==='newest')result.sort((a,b)=>{try {const x=BigInt(a.asset_id),y=BigInt(b.asset_id);return x===y?0:x>y?-1:1;}catch{return 0;}});
  if(sort==='price_desc'||sort==='price_asc')result.sort((a,b)=>{const x=price(a),y=price(b);return x==null?(y==null?compare(a,b):1):y==null?-1:(sort==='price_desc'?y-x:x-y)||compare(a,b);});
  return result;
}
export const priceKey = (appId,name) => JSON.stringify([String(appId),name]);
export function inventoryValue(items, quotes, appId, restrictionsMap = null, options = {}) {
  let rMap = restrictionsMap;
  let opts = options;
  if (restrictionsMap && typeof restrictionsMap === 'object' && !(restrictionsMap instanceof Map) && !restrictionsMap.account_id && !restrictionsMap.community_banned && ('excludeUnsellable' in restrictionsMap || 'restrictionsMap' in restrictionsMap)) {
    opts = restrictionsMap;
    rMap = opts.restrictionsMap || null;
  }
  const excludeUnsellable = opts.excludeUnsellable ?? true;

  let cents = 0, priced = 0, unpriced = 0, not_marketable = 0, unsellable_banned = 0;
  for (const i of items) {
    const amount = Number.isSafeInteger(i.amount) && i.amount > 0 ? i.amount : 1;
    if (!i.marketable) {
      not_marketable += amount;
      continue;
    }
    const ban = rMap ? isItemUnsellable(i, rMap, { appId, ...opts }) : null;
    if (ban && excludeUnsellable) {
      unsellable_banned += amount;
      continue;
    }
    const quote = quotes?.get?.(priceKey(i.app_id ?? appId, i.market_hash_name));
    if (quote?.currency === 'USD' && Number.isSafeInteger(quote.unit_price_cents) && quote.unit_price_cents >= 0) {
      cents += quote.unit_price_cents * amount;
      priced += amount;
    } else {
      unpriced += amount;
    }
  }
  return { cents: Number.isSafeInteger(cents) ? cents : null, priced, unpriced, not_marketable, unsellable_banned };
}

export const CURRENCIES = {USD:'Доллар США',EUR:'Евро',RUB:'Российский рубль',KZT:'Тенге',UAH:'Гривна',BRL:'Бразильский реал',PLN:'Злотый',GBP:'Фунт стерлингов',CNY:'Юань'};
export function convertUsdCents(cents,currency,rates={USD:1}) {
  const rate=currency==='USD'?1:rates?.[currency];
  if(!Object.hasOwn(CURRENCIES,currency)||!Number.isSafeInteger(cents)||cents<0||!Number.isFinite(rate)||rate<=0)return null;
  const converted=Math.round(cents*rate);
  return Number.isSafeInteger(converted)?converted:null;
}
export function accountLibraryValue(rows,accountId,quotes,targetCurrency, options = {}) {
  const treatUnpricedAsFree = options.treatUnpricedAsFree ?? false;
  const unique=new Map(),priority={owned:3,family:2,local:1,history:0};
  for(const row of rows.filter(r=>r.account_id===accountId && !isIgnoredSteamApp(r))){
    const id=String(row.app_id),old=unique.get(id);
    if(!old||(priority[row.access]??-1)>(priority[old.access]??-1))unique.set(id,row);
  }
  const hasExplicitOwned = [...unique.values()].some(r => r.access === 'owned');
  const empty=()=>({cents:0,known:0,unknown:0,free:0,total:0,items:[]});
  const result={owned:empty(),family:empty(),unconfirmed:0};
  for(const row of unique.values()){
    let access = row.access;
    if(access === 'local' && !hasExplicitOwned){
      access = 'owned';
    }
    if(!['owned','family'].includes(access)){result.unconfirmed++;continue;}
    const group=result[access];
    const appIdStr = String(row.app_id);
    const quote=(targetCurrency&&quotes.get(`${targetCurrency}:${appIdStr}`))||quotes.get(appIdStr);
    const expCurrency = targetCurrency || 'USD';
    const isExplicitFree = (quote && (quote.is_free === true || quote.price_cents === 0)) || row.is_free === true;
    const hasKnownPrice = quote && quote.currency === expCurrency && Number.isSafeInteger(quote.price_cents) && quote.price_cents >= 0;
    let cents = null;
    let isFree = false;
    if (isExplicitFree) {
      cents = 0;
      isFree = true;
    } else if (hasKnownPrice) {
      cents = quote.price_cents;
      isFree = quote.is_free === true || cents === 0;
    } else if (treatUnpricedAsFree) {
      cents = 0;
      isFree = true;
    }
    group.total++;group.items.push({row,quote,cents,is_free:isFree});
    if(cents==null)group.unknown++;else{group.known++;group.cents+=cents;if(isFree)group.free++;}
  }
  for(const group of [result.owned,result.family])if(!Number.isSafeInteger(group.cents))group.cents=null;
  result.total_cents=result.owned.cents==null||result.family.cents==null?null:result.owned.cents+result.family.cents;
  if(!Number.isSafeInteger(result.total_cents))result.total_cents=null;
  return result;
}

export function getAccountRestrictions(account, restrictionsMap = {}) {
  const accId = typeof account === 'string' ? account : (account?.id || account?.account_id);
  const username = typeof account === 'object' ? account?.username : null;
  const displayName = typeof account === 'object' ? account?.display_name : null;

  if (restrictionsMap instanceof Map) {
    return restrictionsMap.get(accId) ||
      (username && restrictionsMap.get(username)) ||
      (displayName && restrictionsMap.get(displayName)) ||
      account?.restrictions ||
      null;
  }
  return (restrictionsMap && (restrictionsMap[accId] || (username && restrictionsMap[username]) || (displayName && restrictionsMap[displayName]))) ||
    account?.restrictions ||
    null;
}

function normalizedBanGameName(value) {
  return String(value || '').normalize('NFKC').trim().replace(/\s+/g, ' ').toLocaleLowerCase('ru');
}

function banMatchesGame(ban, appId, gameName) {
  const banId = ban.app_id != null ? String(ban.app_id).trim() : '';
  if (appId && banId) return appId === banId;
  const banName = normalizedBanGameName(ban.name);
  return Boolean(gameName && banName && gameName === banName);
}

export function accountHasGameBan(account, appId, gameName = '', restrictionsMap = {}) {
  const r = getAccountRestrictions(account, restrictionsMap);
  if (!r) return false;
  const targetId = appId != null ? String(appId).trim() : '';
  const targetName = normalizedBanGameName(gameName);

  const bans = Array.isArray(r.game_bans) ? r.game_bans : [];
  return bans.some(b => banMatchesGame(b, targetId, targetName));
}

export function isItemUnsellable(item, restrictionsMap = {}, options = {}) {
  if (!item || !restrictionsMap) return null;
  const acc = item.account_id || options.accountId;
  const r = getAccountRestrictions(acc, restrictionsMap);
  if (!r) return null;

  if (r.community_banned) {
    return { type: 'community', label: 'КТ (Блокировка сообщества)', reason: 'Блокировка сообщества Steam (КТ)' };
  }
  if (r.market_banned) {
    return { type: 'market', label: 'Запрет ТП', reason: 'Запрет Торговой площадки Steam' };
  }
  if (r.trade_banned || (r.economy_ban && r.economy_ban !== 'none')) {
    return { type: 'trade', label: 'Трейд-бан', reason: 'Блокировка обмена Steam' };
  }

  const targetAppId = (item.app_id ?? options.appId) != null ? String(item.app_id ?? options.appId).trim() : '';
  const gameName = normalizedBanGameName(item.game_name || options.gameName);
  const bans = Array.isArray(r.game_bans) ? r.game_bans : [];

  const matchingBan = bans.find(b => banMatchesGame(b, targetAppId, gameName));

  if (matchingBan) {
    const isVac = String(matchingBan.ban_type || '').trim().toUpperCase() === 'VAC';
    return {
      type: isVac ? 'vac' : 'game_ban',
      label: isVac ? 'VAC-бан в игре' : 'Бан в игре',
      name: matchingBan.name,
      reason: isVac ? `VAC-бан в ${matchingBan.name || 'этой игре'}` : `Игровая блокировка в ${matchingBan.name || 'этой игре'}`
    };
  }

  return null;
}

export function accountRestrictionsSummary(account, restrictionsMap = {}) {
  const r = getAccountRestrictions(account, restrictionsMap);
  if (!r) {
    return {
      has_restrictions: false,
      market_banned: false,
      trade_banned: false,
      vac_banned: false,
      vac_bans_count: 0,
      community_banned: false,
      is_limited: false,
      game_bans: [],
      game_bans_count: 0,
      badges: [{ type: 'unknown', label: 'Нет данных о блокировках' }],
    };
  }

  const market_banned = Boolean(r.market_banned || r.is_limited);
  const trade_banned = Boolean(r.trade_banned || (r.economy_ban && r.economy_ban !== 'none'));
  const vac_banned = Boolean(r.vac_banned || (r.vac_bans_count && r.vac_bans_count > 0));
  const community_banned = Boolean(r.community_banned);
  const is_limited = Boolean(r.is_limited);
  const vac_bans_count = r.vac_bans_count || (vac_banned ? 1 : 0);
  const game_bans = Array.isArray(r.game_bans) ? r.game_bans : [];
  const game_bans_count = Math.max(game_bans.length, r.game_bans_count || 0);

  const badges = [];
  if (market_banned) badges.push({ type: 'market', label: 'Запрет ТП' });
  if (trade_banned) badges.push({ type: 'trade', label: 'Запрет обменов' });
  if (vac_banned) {
    badges.push({ type: 'vac', label: vac_bans_count > 1 ? `VAC-баны (${vac_bans_count})` : 'VAC-бан' });
  }
  for (const b of game_bans) {
    badges.push({ type: 'game_ban', label: `Бан в игре: ${b.name}`, app_id: b.app_id, name: b.name });
  }
  if (game_bans_count > game_bans.length) {
    const unknown = game_bans_count - game_bans.length;
    badges.push({ type: 'game_ban_count', label: unknown > 1 ? `Игры не указаны (${unknown})` : 'Игра не указана' });
  }
  if (community_banned) badges.push({ type: 'community', label: 'Блокировка сообщества' });
  if (is_limited) badges.push({ type: 'limited', label: 'Ограниченный ($5)' });

  const has_restrictions = badges.length > 0;
  if (!has_restrictions) {
    badges.push({ type: 'clean', label: 'Бан не найден в доступных данных' });
  }

  return {
    has_restrictions,
    market_banned,
    trade_banned,
    vac_banned,
    vac_bans_count,
    community_banned,
    is_limited,
    game_bans,
    game_bans_count,
    badges,
  };
}

export const KNOWN_STEAM_APP_NAMES = {
  '730': 'Counter-Strike 2',
  '570': 'Dota 2',
  '440': 'Team Fortress 2',
  '252490': 'Rust',
  '753': 'Коллекционные карточки',
  '578080': 'PUBG: BATTLEGROUNDS',
  '322330': "Don't Starve Together",
  '2923300': 'Banana',
  '3231090': "Amarillo's Butt Slapper",
  '218620': 'PAYDAY 2',
  '304930': 'Unturned',
};

export const KNOWN_GAME_SHORTS = {
  'all': 'Все',
  '730': 'CS2',
  '570': 'Dota',
  '440': 'TF2',
  '252490': 'Rust',
  '753': 'Steam',
  '578080': 'PUBG',
  '322330': 'DST',
  '2923300': 'Banana',
  '3231090': 'ABS',
  '218620': 'PD2',
  '304930': 'Unturned',
};

export function getDota2HeroIconUrl(heroValue) {
  if (!heroValue || typeof heroValue !== 'string') return null;
  if (heroValue === 'DOTA_OtherType' || heroValue === 'none' || heroValue === 'all') return null;
  const slug = heroValue.replace(/^npc_dota_hero_/, '').replace(/_persona\d*$/, '');
  if (!slug) return null;
  return `https://cdn.cloudflare.steamstatic.com/apps/dota2/images/dota_react/heroes/icons/${slug}.png`;
}

export const DOTA2_LOGO_SVG = `<svg class="dota2-icon-svg" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="#e23b24" d="M 9.817 23.607 L 9.471 23.313 L 9.003 23.273 C 8.419 23.225 8.147 23.172 7.817 23.041 L 7.559 22.938 L 7.281 23.048 C 7.045 23.143 6.467 23.229 6.028 23.236 C 6.004 23.236 5.964 23.18 5.939 23.111 C 5.911 23.041 5.862 22.974 5.824 22.961 C 5.789 22.949 5.527 23.007 5.237 23.094 L 4.712 23.252 L 4.523 23.161 C 4.285 23.045 4.185 23.047 3.787 23.168 C 3.484 23.262 3.446 23.264 3.203 23.212 C 2.84 23.136 2.537 23.141 1.854 23.238 C 1.11 23.344 1.094 23.344 0.875 23.194 C 0.728 23.094 0.648 23.069 0.469 23.069 C 0.266 23.069 0.244 23.06 0.221 22.965 C 0.206 22.906 0.225 22.787 0.264 22.688 C 0.376 22.41 0.42 21.736 0.432 20.125 L 0.444 18.62 L 0.655 18.635 L 0.863 18.65 L 0.895 17.78 C 0.911 17.302 0.927 16.611 0.93 16.246 L 0.935 15.586 L 0.823 15.506 C 0.518 15.283 0.532 15.315 0.405 14.588 C 0.339 14.211 0.274 13.658 0.259 13.358 C 0.234 12.826 0.236 12.811 0.334 12.7 L 0.434 12.586 L 0.682 12.799 C 0.82 12.916 0.937 12.993 0.948 12.969 C 0.984 12.856 0.871 11.508 0.813 11.338 C 0.775 11.238 0.747 11.101 0.747 11.03 C 0.747 10.962 0.679 10.778 0.594 10.618 L 0.443 10.329 L 0.494 9.816 C 0.561 9.136 0.475 8.636 0.19 8.034 L 0.065 7.763 L 0.144 7.09 L 0.228 6.416 L 0.14 6.067 L 0.049 5.717 L 0.154 5.326 L 0.255 4.933 L 0.14 4.553 L 0.021 4.174 L 0.164 3.774 C 0.305 3.385 0.308 3.364 0.305 2.73 C 0.303 2.016 0.228 1.551 0.08 1.365 C -0.047 1.203 -0.026 1.092 0.163 0.919 C 0.253 0.834 0.328 0.726 0.328 0.681 C 0.328 0.612 0.359 0.599 0.549 0.595 C 0.669 0.595 1.039 0.563 1.372 0.525 L 1.979 0.452 L 2.773 0.569 C 3.565 0.684 3.566 0.684 4.652 0.638 L 5.74 0.591 L 5.997 0.746 L 6.257 0.899 L 7.312 0.884 L 8.368 0.869 L 8.724 0.74 C 8.918 0.669 9.229 0.545 9.414 0.461 C 9.596 0.377 9.927 0.264 10.144 0.204 L 10.542 0.099 L 11.056 0.261 L 11.57 0.425 L 12.014 0.276 L 12.456 0.127 L 12.933 0.285 C 13.196 0.371 13.684 0.583 14.022 0.757 L 14.633 1.069 L 15.273 1.051 C 15.758 1.036 15.924 1.018 15.95 0.973 C 15.97 0.942 16.031 0.758 16.083 0.566 L 16.178 0.215 L 16.783 0.235 C 17.236 0.251 17.642 0.299 18.384 0.429 L 19.379 0.605 L 19.638 0.501 C 19.778 0.444 20.046 0.375 20.233 0.351 C 20.546 0.309 20.6 0.314 21.027 0.422 C 21.425 0.522 21.562 0.537 22.109 0.537 C 22.454 0.537 22.812 0.522 22.907 0.506 C 23.07 0.478 23.097 0.489 23.428 0.728 C 23.621 0.866 23.792 1.001 23.807 1.03 C 23.824 1.057 23.86 1.203 23.884 1.354 L 23.93 1.632 L 23.721 2.028 C 23.604 2.246 23.507 2.443 23.507 2.466 C 23.507 2.489 23.555 2.527 23.611 2.554 C 23.668 2.579 23.764 2.677 23.826 2.771 L 23.937 2.941 L 23.912 5.226 C 23.897 6.483 23.872 7.604 23.858 7.715 L 23.831 7.919 L 23.486 7.897 L 23.142 7.87 L 23.113 8.061 C 23.097 8.164 23.084 8.353 23.084 8.485 C 23.084 8.677 23.054 8.778 22.924 9.031 L 22.761 9.342 L 22.851 9.471 C 22.901 9.543 23.072 9.736 23.231 9.901 L 23.523 10.206 L 23.434 10.529 L 23.344 10.854 L 23.451 11.542 C 23.575 12.369 23.566 12.577 23.363 13.293 C 23.282 13.58 23.217 13.846 23.217 13.886 C 23.217 13.961 23.272 13.98 23.644 14.031 C 23.809 14.052 23.844 14.072 23.844 14.147 C 23.844 14.196 23.87 15.251 23.904 16.492 C 24.011 20.567 24.021 21.456 23.971 22.38 C 23.944 22.862 23.913 23.262 23.904 23.271 C 23.894 23.28 23.563 23.147 23.166 22.976 L 22.443 22.668 L 22.131 22.803 C 21.809 22.942 21.777 22.978 21.735 23.245 C 21.706 23.422 21.728 23.414 20.935 23.576 L 20.468 23.672 L 20.014 23.52 C 19.594 23.379 19.537 23.369 19.206 23.391 C 18.959 23.406 18.789 23.394 18.644 23.351 C 18.436 23.292 18.436 23.292 18.087 23.453 L 17.735 23.612 L 16.435 23.612 C 15.066 23.61 14.784 23.632 13.841 23.807 C 13.189 23.927 12.96 23.922 11.819 23.785 L 11.217 23.711 L 10.796 23.807 C 10.562 23.861 10.326 23.903 10.27 23.901 C 10.202 23.901 10.046 23.8 9.819 23.607 L 9.817 23.607 Z M 7.284 19.918 C 8.289 19.539 9.118 19.223 9.123 19.216 C 9.136 19.202 5.206 15.39 4.684 14.909 C 4.539 14.775 4.409 14.672 4.399 14.682 C 4.39 14.693 4.07 15.547 3.69 16.577 C 3.27 17.715 3.011 18.476 3.032 18.508 C 3.057 18.55 5.424 20.599 5.451 20.605 C 5.454 20.605 6.279 20.296 7.284 19.918 Z M 20.796 17.829 C 21.319 16.564 21.732 15.519 21.718 15.504 C 21.699 15.483 11.888 8.858 5.02 4.226 L 4.469 3.855 L 3.569 4.262 C 3.072 4.484 2.673 4.687 2.68 4.709 C 2.69 4.737 6.011 8.219 10.064 12.449 L 17.432 20.141 L 18.638 20.133 L 19.847 20.125 L 20.796 17.829 Z M 19.434 6.972 C 19.607 6.01 19.75 5.191 19.75 5.151 C 19.75 5.108 19.497 4.902 19.074 4.605 C 18.702 4.343 18.373 4.115 18.344 4.096 C 18.291 4.063 14.442 5.104 14.447 5.149 C 14.451 5.179 19.091 8.748 19.107 8.731 C 19.115 8.725 19.261 7.932 19.434 6.972 Z"/></svg>`;

export function getInventoryGamesWithItems({
  accounts = [],
  inventoryAccount = 'all',
  inventoryContexts = new Map(),
  inventoryPages = new Map(),
  games = [],
} = {}) {
  const targetAccounts = accounts.filter(a => a.launcher === 'steam' && (inventoryAccount === 'all' || a.id === inventoryAccount));
  const accountIds = new Set(targetAccounts.map(a => a.id));
  const gameMap = new Map();

  // 1. Process inventoryContexts
  for (const [accId, contexts] of inventoryContexts.entries()) {
    if (!accountIds.has(accId) || !Array.isArray(contexts)) continue;
    for (const c of contexts) {
      const appId = String(c.app_id);
      const count = typeof c.count === 'number' ? c.count : null;
      if (count === 0) continue;

      let entry = gameMap.get(appId);
      if (!entry) {
        let name = games.find(g => String(g.app_id) === appId)?.name || c.name || KNOWN_STEAM_APP_NAMES[appId] || `Steam #${appId}`;
        if (appId === '753' && (!name || name === 'Steam')) {
          name = 'Коллекционные карточки';
        }
        entry = { app_id: appId, name, count: 0, hasExplicitCount: false };
        gameMap.set(appId, entry);
      }
      if (count != null && count > 0) {
        entry.count += count;
        entry.hasExplicitCount = true;
      }
    }
  }

  // 2. Process loaded inventoryPages
  for (const [key, page] of inventoryPages.entries()) {
    let accId, appId;
    try {
      [accId, appId] = JSON.parse(key);
    } catch {
      continue;
    }
    if (!accountIds.has(accId)) continue;
    const pageCount = (page && Array.isArray(page.items) ? page.items.length : 0) || page?.total_count || 0;
    const sAppId = String(appId);

    if (pageCount > 0) {
      let entry = gameMap.get(sAppId);
      if (!entry) {
        let name = games.find(g => String(g.app_id) === sAppId)?.name || KNOWN_STEAM_APP_NAMES[sAppId] || `Steam #${sAppId}`;
        if (sAppId === '753' && (!name || name === 'Steam')) {
          name = 'Коллекционные карточки';
        }
        entry = { app_id: sAppId, name, count: 0, hasExplicitCount: false };
        gameMap.set(sAppId, entry);
      }
      if (!entry.hasExplicitCount) {
        entry.count += pageCount;
      }
    }
  }

  // 3. Filter only games where there is actually something
  const result = [];
  for (const [appId, info] of gameMap.entries()) {
    const pagesForGame = [];
    for (const [key, page] of inventoryPages.entries()) {
      try {
        const [accId, aId] = JSON.parse(key);
        if (accountIds.has(accId) && String(aId) === appId) {
          pagesForGame.push(page);
        }
      } catch {}
    }

    if (pagesForGame.length > 0 && pagesForGame.every(p => (p.total_count === 0 || (p.items && p.items.length === 0)) && (!p.items || !p.items.length) && p.total_count === 0)) {
      if (!info.hasExplicitCount || info.count === 0) {
        continue;
      }
    }

    const hasItems = info.count > 0 || pagesForGame.some(p => (p.items && p.items.length > 0) || p.total_count > 0) || (!info.hasExplicitCount && pagesForGame.length === 0);

    if (hasItems) {
      const short = KNOWN_GAME_SHORTS[appId] ||
        info.name.split(/[\s:_-]+/).filter(Boolean).map(w => w[0]).join('').slice(0, 4).toUpperCase() ||
        info.name.slice(0, 2).toUpperCase();
      result.push({
        app_id: appId,
        name: info.name,
        short,
        count: info.count,
      });
    }
  }

  // 4. Sort: priority games first, then count desc, then name
  const priorityOrder = ['730', '570', '753', '578080', '440', '252490'];
  result.sort((a, b) => {
    const pA = priorityOrder.indexOf(a.app_id);
    const pB = priorityOrder.indexOf(b.app_id);
    if (pA !== -1 && pB !== -1) return pA - pB;
    if (pA !== -1) return -1;
    if (pB !== -1) return 1;
    if (b.count !== a.count) return b.count - a.count;
    return a.name.localeCompare(b.name, getLocale());
  });

  return result;
}
