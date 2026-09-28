import test from 'node:test';
import assert from 'node:assert/strict';
import {filterGames,metricKey,normalizeEpic,normalizeMetrics,safeImage,mergeLibraries,accountHasGameBan,accountRestrictionsSummary} from '../../ui/model.mjs';
const games=[{app_id:'730',launcher:'steam',name:'Counter-Strike 2',is_installed:true},{app_id:'730',launcher:'epic',name:'Игра Epic',is_installed:false},{app_id:'570',launcher:'steam',name:'Dota 2',is_installed:true}];
test('combined search, platform, installed and favorites filters',()=>{assert.deepEqual(filterGames(games,{platform:'steam',installed:'installed',query:'  COUNTER ',favorites:['steam:730']}),[games[0]]);assert.equal(filterGames(games,{platform:'epic',installed:'installed'}).length,0);});
test('recent sort uses timestamps, then stable alphabetical fallback',()=>{assert.deepEqual(filterGames(games,{sort:'recent',recent:{'steam:570':10}}).map(g=>g.name),['Dota 2','Игра Epic','Counter-Strike 2']);});
test('metrics are isolated by platform, game and account',()=>{assert.notEqual(metricKey(games[0],{id:'a'}),metricKey(games[1],{id:'a'}));assert.notEqual(metricKey(games[0],{id:'a'}),metricKey(games[0],{id:'b'}));assert.notEqual(metricKey(games[0],{id:'a'}),metricKey(games[2],{id:'a'}));});
test('manual metrics preserve unknown versus zero and reject bad values',()=>{assert.deepEqual(normalizeMetrics({skins:'0',value:'',hours:'1.5'}),{skins:0,value:null,hours:1.5});for(const skins of ['-1','1.2','NaN','Infinity'])assert.throws(()=>normalizeMetrics({skins}));});
test('Epic game mapping retains the owning account',()=>{assert.deepEqual(normalizeEpic([{app_name:'game',title:'Game'}],'owner'),[{app_name:'game',title:'Game',launcher:'epic',app_id:'game',name:'Game',owner_id:'owner'}]);});
test('external artwork only accepts HTTP(S) and data base64 PNG/JPEG/WEBP',()=>{
  assert.equal(safeImage('javascript:alert(1)'),'');
  assert.equal(safeImage('file:///tmp/a'),'');
  assert.equal(safeImage('https://example.com/a.jpg'),'https://example.com/a.jpg');
  assert.equal(safeImage('data:image/png;base64,iVBORw0KGgoAAAANSUhEUg=='),'data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==');
});

test('Epic libraries retain every owner and installed metadata without duplicates',()=>{
const a=normalizeEpic([{app_name:'shared',title:'Shared',is_installed:false}],'a');
const b=normalizeEpic([{app_name:'shared',title:'Shared',is_installed:true},{app_name:'other',title:'Other'}],'b');
const merged=mergeLibraries([a,b]);assert.equal(merged.length,2);assert.deepEqual(merged[0].owner_ids,['a','b']);assert.equal(merged[0].is_installed,true);
});

import {normalizeSteam,steamStats,inventoryKey,appendInventory} from '../../ui/model.mjs';
test('Steam merges accounts without mixing individual playtime or ownership',()=>{
 const rows=[{app_id:'620',name:'Steam #620',account_id:'a',access:'family',playtime_minutes:0},{app_id:'620',name:'Portal 2',account_id:'b',access:'owned',playtime_minutes:120}];
 assert.equal(normalizeSteam(rows).length,1);assert.equal(normalizeSteam(rows)[0].name,'Portal 2');assert.equal(steamStats(rows,'620','a').playtime_minutes,0);assert.equal(steamStats(rows,'620','b').playtime_minutes,120);assert.equal(steamStats(rows,'620','missing'),undefined);
});
test('inventory isolates contexts and deduplicates overlapping pages using string IDs',()=>{
 assert.notEqual(inventoryKey('a','730'),inventoryKey('b','730'));assert.notEqual(inventoryKey('a','753','2'),inventoryKey('a','753','6'));
 const merged=appendInventory({items:[{asset_id:'9007199254740999',name:'one'}]},{items:[{asset_id:'9007199254740999',name:'updated'},{asset_id:'9007199254740998',name:'two'}],next_cursor:null});
 assert.equal(merged.items.length,2);assert.equal(merged.items[0].name,'updated');
});

