import test from 'node:test';
import assert from 'node:assert/strict';
import {readLibraryCache, writeLibraryCache} from '../../ui/library-cache.mjs';

function storage() {
  const values = new Map();
  return {getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value)};
}
const state = {
  games: [{id:'steam_730', launcher:'steam', app_id:'730', name:'Counter-Strike 2', is_installed:true, owner_ids:['alice'], update:{phase:'downloading'}, access_token:'secret'}],
  accounts: [{id:'alice', launcher:'steam', username:'Alice', is_active:true, password:'secret', credentials:{token:'secret'}}],
  steamRows: [{launcher:'steam', app_id:'730', name:'Counter-Strike 2', account_id:'alice', access:'owned', playtime_minutes:123}],
};
test('display snapshot survives a new reader, excludes credentials and transient downloads', () => {
  const store = storage();
  assert.equal(writeLibraryCache(store, state, 1000), true);
  const saved = readLibraryCache(store, 2000);
  assert.equal(saved.games[0].name, 'Counter-Strike 2');
  assert.deepEqual(saved.games[0].owner_ids, ['alice']);
  assert.equal(saved.accounts[0].username, 'Alice');
  assert.equal(saved.steamRows[0].playtime_minutes, 123);
  assert.equal(JSON.stringify(saved).includes('secret'), false);
  assert.equal(saved.games[0].update, undefined);
});
test('empty authoritative refresh clears old games and accounts', () => {
  const store = storage();
  writeLibraryCache(store, state, 1000);
  writeLibraryCache(store, {games:[], accounts:[], steamRows:[]}, 2000);
  assert.deepEqual(readLibraryCache(store, 3000), {games:[], accounts:[], steamRows:[], savedAt:2000});
});
test('invalid, old, future and inaccessible storage all fall back to normal loading', () => {
  const store = storage();
  for (const value of ['{', '{}', JSON.stringify({version:1,savedAt:1000,games:null,accounts:[],steamRows:[]})]) {
    store.setItem('swch-library-cache-v1', value);
    assert.equal(readLibraryCache(store, 2000), null);
  }
  writeLibraryCache(store, state, 1000);
  assert.equal(readLibraryCache(store, 999), null);
  assert.equal(readLibraryCache(store, 1000 + 31*86400000), null);
  const blocked = {getItem(){throw Error('disabled');}, setItem(){throw Error('quota');}};
  assert.equal(readLibraryCache(blocked), null);
  assert.equal(writeLibraryCache(blocked, state), false);
});
