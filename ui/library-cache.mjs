// A display snapshot, never an authentication/session cache. Keep fields explicit:
// account and platform responses may also contain credentials or private payloads.
const KEY = 'swch-library-cache-v1';
const MAX_AGE = 30 * 24 * 60 * 60 * 1000;
const GAME_FIELDS = ['id', 'launcher', 'app_id', 'name', 'is_installed', 'size_bytes',
  'banner_url', 'hero_url', 'description', 'is_free', 'has_inventory', 'namespace',
  'owner_id', 'account_id', 'access', 'source', 'playtime_minutes', 'playtime_2weeks', 'last_played', 'updated_at'];
const ACCOUNT_FIELDS = ['id', 'launcher', 'username', 'display_name', 'avatar_url', 'is_active', 'last_used'];
const scalar = value => value === null || ['string', 'boolean'].includes(typeof value)
  || (typeof value === 'number' && Number.isFinite(value));
function pick(row, fields) {
  return Object.fromEntries(fields.filter(key => Object.hasOwn(row, key) && scalar(row[key])).map(key => [key, row[key]]));
}
function game(row) {
  const result = pick(row, GAME_FIELDS);
  for (const key of ['owner_ids', 'steam_accounts', 'owner_steamids']) {
    if (Array.isArray(row[key])) result[key] = row[key].filter(id => typeof id === 'string');
  }
  return result;
}
function validRow(row) {
  return row && typeof row === 'object' && typeof row.launcher === 'string' && !row.launcher.startsWith('mod:');
}
function snapshot({games, accounts, steamRows}) {
  if (![games, accounts, steamRows].every(Array.isArray)) throw new Error('Invalid library snapshot');
  return {
    games: games.filter(row => validRow(row) && typeof row.app_id === 'string' && typeof row.name === 'string').map(game),
    accounts: accounts.filter(row => validRow(row) && typeof row.id === 'string').map(row => pick(row, ACCOUNT_FIELDS)),
    steamRows: steamRows.filter(row => validRow(row) && row.launcher === 'steam' && typeof row.app_id === 'string' && typeof row.name === 'string').map(game),
  };
}
export function readLibraryCache(storage, now = Date.now()) {
  try {
    const saved = JSON.parse(storage.getItem(KEY));
    if (saved?.version !== 1 || !Number.isFinite(saved.savedAt) || saved.savedAt > now || now - saved.savedAt > MAX_AGE) return null;
    return {...snapshot(saved), savedAt: saved.savedAt};
  } catch { return null; }
}
export function writeLibraryCache(storage, state, now = Date.now()) {
  try {
    storage.setItem(KEY, JSON.stringify({version: 1, savedAt: now, ...snapshot(state)}));
    return true;
  } catch { return false; } // A full/disabled store must never interrupt loading.
}