import {eligibleSteamAccounts,aggregateSteamStats,filterInventory,inventoryIdentity,priceKey,inventoryValue,isItemUnsellable} from '../../ui/model.mjs';
test('favorites stay above all other games for every sort, including playtime',()=>{
 const favorite=['steam:730'],minutes={'steam:570':900,'steam:730':0};
 for(const sort of ['name','recent','hours'])assert.equal(filterGames(games,{favoriteKeys:favorite,sort,playtime:minutes})[0].app_id,'730');
 assert.equal(filterGames(games,{sort:'hours',playtime:minutes})[0].app_id,'570');
});
test('account-game association requires ownership, sharing or positive playtime, even for free games',()=>{
 const accounts=[{id:'a',launcher:'steam'},{id:'b',launcher:'steam'},{id:'c',launcher:'steam'},{id:'e',launcher:'epic'}];
 const rows=[{app_id:'620',account_id:'a',access:'family'},{app_id:'620',account_id:'b',access:'history'}];
 assert.deepEqual(eligibleSteamAccounts({app_id:'620',is_free:false},accounts,rows).map(a=>a.id),['a']);
 assert.deepEqual(eligibleSteamAccounts({app_id:'730',is_free:true},accounts,rows).map(a=>a.id),[]);
 rows.push({app_id:'730',account_id:'b',access:'history',playtime_minutes:1},{app_id:'730',account_id:'a',access:'local',playtime_minutes:0},{app_id:'730',account_id:'c',access:'local',playtime_minutes:null});
 assert.deepEqual(eligibleSteamAccounts({app_id:'730',is_free:true},accounts,rows).map(a=>a.id),['b']);
});
test('aggregate hours deduplicate account-game pairs and retain unknown coverage',()=>{
 const rows=[{account_id:'a',app_id:'730',playtime_minutes:120},{account_id:'a',app_id:'730',playtime_minutes:120},{account_id:'b',app_id:'730',playtime_minutes:0},{account_id:'a',app_id:'570',playtime_minutes:60}];
 assert.deepEqual(aggregateSteamStats(rows,['a','b','c'],'730'),{minutes:120,recent:null,known_accounts:2,total_accounts:3});
 assert.equal(aggregateSteamStats(rows,['a']).minutes,180);assert.equal(aggregateSteamStats(rows,['c']).minutes,null);
});
test('combined inventory keeps same asset IDs from different accounts and supports trade filters',()=>{
 const a={account_id:'a',asset_id:'1',name:'Skin',tags:['Rare'],tradable:true,marketable:false};
 const b={...a,account_id:'b',tradable:false,marketable:true};
 assert.notEqual(inventoryIdentity(a),inventoryIdentity(b));
 assert.deepEqual(filterInventory([a,b],{availability:'tradable',query:'skin',tag:'Rare'}),[a]);
 assert.deepEqual(filterInventory([a,b],{availability:'marketable'}),[b]);assert.deepEqual(filterInventory([a,b],{availability:'locked'}),[b]);
});
test('inventory valuation multiplies stacks, separates unknown/unmarketable and scopes by game',()=>{
 const quotes=new Map([[priceKey('730','Skin'),{unit_price_cents:123,currency:'USD'}]]);
 const items=[{amount:3,marketable:true,market_hash_name:'Skin'},{amount:2,marketable:true,market_hash_name:'Unknown'},{amount:1,marketable:false}];
 assert.deepEqual(inventoryValue(items,quotes,'730'),{cents:369,priced:3,unpriced:2,not_marketable:1,unsellable_banned:0});
 assert.equal(inventoryValue(items,quotes,'570').priced,0);
});

test('all-game inventory prices and identities remain isolated across games and contexts',()=>{
 const items=[{account_id:'a',app_id:'730',context_id:'2',asset_id:'1',name:'Same',amount:2,marketable:true},{account_id:'a',app_id:'570',context_id:'2',asset_id:'1',name:'Same',amount:3,marketable:true},{account_id:'a',app_id:'730',context_id:'3',asset_id:'1',name:'Same',amount:1,marketable:true}].map(i=>({...i,market_hash_name:i.name}));
 assert.equal(new Set(items.map(inventoryIdentity)).size,3);
 const quotes=new Map([[priceKey('730','Same'),{currency:'USD',unit_price_cents:100}],[priceKey('570','Same'),{currency:'USD',unit_price_cents:500}]]);
 assert.equal(inventoryValue(items,quotes).cents,1800);
 assert.equal(inventoryValue(items.filter(i=>i.app_id==='730'),quotes).cents,300);
});
test('Steam facets use OR within category, AND across categories; price sorting keeps unknown last',()=>{
 const item=(id,rarity,type)=>({app_id:'730',asset_id:id,name:id,amount:1,marketable:true,tradable:true,market_hash_name:id,tags:[],tag_details:[{category:'rarity',value:rarity},{category:'type',value:type}]});
 const items=[item('900719925474099300','rare','rifle'),item('900719925474099302','common','rifle'),item('3','rare','pistol')];
 assert.equal(filterInventory(items,{facets:{'730:rarity':['rare','common'],'730:type':['rifle']}}).length,2);
 assert.equal(filterInventory(items,{sort:'newest'})[0].asset_id,'900719925474099302');
 const quotes=new Map([[priceKey('730',items[0].name),{currency:'USD',unit_price_cents:500}],[priceKey('730',items[2].name),{currency:'USD',unit_price_cents:100}]]);
 assert.deepEqual(filterInventory(items,{sort:'price_asc',quotes}).map(i=>i.asset_id),['3','900719925474099300','900719925474099302']);
 assert.equal(filterInventory(items,{minPrice:'2',quotes}).length,1);
});

import {CURRENCIES,convertUsdCents,accountLibraryValue} from '../../ui/model.mjs';
test('currency conversion supports only selected Steam currencies and never relabels USD on missing rates',()=>{
 assert(!Object.hasOwn(CURRENCIES,'BYN'));
 assert.equal(convertUsdCents(1234,'RUB',{RUB:90}),111060);
 assert.equal(convertUsdCents(1234,'EUR',{EUR:0.9}),1111);
 assert.equal(convertUsdCents(1234,'USD'),1234);
 for(const rate of [undefined,0,-1,Infinity,NaN])assert.equal(convertUsdCents(100,'EUR',{EUR:rate}),null);
 assert.equal(convertUsdCents(null,'USD'),null);assert.equal(convertUsdCents(0,'EUR',{EUR:0.9}),0);assert.equal(convertUsdCents(100,'BYN',{BYN:3}),null);
});
test('library valuation separates ownership from family access, deduplicates licenses and keeps unknown prices',()=>{
 const rows=[{account_id:'a',app_id:'1',access:'family'},{account_id:'a',app_id:'1',access:'owned'},{account_id:'a',app_id:'1',access:'owned'},{account_id:'a',app_id:'2',access:'family'},{account_id:'a',app_id:'3',access:'owned'},{account_id:'a',app_id:'4',access:'owned'},{account_id:'a',app_id:'5',access:'local'},{account_id:'b',app_id:'1',access:'family'}];
 const quote=(price_cents,is_free=false)=>({currency:'USD',region:'US',price_cents,is_free});
 const quotes=new Map([['1',quote(1000)],['2',quote(2000)],['3',quote(0,true)],['4',quote(null)]]);
 const value=accountLibraryValue(rows,'a',quotes);assert.equal(value.owned.cents,1000);assert.equal(value.family.cents,2000);assert.equal(value.total_cents,3000);assert.equal(value.owned.total,3);assert.equal(value.owned.unknown,1);assert.equal(value.owned.free,1);assert.equal(value.unconfirmed,1);
 assert.equal(accountLibraryValue(rows,'b',quotes).family.cents,1000);assert.equal(accountLibraryValue(rows,'b',quotes).owned.cents,0);
 quotes.set('1',{currency:'EUR',region:'DE',price_cents:1000});assert.equal(accountLibraryValue(rows,'b',quotes).family.unknown,1);
});
test('inventory price bounds use the selected display currency',()=>{
 const items=[{app_id:'730',asset_id:'1',name:'Skin',market_hash_name:'Skin',tags:[],marketable:true}];
 const quotes=new Map([[priceKey('730','Skin'),{currency:'USD',unit_price_cents:200}]]);
 assert.equal(filterInventory(items,{quotes,exchangeRate:90,minPrice:'150',maxPrice:'200'}).length,1);
 assert.equal(filterInventory(items,{quotes,exchangeRate:90,minPrice:'190'}).length,0);
 assert.equal(filterInventory(items,{quotes,exchangeRate:null,minPrice:'1'}).length,0);
});

test('accountHasGameBan correctly detects game bans by app_id and name', () => {
  const restrictionsMap = {
    'acc1': {
      account_id: 'acc1',
      trade_banned: true,
      market_banned: true,
      vac_banned: true,
      game_bans: [
        { app_id: '730', name: 'Counter-Strike 2' },
        { app_id: '252490', name: 'Rust' }
      ]
    },
    'acc2': {
      account_id: 'acc2',
      trade_banned: false,
      market_banned: false,
      vac_banned: false,
      game_bans: []
    }
  };

  // Match by appId
  assert.equal(accountHasGameBan('acc1', '730', 'Counter-Strike 2', restrictionsMap), true);
  assert.equal(accountHasGameBan('acc1', 730, '', restrictionsMap), true);
  assert.equal(accountHasGameBan('acc1', '252490', 'Rust', restrictionsMap), true);
  assert.equal(accountHasGameBan('acc1', '570', 'Dota 2', restrictionsMap), false);

  // A known but different AppID must not be overridden by a matching name.
  assert.equal(accountHasGameBan('acc1', '99999', 'Counter-Strike 2', restrictionsMap), false);
  assert.equal(accountHasGameBan('acc1', null, 'rust', restrictionsMap), true);

  // Clean account
  assert.equal(accountHasGameBan('acc2', '730', 'Counter-Strike 2', restrictionsMap), false);
  assert.equal(accountHasGameBan('non_existent', '730', 'Counter-Strike 2', restrictionsMap), false);
});

test('game ban attribution uses exact AppID or exact normalized name when an ID is absent', () => {
  const restrictions = {
    account_id: 'account',
    game_bans: [
      { app_id: '730', name: 'Counter-Strike 2' },
      { app_id: '', name: 'Dota   2' },
    ],
  };
  const map = new Map([['account', restrictions]]);
  assert.equal(accountHasGameBan('account', '730', 'Counter-Strike 2 Full Edition', map), true);
  assert.equal(accountHasGameBan('account', '999999', 'Counter-Strike 2', map), false);
  assert.equal(accountHasGameBan('account', '999999', 'Counter-Strike 2 Full Edition', map), false);
  assert.equal(accountHasGameBan('account', null, 'Counter-Strike 2 Full Edition', map), false);
  assert.equal(accountHasGameBan('account', '570', '  DOTA 2 ', map), true);
  assert.equal(accountHasGameBan('account', '570', 'Dota 2 Full Edition', map), false);
  assert.equal(isItemUnsellable({ account_id: 'account', app_id: '999999', game_name: 'Counter-Strike 2' }, map), null);
  assert.equal(isItemUnsellable({ account_id: 'account', app_id: '570', game_name: 'DOTA 2' }, map)?.type, 'game_ban');
});

test('an account-wide VAC count or unknown ban type does not identify a VAC game', () => {
  const map = new Map([
    ['count-only', { account_id: 'count-only', vac_banned: true, vac_bans_count: 1, game_bans: [] }],
    ['unknown-type', { account_id: 'unknown-type', vac_banned: true, vac_bans_count: 1,
      game_bans: [{ app_id: '730', name: 'Counter-Strike 2', ban_type: 'Блокировка Steam API' }] }],
    ['explicit-vac', { account_id: 'explicit-vac', vac_banned: true, vac_bans_count: 1,
      game_bans: [{ app_id: '730', name: 'Counter-Strike 2', ban_type: 'VAC' }] }],
  ]);
  assert.equal(isItemUnsellable({ account_id: 'count-only', app_id: '730', game_name: 'Counter-Strike 2' }, map), null);
  assert.equal(isItemUnsellable({ account_id: 'unknown-type', app_id: '730', game_name: 'Counter-Strike 2' }, map)?.type, 'game_ban');
  assert.equal(isItemUnsellable({ account_id: 'explicit-vac', app_id: '730', game_name: 'Counter-Strike 2' }, map)?.type, 'vac');
});

test('accountRestrictionsSummary summarizes all restrictions and badges correctly', () => {
  const restrictionsMap = {
    'banned_acc': {
      account_id: 'banned_acc',
      trade_banned: true,
      market_banned: true,
      vac_banned: true,
      vac_bans_count: 1,
      community_banned: true,
      game_bans_count: 2,
      game_bans: [
        { app_id: '730', name: 'Counter-Strike 2' }
      ]
    },
    'clean_acc': {
      account_id: 'clean_acc',
      trade_banned: false,
      market_banned: false,
      vac_banned: false,
      vac_bans_count: 0,
      community_banned: false,
      game_bans: []
    }
  };

  const bannedSummary = accountRestrictionsSummary('banned_acc', restrictionsMap);
  assert.equal(bannedSummary.has_restrictions, true);
  assert.equal(bannedSummary.trade_banned, true);
  assert.equal(bannedSummary.market_banned, true);
  assert.equal(bannedSummary.community_banned, true);
  assert.equal(bannedSummary.game_bans.length, 1);
  assert.equal(bannedSummary.game_bans_count, 2);
  const badgeTypes = bannedSummary.badges.map(b => b.type);
  assert.ok(badgeTypes.includes('market'));
  assert.ok(badgeTypes.includes('trade'));
  assert.ok(badgeTypes.includes('game_ban'));
  assert.ok(badgeTypes.includes('game_ban_count'));
  assert.ok(badgeTypes.includes('community'));

  const cleanSummary = accountRestrictionsSummary('clean_acc', restrictionsMap);
  assert.equal(cleanSummary.has_restrictions, false);
  assert.equal(cleanSummary.badges.length, 1);
  assert.equal(cleanSummary.badges[0].type, 'clean');
  assert.equal(cleanSummary.badges[0].label, 'Бан не найден в доступных данных');

  // An account with no fetched data must not be declared clean.
  const unknownSummary = accountRestrictionsSummary('unknown', restrictionsMap);
  assert.equal(unknownSummary.has_restrictions, false);
  assert.equal(unknownSummary.badges[0].type, 'unknown');
});

test('accountLibraryValue evaluates games in account native currency', () => {
  const rows = [
    { account_id: 'br_user', app_id: '322170', access: 'owned' },
    { account_id: 'br_user', app_id: '570', access: 'family' },
    { account_id: 'ru_user', app_id: '322170', access: 'owned' },
  ];
  const quotes = new Map([
    ['BRL:322170', { currency: 'BRL', region: 'BR', price_cents: 1699, is_free: false }],
    ['BRL:570', { currency: 'BRL', region: 'BR', price_cents: 0, is_free: true }],
    ['RUB:322170', { currency: 'RUB', region: 'RU', price_cents: 20000, is_free: false }],
  ]);

  const brVal = accountLibraryValue(rows, 'br_user', quotes, 'BRL');
  assert.equal(brVal.owned.cents, 1699);
  assert.equal(brVal.family.cents, 0);
  assert.equal(brVal.total_cents, 1699);
  assert.equal(brVal.family.total, 1);

  const ruVal = accountLibraryValue(rows, 'ru_user', quotes, 'RUB');
  assert.equal(ruVal.owned.cents, 20000);
  assert.equal(ruVal.family.total, 0); // 0 family games!
  assert.equal(ruVal.total_cents, 20000);
});

test('isItemUnsellable accurately detects items blocked by KT, market ban, trade ban and game/VAC ban', () => {
  const restrictionsMap = {
    'kt_acc': {
      account_id: 'kt_acc',
      community_banned: true,
      trade_banned: true,
      market_banned: true,
      vac_banned: false,
      game_bans: []
    },
    'market_banned_acc': {
      account_id: 'market_banned_acc',
      community_banned: false,
      trade_banned: false,
      market_banned: true,
      vac_banned: false,
      game_bans: []
    },
    'trade_banned_acc': {
      account_id: 'trade_banned_acc',
      community_banned: false,
      trade_banned: true,
      market_banned: false,
      vac_banned: false,
      game_bans: []
    },
    'cs_vac_acc': {
      account_id: 'cs_vac_acc',
      community_banned: false,
      trade_banned: false,
      market_banned: false,
      vac_banned: true,
      game_bans: [
        { app_id: '730', name: 'Counter-Strike 2', ban_type: 'VAC' }
      ]
    },
    'unknown_vac_acc': {
      account_id: 'unknown_vac_acc',
      vac_banned: true,
      vac_bans_count: 1,
      game_bans: []
    },
    'clean_acc': {
      account_id: 'clean_acc',
      community_banned: false,
      trade_banned: false,
      market_banned: false,
      vac_banned: false,
      game_bans: []
    }
  };

  // Community Ban (КТ) blocks all items
  const ktItem = isItemUnsellable({ account_id: 'kt_acc', app_id: '730' }, restrictionsMap);
  assert.equal(ktItem.type, 'community');
  assert.ok(ktItem.label.includes('КТ'));

  // Market Ban blocks all items
  const marketItem = isItemUnsellable({ account_id: 'market_banned_acc', app_id: '570' }, restrictionsMap);
  assert.equal(marketItem.type, 'market');

  // Trade Ban blocks all items
  const tradeItem = isItemUnsellable({ account_id: 'trade_banned_acc', app_id: '730' }, restrictionsMap);
  assert.equal(tradeItem.type, 'trade');

  // VAC ban in CS2 blocks CS2 items (730), but NOT Dota 2 items (570)
  const vacItem = isItemUnsellable({ account_id: 'cs_vac_acc', app_id: '730' }, restrictionsMap);
  assert.equal(vacItem.type, 'vac');
  assert.ok(vacItem.label.includes('VAC'));

  const nonBannedGameItem = isItemUnsellable({ account_id: 'cs_vac_acc', app_id: '570' }, restrictionsMap);
  assert.equal(nonBannedGameItem, null);

  // An account-wide VAC count alone does not identify CS2 or block its items.
  assert.equal(isItemUnsellable({ account_id: 'unknown_vac_acc', app_id: '730' }, restrictionsMap), null);

  // Clean account has no unsellable items
  const cleanItem = isItemUnsellable({ account_id: 'clean_acc', app_id: '730' }, restrictionsMap);
  assert.equal(cleanItem, null);
});

test('inventoryValue excludes banned unsellable items when enabled and includes them when disabled', () => {
  const restrictionsMap = {
    'banned_acc': {
      account_id: 'banned_acc',
      community_banned: true,
      trade_banned: true,
      market_banned: true,
      vac_banned: false,
      game_bans: []
    },
    'cs_banned_acc': {
      account_id: 'cs_banned_acc',
      community_banned: false,
      trade_banned: false,
      market_banned: false,
      vac_banned: true,
      game_bans: [{ app_id: '730', name: 'Counter-Strike 2', ban_type: 'VAC' }]
    },
    'clean_acc': {
      account_id: 'clean_acc',
      community_banned: false,
      trade_banned: false,
      market_banned: false,
      vac_banned: false,
      game_bans: []
    }
  };

  const quotes = new Map([
    [priceKey('730', 'AK-47'), { unit_price_cents: 2000, currency: 'USD' }],
    [priceKey('570', 'Dragonclaw Hook'), { unit_price_cents: 15000, currency: 'USD' }]
  ]);

  const items = [
    { account_id: 'banned_acc', app_id: '730', amount: 1, marketable: true, market_hash_name: 'AK-47' }, // 2000 cents, KT ban
    { account_id: 'cs_banned_acc', app_id: '730', amount: 1, marketable: true, market_hash_name: 'AK-47' }, // 2000 cents, VAC in CS
    { account_id: 'cs_banned_acc', app_id: '570', amount: 1, marketable: true, market_hash_name: 'Dragonclaw Hook' }, // 15000 cents, allowed (Dota 2)
    { account_id: 'clean_acc', app_id: '730', amount: 2, marketable: true, market_hash_name: 'AK-47' }, // 4000 cents, clean
  ];

  // Default / Enabled: exclude unsellable banned items (banned_acc item + cs_banned_acc CS item excluded)
  const resultExcluded = inventoryValue(items, quotes, null, restrictionsMap, { excludeUnsellable: true });
  assert.equal(resultExcluded.unsellable_banned, 2); // 1 from banned_acc + 1 from cs_banned_acc
  assert.equal(resultExcluded.priced, 3); // 1 dota item + 2 clean AK items
  assert.equal(resultExcluded.cents, 15000 + 4000); // 19000 cents

  // Setting disabled: all marketable items are priced regardless of bans
  const resultIncluded = inventoryValue(items, quotes, null, restrictionsMap, { excludeUnsellable: false });
  assert.equal(resultIncluded.unsellable_banned, 0);
  assert.equal(resultIncluded.priced, 5); // all 5 items
  assert.equal(resultIncluded.cents, 2000 + 2000 + 15000 + 4000); // 23000 cents
});

test('filterInventory supports filtering by banned availability and market status', () => {
  const restrictionsMap = {
    'banned_acc': {
      account_id: 'banned_acc',
      community_banned: true,
      game_bans: []
    },
    'clean_acc': {
      account_id: 'clean_acc',
      community_banned: false,
      game_bans: []
    }
  };

  const items = [
    { account_id: 'banned_acc', app_id: '730', name: 'Item 1', amount: 1, marketable: true, tradable: true },
    { account_id: 'clean_acc', app_id: '730', name: 'Item 2', amount: 1, marketable: true, tradable: true }
  ];

  const bannedItems = filterInventory(items, { availability: 'banned', restrictionsMap });
  assert.equal(bannedItems.length, 1);
  assert.equal(bannedItems[0].name, 'Item 1');

  const bannedMarket = filterInventory(items, { market: 'banned', restrictionsMap });
  assert.equal(bannedMarket.length, 1);
  assert.equal(bannedMarket[0].name, 'Item 1');
});

import { isIgnoredSteamApp } from '../../ui/model.mjs';

test('isIgnoredSteamApp correctly identifies and filters Steam utility apps', () => {
  assert.equal(isIgnoredSteamApp({ app_id: '7', name: 'Steam Client' }), true);
  assert.equal(isIgnoredSteamApp({ app_id: '760', name: 'Steam Screenshots' }), true);
  assert.equal(isIgnoredSteamApp({ app_id: '2371090', name: 'Steam Game Notes' }), true);
  assert.equal(isIgnoredSteamApp({ app_id: '9999', name: 'steam client' }), true);
  assert.equal(isIgnoredSteamApp({ app_id: '9999', name: 'Steam screenshots' }), true);
  assert.equal(isIgnoredSteamApp({ app_id: '9999', name: 'STEAM GAME NOTES' }), true);
  assert.equal(isIgnoredSteamApp('7'), true);
  assert.equal(isIgnoredSteamApp('steam_760'), true);
  assert.equal(isIgnoredSteamApp('Steam Screenshots'), true);

  assert.equal(isIgnoredSteamApp({ app_id: '730', name: 'Counter-Strike 2' }), false);
  assert.equal(isIgnoredSteamApp({ app_id: '570', name: 'Dota 2' }), false);

  const testGames = [
    { app_id: '7', name: 'Steam Client', launcher: 'steam' },
    { app_id: '760', name: 'Steam Screenshots', launcher: 'steam' },
    { app_id: '2371090', name: 'Steam Game Notes', launcher: 'steam' },
    { app_id: '730', name: 'Counter-Strike 2', launcher: 'steam' },
  ];

  const filtered = filterGames(testGames);
  assert.equal(filtered.length, 1);
  assert.equal(filtered[0].app_id, '730');

  const normalized = normalizeSteam(testGames);
  assert.equal(normalized.length, 1);
  assert.equal(normalized[0].app_id, '730');

  const rows = [
    { app_id: '7', name: 'Steam Client', account_id: 'u1', access: 'owned' },
    { app_id: '760', name: 'Steam Screenshots', account_id: 'u1', access: 'owned' },
    { app_id: '2371090', name: 'Steam Game Notes', account_id: 'u1', access: 'owned' },
    { app_id: '730', name: 'Counter-Strike 2', account_id: 'u1', access: 'owned' },
  ];
  const quotes = new Map([['USD:730', { currency: 'USD', price_cents: 0, is_free: true }]]);
  const val = accountLibraryValue(rows, 'u1', quotes, 'USD');
  assert.equal(val.owned.total, 1);
  assert.equal(val.owned.items.length, 1);
  assert.equal(val.owned.items[0].row.app_id, '730');
});

test('accountHasGameBan correctly filters out banned games for a specific account', () => {
  const restrictionsMap = new Map([
    ['u1', {
      account_id: 'u1',
      game_bans: [{ app_id: '730', name: 'Counter-Strike 2' }]
    }],
    ['u2', {
      account_id: 'u2',
      game_bans: []
    }]
  ]);

  const cs2 = { app_id: '730', name: 'Counter-Strike 2', launcher: 'steam' };
  const dota2 = { app_id: '570', name: 'Dota 2', launcher: 'steam' };

  assert.equal(accountHasGameBan('u1', cs2.app_id, cs2.name, restrictionsMap), true);
  assert.equal(accountHasGameBan('u1', dota2.app_id, dota2.name, restrictionsMap), false);
  assert.equal(accountHasGameBan('u2', cs2.app_id, cs2.name, restrictionsMap), false);

  // When filtering library for u1, banned games are excluded
  const steamAccountFilter = 'u1';
  const gamesList = [cs2, dota2];
  const eligible = gamesList.filter(g => {
    if (steamAccountFilter !== 'all') {
      if (accountHasGameBan(steamAccountFilter, g.app_id, g.name, restrictionsMap)) return false;
    }
    return true;
  });

  assert.deepEqual(eligible.map(g => g.app_id), ['570']);
});

test('accountLibraryValue with treatUnpricedAsFree counts unpriced games as free and fully evaluated', () => {
  const rows = [
    { account_id: 'u1', app_id: '1', access: 'owned' },
    { account_id: 'u1', app_id: '2', access: 'owned' },
    { account_id: 'u1', app_id: '3', access: 'owned' },
    { account_id: 'u1', app_id: '4', access: 'family' }
  ];
  // Only app '1' has a price quote ($19.99), app '2' is free ($0), app '3' and '4' have no quotes
  const quotes = new Map([
    ['USD:1', { currency: 'USD', price_cents: 1999, is_free: false }],
    ['USD:2', { currency: 'USD', price_cents: 0, is_free: true }]
  ]);

  const val = accountLibraryValue(rows, 'u1', quotes, 'USD', { treatUnpricedAsFree: true });
  assert.equal(val.owned.total, 3);
  assert.equal(val.owned.known, 3, 'all 3 owned games should be counted as evaluated');
  assert.equal(val.owned.unknown, 0, 'no unpriced games remain');
  assert.equal(val.owned.free, 2, 'app 2 and unpriced app 3 counted as free');
  assert.equal(val.owned.cents, 1999);

  assert.equal(val.family.total, 1);
  assert.equal(val.family.known, 1, 'family game counted as evaluated');
  assert.equal(val.family.unknown, 0);
  assert.equal(val.family.free, 1, 'unpriced family game counted as free');
  assert.equal(val.family.cents, 0);

  assert.equal(val.total_cents, 1999);
});

test('accountLibraryValue recognizes explicit is_free quotes and rows as free even without treatUnpricedAsFree', () => {
  const rows = [
    { account_id: 'u1', app_id: '10', access: 'owned', is_free: true },
    { account_id: 'u1', app_id: '20', access: 'owned' },
    { account_id: 'u1', app_id: '30', access: 'owned' }
  ];
  // app 10 is free on row, app 20 is quote with is_free: true and null price_cents, app 30 has no quote
  const quotes = new Map([
    ['USD:20', { currency: 'USD', price_cents: null, is_free: true }]
  ]);

  const val = accountLibraryValue(rows, 'u1', quotes, 'USD');
  assert.equal(val.owned.total, 3);
  assert.equal(val.owned.known, 2, 'apps 10 and 20 are known free');
  assert.equal(val.owned.free, 2);
  assert.equal(val.owned.unknown, 1, 'app 30 is unknown when treatUnpricedAsFree is not enabled');
  assert.equal(val.owned.cents, 0);
});

import { getDota2HeroIconUrl, getInventoryGamesWithItems, KNOWN_STEAM_APP_NAMES, KNOWN_GAME_SHORTS, DOTA2_LOGO_SVG } from '../../ui/model.mjs';

test('getDota2HeroIconUrl returns valid CDN URL for standard hero slugs and personas, and null for non-heroes', () => {
  assert.equal(
    getDota2HeroIconUrl('npc_dota_hero_pudge'),
    'https://cdn.cloudflare.steamstatic.com/apps/dota2/images/dota_react/heroes/icons/pudge.png'
  );
  assert.equal(
    getDota2HeroIconUrl('npc_dota_hero_crystal_maiden'),
    'https://cdn.cloudflare.steamstatic.com/apps/dota2/images/dota_react/heroes/icons/crystal_maiden.png'
  );
  assert.equal(
    getDota2HeroIconUrl('npc_dota_hero_antimage_persona1'),
    'https://cdn.cloudflare.steamstatic.com/apps/dota2/images/dota_react/heroes/icons/antimage.png'
  );
  assert.equal(
    getDota2HeroIconUrl('npc_dota_hero_phantom_assassin_persona'),
    'https://cdn.cloudflare.steamstatic.com/apps/dota2/images/dota_react/heroes/icons/phantom_assassin.png'
  );
  assert.equal(getDota2HeroIconUrl('DOTA_OtherType'), null);
  assert.equal(getDota2HeroIconUrl(''), null);
  assert.equal(getDota2HeroIconUrl(null), null);
});

test('getInventoryGamesWithItems excludes games with 0 items and populates counts from contexts and pages', () => {
  const accounts = [
    { id: 'acc1', launcher: 'steam', display_name: 'Acc 1' },
    { id: 'acc2', launcher: 'steam', display_name: 'Acc 2' },
  ];

  const inventoryContexts = new Map([
    ['acc1', [
      { app_id: '570', name: 'Dota 2', count: 955 },
      { app_id: '730', name: 'Counter-Strike 2', count: 50 },
      { app_id: '440', name: 'Team Fortress 2', count: 0 }, // empty!
      { app_id: '252490', name: 'Rust', count: 0 }, // empty!
    ]],
    ['acc2', [
      { app_id: '730', name: 'Counter-Strike 2', count: 3 },
      { app_id: '753', name: 'Steam', count: 20 },
    ]]
  ]);

  const inventoryPages = new Map([
    // acc1 has page for Banana (app 2923300) with 4 items
    [JSON.stringify(['acc1', '2923300', '2']), {
      total_count: 4,
      items: [{ asset_id: '1' }, { asset_id: '2' }, { asset_id: '3' }, { asset_id: '4' }]
    }]
  ]);

  const games = [
    { app_id: '570', name: 'Dota 2', has_inventory: true },
    { app_id: '730', name: 'Counter-Strike 2', has_inventory: true },
    { app_id: '440', name: 'Team Fortress 2', has_inventory: true }, // owned in library, but 0 items!
    { app_id: '252490', name: 'Rust', has_inventory: true }, // owned in library, but 0 items!
    { app_id: '2923300', name: 'Banana', has_inventory: true }
  ];

  // For 'all' accounts:
  const allGames = getInventoryGamesWithItems({
    accounts,
    inventoryAccount: 'all',
    inventoryContexts,
    inventoryPages,
    games
  });

  // TF2 (440) and Rust (252490) must NOT be present because count is 0
  const allAppIds = allGames.map(g => g.app_id);
  assert.ok(!allAppIds.includes('440'), 'TF2 with 0 items should not be in the list');
  assert.ok(!allAppIds.includes('252490'), 'Rust with 0 items should not be in the list');

  // Games with items must be present
  assert.ok(allAppIds.includes('570'), 'Dota 2 should be included');
  assert.ok(allAppIds.includes('730'), 'CS2 should be included');
  assert.ok(allAppIds.includes('753'), 'Steam Cards should be included');
  assert.ok(allAppIds.includes('2923300'), 'Banana should be included from pages');

  const dota = allGames.find(g => g.app_id === '570');
  assert.equal(dota.count, 955);
  assert.equal(dota.name, 'Dota 2');
  assert.equal(dota.short, 'Dota');

  const cs2 = allGames.find(g => g.app_id === '730');
  assert.equal(cs2.count, 53); // 50 from acc1 + 3 from acc2

  // For specific account 'acc2':
  const acc2Games = getInventoryGamesWithItems({
    accounts,
    inventoryAccount: 'acc2',
    inventoryContexts,
    inventoryPages,
    games
  });
  const acc2AppIds = acc2Games.map(g => g.app_id);
  assert.deepEqual(acc2AppIds.sort(), ['730', '753'].sort());
  assert.ok(!acc2AppIds.includes('570'), 'Dota 2 is not in acc2 contexts');
});

test('DOTA2_LOGO_SVG contains valid SVG element and path for Dota 2 logo', () => {
  assert.ok(typeof DOTA2_LOGO_SVG === 'string');
  assert.ok(DOTA2_LOGO_SVG.startsWith('<svg'));
  assert.ok(DOTA2_LOGO_SVG.includes('dota2-icon-svg'));
  assert.ok(DOTA2_LOGO_SVG.includes('<path fill="#e23b24"'));
});



import {accountPlayedGames} from '../../ui/model.mjs';
test('played games stay account scoped and reject zero, unknown, negative and non-finite time',()=>{
 const rows=[{app_id:'1',account_id:'a',playtime_minutes:1},{app_id:'1',account_id:'a',playtime_minutes:2},{app_id:'2',account_id:'b',playtime_minutes:90},...[0,null,-1,NaN,Infinity].map((n,i)=>({app_id:String(10+i),account_id:'a',playtime_minutes:n}))];
 assert.deepEqual(accountPlayedGames({id:'a',launcher:'steam'},rows).map(g=>[g.app_id,g.playtime_minutes]),[['1',2]]);
});
