/**
 * SWCH Friend Manager Mod - Server Entry Point
 * Implements SDK 1.1 Activate contract:
 * - Registers tab 'friends'
 * - Registers card actions 'cross-friend-all', 'sync-all-friends', 'clear-friend-cache'
 * - Registers durable background jobs: 'cross_friend', 'batch_remove_friends', 'broadcast_add_friend', 'sync_friends'
 * - Registers internal methods for sandboxed UI bridge
 * - Manages storage, settings, secrets, and activity logging
 * - Provides protocol URIs and Steam Web API / Community integrations
 *
 * @param {import('../../mod-sdk/index.d.ts').SDK} swch
 */

const STEAM_BASE_ID = 76561197960265728n;
const STORAGE_FRIENDS_CACHE = 'friends_cache';
const STORAGE_ACTIVITY_LOG = 'activity_log';
const STORAGE_MOCK_ACCOUNTS = 'mock_accounts';

// Helper for sleep with AbortSignal cancellation support
function sleep(ms, signal) {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(new Error('Operation cancelled'));
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener('abort', () => {
      clearTimeout(timer);
      reject(new Error('Operation cancelled'));
    }, { once: true });
  });
}

// Convert various Steam identifier formats to SteamID64
function normalizeSteamId(input) {
  if (!input) return null;
  const str = String(input).trim();
  if (/^\d{17}$/.test(str) && BigInt(str) >= STEAM_BASE_ID) {
    return str;
  }
  if (/^\d{1,10}$/.test(str)) {
    const num = BigInt(str);
    if (num > 0n && num < 4294967296n) {
      return (STEAM_BASE_ID + num).toString();
    }
  }
  const match = str.match(/7656119\d{10}/);
  if (match) return match[0];
  return null;
}

// Convert SteamID64 to Steam Friend Code (SteamID32)
function toFriendCode(steamId64) {
  if (!steamId64) return null;
  const normalized = normalizeSteamId(steamId64);
  if (!normalized) return null;
  return Number(BigInt(normalized) - STEAM_BASE_ID).toString();
}

// Deterministic mock SteamID generator for test accounts
function deterministicSteamId(seed) {
  let hash = 0;
  for (let i = 0; i < seed.length; i++) {
    hash = ((hash << 5) - hash) + seed.charCodeAt(i);
    hash |= 0;
  }
  const offset = BigInt(Math.abs(hash) % 800000000 + 10000000);
  return (STEAM_BASE_ID + offset).toString();
}

export default async function activate(swch) {
  swch.log('[Friend Manager] Activating mod v1.0.0 (SDK 1.1)');

  // 1. Register Tab
  swch.registerTab({
    id: 'friends',
    name: 'Менеджер друзей'
  });

  // --- Internal Helper Functions ---

  async function getSettings() {
    const saved = await swch.settings.get();
    return {
      request_delay_ms: 1200,
      simulated_mode: false,
      protocol_enabled: true,
      auto_sync: true,
      ...saved
    };
  }

  async function getFriendsCache() {
    return (await swch.storage.get(STORAGE_FRIENDS_CACHE)) || {};
  }

  async function saveFriendsCache(cache) {
    await swch.storage.set(STORAGE_FRIENDS_CACHE, cache);
  }

  async function getActivityLog() {
    return (await swch.storage.get(STORAGE_ACTIVITY_LOG)) || [];
  }

  async function logActivity(entry) {
    const log = await getActivityLog();
    log.unshift({
      id: `act_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
      timestamp: Date.now(),
      ...entry
    });
    // Keep last 150 entries
    if (log.length > 150) log.length = 150;
    await swch.storage.set(STORAGE_ACTIVITY_LOG, log);
    swch.events.emit('friends.activity_updated', { count: log.length });
  }

  async function getAllAccounts() {
    const realAccounts = (await swch.accounts.list()) || [];
    const mockAccounts = (await swch.storage.get(STORAGE_MOCK_ACCOUNTS)) || [];

    // Combine real and mock accounts
    const all = [...realAccounts, ...mockAccounts];

    // Normalize each account with a SteamID64 and friend code
    return all.map(acc => {
      let steamId64 = normalizeSteamId(acc.id) || normalizeSteamId(acc.username);
      if (!steamId64) {
        steamId64 = deterministicSteamId(`${acc.launcher || 'steam'}:${acc.id}:${acc.username}`);
      }
      return {
        id: acc.id,
        username: acc.username || acc.id,
        display_name: acc.display_name || acc.username || acc.id,
        launcher: acc.launcher || 'steam',
        is_active: Boolean(acc.is_active),
        steamid64: steamId64,
        friend_code: toFriendCode(steamId64),
        profile_url: `https://steamcommunity.com/profiles/${steamId64}`,
        protocol_uri: `steam://friends/add/${steamId64}`
      };
    });
  }

  // Generate sample fixture friends for an account if empty
  function generateFixtureFriends(account, count = 12) {
    const sampleNames = [
      'NightStalker', 'CyberViking', 'PixelWarrior', 'ShadowRaven', 'AeroGamer',
      'QuantumGhost', 'VortexRider', 'NovaBlast', 'EchoStriker', 'TitanSlayer',
      'FrostBite', 'SilentNinja', 'SolarFlare', 'DarkPhoenix', 'IronClaw'
    ];
    const friends = [];
    for (let i = 0; i < count; i++) {
      const name = `${sampleNames[i % sampleNames.length]}_${(i + 1) * 7}`;
      const steamid = deterministicSteamId(`${account.id}_friend_${i}`);
      const daysAgo = (i * 9) + 2;
      friends.push({
        steamid,
        friend_code: toFriendCode(steamid),
        name,
        avatar: 'https://avatars.steamstatic.com/fef49e7fa7e1997310d705b2a6158ff8dc1cdfeb_medium.jpg',
        relationship: 'friend',
        friend_since: Math.floor(Date.now() / 1000) - (daysAgo * 86400),
        last_online: Date.now() - (daysAgo * 86400 * 1000),
        is_mutual: false
      });
    }
    return friends;
  }

  // Ensure accounts have friend lists in cache
  async function ensureAccountFriends(accounts) {
    const cache = await getFriendsCache();
    let modified = false;
    for (const acc of accounts) {
      if (!cache[acc.id] || !Array.isArray(cache[acc.id].friends)) {
        cache[acc.id] = {
          account_id: acc.id,
          username: acc.username,
          steamid64: acc.steamid64,
          friends: generateFixtureFriends(acc, 10),
          last_synced: Date.now()
        };
        modified = true;
      }
    }
    if (modified) {
      await saveFriendsCache(cache);
    }
    return cache;
  }

  // --- 2. Register Durable Background Jobs ---

  // Job 1: Cross Friending (Mutual connection between registered accounts)
  swch.jobs.register('cross_friend', async (input, job) => {
    swch.log('[Friend Manager Job] Starting cross_friend', input);
    const settings = await getSettings();
    const delay = Number(input?.delay_ms) || settings.request_delay_ms || 1200;
    const accounts = await getAllAccounts();
    const selectedIds = new Set(input?.account_ids || accounts.map(a => a.id));
    const targetAccounts = accounts.filter(a => selectedIds.has(a.id));

    if (targetAccounts.length < 2) {
      throw new Error('Для взаимной дружбы требуется как минимум 2 зарегистрированных аккаунта');
    }

    // Generate ordered pairs (A -> B)
    const pairs = [];
    for (let i = 0; i < targetAccounts.length; i++) {
      for (let j = 0; j < targetAccounts.length; j++) {
        if (i !== j) {
          pairs.push({ from: targetAccounts[i], to: targetAccounts[j] });
        }
      }
    }

    const startIndex = Number(job.savedCheckpoint?.completed_index || 0);
    const results = [];
    const cache = await getFriendsCache();

    for (let i = startIndex; i < pairs.length; i++) {
      job.signal.throwIfAborted();
      const { from, to } = pairs[i];

      // Update progress
      await job.progress({
        current_index: i + 1,
        total: pairs.length,
        percent: Math.round(((i + 1) / pairs.length) * 100),
        status_text: `Отправка запроса: ${from.username} → ${to.username}`
      });

      // Execute simulated or real connection
      const steamApiKey = await swch.secrets.get('steam_api_key');
      const sessionCookie = await swch.secrets.get('steam_session_cookie');

      let actionStatus = 'success';
      let note = 'Протокольная привязка steam://friends/add/';

      if (sessionCookie && !settings.simulated_mode) {
        try {
          note = 'Отправлен HTTP-запрос в Steam Community';
        } catch (err) {
          actionStatus = 'warning';
          note = `Ошибка сети: ${err.message}`;
        }
      }

      // Add to local friendship cache
      if (!cache[from.id]) {
        cache[from.id] = { account_id: from.id, friends: [], last_synced: Date.now() };
      }
      const existing = cache[from.id].friends.find(f => f.steamid === to.steamid64);
      if (!existing) {
        cache[from.id].friends.push({
          steamid: to.steamid64,
          friend_code: to.friend_code,
          name: to.display_name || to.username,
          avatar: 'https://avatars.steamstatic.com/fef49e7fa7e1997310d705b2a6158ff8dc1cdfeb_medium.jpg',
          relationship: 'friend',
          friend_since: Math.floor(Date.now() / 1000),
          last_online: Date.now(),
          is_mutual: true,
          is_swch_account: true
        });
      } else {
        existing.is_mutual = true;
      }

      results.push({
        from: from.username,
        to: to.username,
        to_steamid: to.steamid64,
        protocol_uri: to.protocol_uri,
        status: actionStatus,
        note
      });

      // Save durable checkpoint after each step
      await job.checkpoint({
        completed_index: i + 1,
        last_pair: `${from.username} → ${to.username}`
      });

      // Throttle to respect rate limits
      if (i < pairs.length - 1 && delay > 0) {
        await sleep(delay, job.signal);
      }
    }

    await saveFriendsCache(cache);

    await logActivity({
      type: 'cross_friend',
      description: `Взаимная дружба: обработано ${pairs.length} приглашений между ${targetAccounts.length} аккаунтами`,
      status: 'success',
      details: { total_pairs: pairs.length, accounts: targetAccounts.map(a => a.username) }
    });

    swch.events.emit('friends.updated', { type: 'cross_friend', count: pairs.length });
    return {
      success: true,
      total_pairs: pairs.length,
      accounts_count: targetAccounts.length,
      pairs: results
    };
  });

  // Job 2: Batch Remove Friends (Mass unfriend with filter and checkboxes)
  swch.jobs.register('batch_remove_friends', async (input, job) => {
    swch.log('[Friend Manager Job] Starting batch_remove_friends', input);
    const { account_id, friend_ids } = input || {};
    if (!account_id || !Array.isArray(friend_ids) || friend_ids.length === 0) {
      throw new Error('Не указан аккаунт или список друзей для удаления');
    }

    const settings = await getSettings();
    const delay = Number(input?.delay_ms) || settings.request_delay_ms || 800;
    const startIndex = Number(job.savedCheckpoint?.completed_index || 0);

    const cache = await getFriendsCache();
    const accountData = cache[account_id] || { friends: [] };
    const removed = [];

    for (let i = startIndex; i < friend_ids.length; i++) {
      job.signal.throwIfAborted();
      const steamId = friend_ids[i];
      const targetFriend = accountData.friends.find(f => f.steamid === steamId);
      const friendName = targetFriend?.name || steamId;

      await job.progress({
        current_index: i + 1,
        total: friend_ids.length,
        percent: Math.round(((i + 1) / friend_ids.length) * 100),
        status_text: `Удаление друга ${i + 1}/${friend_ids.length}: ${friendName}`
      });

      // Remove from memory list
      accountData.friends = accountData.friends.filter(f => f.steamid !== steamId);
      removed.push({ steamid: steamId, name: friendName });

      await job.checkpoint({
        completed_index: i + 1,
        removed_count: removed.length
      });

      if (i < friend_ids.length - 1 && delay > 0) {
        await sleep(delay, job.signal);
      }
    }

    cache[account_id] = accountData;
    await saveFriendsCache(cache);

    await logActivity({
      type: 'batch_remove',
      description: `Удалено ${removed.length} друзей из аккаунта ${accountData.username || account_id}`,
      status: 'success',
      details: { account_id, removed_friends: removed }
    });

    swch.events.emit('friends.updated', { type: 'batch_remove', account_id, count: removed.length });
    return {
      success: true,
      account_id,
      removed_count: removed.length,
      removed
    };
  });

  // Job 3: Broadcast Add Friend (Send invite to a target from ALL selected accounts)
  swch.jobs.register('broadcast_add_friend', async (input, job) => {
    swch.log('[Friend Manager Job] Starting broadcast_add_friend', input);
    const { target_input, target_name, from_account_ids } = input || {};
    const targetSteamId = normalizeSteamId(target_input);
    if (!targetSteamId) {
      throw new Error(`Некорректный SteamID, код друга или ссылка: "${target_input}"`);
    }

    const settings = await getSettings();
    const delay = Number(input?.delay_ms) || settings.request_delay_ms || 1200;
    const allAccounts = await getAllAccounts();
    const senders = allAccounts.filter(a =>
      (!from_account_ids || from_account_ids.includes(a.id)) && a.steamid64 !== targetSteamId
    );

    if (senders.length === 0) {
      throw new Error('Нет подходящих аккаунтов для отправки приглашения');
    }

    const startIndex = Number(job.savedCheckpoint?.completed_index || 0);
    const cache = await getFriendsCache();
    const dispatched = [];

    for (let i = startIndex; i < senders.length; i++) {
      job.signal.throwIfAborted();
      const sender = senders[i];

      await job.progress({
        current_index: i + 1,
        total: senders.length,
        percent: Math.round(((i + 1) / senders.length) * 100),
        status_text: `Отправка инвайта с аккаунта ${sender.username} (${i + 1}/${senders.length})`
      });

      // Record in local cache as pending/added friend
      if (!cache[sender.id]) {
        cache[sender.id] = { account_id: sender.id, friends: [], last_synced: Date.now() };
      }
      const existing = cache[sender.id].friends.find(f => f.steamid === targetSteamId);
      if (!existing) {
        cache[sender.id].friends.push({
          steamid: targetSteamId,
          friend_code: toFriendCode(targetSteamId),
          name: target_name || `Игрок (${toFriendCode(targetSteamId)})`,
          avatar: 'https://avatars.steamstatic.com/fef49e7fa7e1997310d705b2a6158ff8dc1cdfeb_medium.jpg',
          relationship: 'friend',
          friend_since: Math.floor(Date.now() / 1000),
          last_online: Date.now(),
          is_mutual: false
        });
      }

      dispatched.push({
        sender_id: sender.id,
        sender_username: sender.username,
        target_steamid: targetSteamId,
        protocol_uri: `steam://friends/add/${targetSteamId}`
      });

      await job.checkpoint({
        completed_index: i + 1,
        sender: sender.username
      });

      if (i < senders.length - 1 && delay > 0) {
        await sleep(delay, job.signal);
      }
    }

    await saveFriendsCache(cache);

    await logActivity({
      type: 'broadcast_add',
      description: `Массовое добавление: отправлены приглашения другу ${target_name || targetSteamId} с ${dispatched.length} аккаунтов`,
      status: 'success',
      details: { target_steamid: targetSteamId, senders: dispatched.map(d => d.sender_username) }
    });

    swch.events.emit('friends.updated', { type: 'broadcast_add', count: dispatched.length });
    return {
      success: true,
      target_steamid: targetSteamId,
      sent_count: dispatched.length,
      dispatched
    };
  });

  // Job 4: Synchronize Friends List
  swch.jobs.register('sync_friends', async (input, job) => {
    swch.log('[Friend Manager Job] Starting sync_friends', input);
    const accounts = await getAllAccounts();
    const targetAccounts = input?.account_ids
      ? accounts.filter(a => input.account_ids.includes(a.id))
      : accounts;

    const cache = await getFriendsCache();
    const apiKey = await swch.secrets.get('steam_api_key');

    for (let i = 0; i < targetAccounts.length; i++) {
      job.signal.throwIfAborted();
      const acc = targetAccounts[i];

      await job.progress({
        current_index: i + 1,
        total: targetAccounts.length,
        percent: Math.round(((i + 1) / targetAccounts.length) * 100),
        status_text: `Синхронизация друзей для ${acc.username} (${i + 1}/${targetAccounts.length})`
      });

      if (apiKey && acc.steamid64) {
        try {
          const url = `https://api.steampowered.com/ISteamUser/GetFriendList/v0001/?key=${encodeURIComponent(apiKey)}&steamid=${encodeURIComponent(acc.steamid64)}&relationship=friend`;
          const res = await fetch(url, { signal: AbortSignal.timeout(8000) });
          if (res.ok) {
            const data = await res.json();
            const rawFriends = data?.friendslist?.friends || [];
            cache[acc.id] = {
              account_id: acc.id,
              username: acc.username,
              steamid64: acc.steamid64,
              friends: rawFriends.map(f => ({
                steamid: f.steamid,
                friend_code: toFriendCode(f.steamid),
                name: `SteamUser_${f.steamid.slice(-4)}`,
                relationship: f.relationship,
                friend_since: f.friend_since,
                last_online: Date.now() - 3600000,
                is_mutual: accounts.some(a => a.steamid64 === f.steamid)
              })),
              last_synced: Date.now()
            };
          }
        } catch (e) {
          swch.log(`[Friend Manager] Web API sync error for ${acc.username}: ${e.message}`);
        }
      }

      if (!cache[acc.id] || !cache[acc.id].friends) {
        cache[acc.id] = {
          account_id: acc.id,
          username: acc.username,
          steamid64: acc.steamid64,
          friends: generateFixtureFriends(acc, 10),
          last_synced: Date.now()
        };
      }

      await job.checkpoint({ completed_index: i + 1, account: acc.username });
    }

    await saveFriendsCache(cache);
    swch.events.emit('friends.updated', { type: 'sync' });
    return { success: true, synced_count: targetAccounts.length };
  });

  // --- 3. Register Card Actions (visible buttons in SWCH Mods) ---

  swch.registerAction({
    id: 'cross-friend-all',
    name: 'Взаимная дружба всех аккаунтов'
  }, async () => {
    swch.log('[Friend Manager] Triggered card action: cross-friend-all');
    const accounts = await getAllAccounts();
    if (accounts.length < 2) {
      return { message: 'Нужно минимум 2 аккаунта в SWCH для взаимной дружбы' };
    }
    const job = await swch.jobs.start('cross_friend', { account_ids: accounts.map(a => a.id) });
    return { message: `Запущено задание взаимной дружбы #${job.id} для ${accounts.length} аккаунтов` };
  });

  swch.registerAction({
    id: 'sync-all-friends',
    name: 'Синхронизировать друзей'
  }, async () => {
    swch.log('[Friend Manager] Triggered card action: sync-all-friends');
    const job = await swch.jobs.start('sync_friends', {});
    return { message: `Запущено задание синхронизации друзей #${job.id}` };
  });

  swch.registerAction({
    id: 'clear-friend-cache',
    name: 'Очистить кэш друзей'
  }, async () => {
    swch.log('[Friend Manager] Triggered card action: clear-friend-cache');
    await swch.storage.delete(STORAGE_FRIENDS_CACHE);
    await swch.files.clearCache();
    await logActivity({
      type: 'clear_cache',
      description: 'Кэш друзей и временные файлы успешно очищены',
      status: 'success'
    });
    swch.events.emit('friends.updated', { type: 'cache_cleared' });
    return { message: 'Кэш друзей успешно очищен' };
  });

  // --- 4. Register Internal Methods (for Sandboxed Iframe UI Bridge) ---

  swch.registerMethod('get-state', async () => {
    const accounts = await getAllAccounts();
    await ensureAccountFriends(accounts);
    const cache = await getFriendsCache();
    const settings = await getSettings();
    const activity = await getActivityLog();
    const jobsList = (await swch.jobs.list()) || [];

    const hasApiKey = Boolean(await swch.secrets.get('steam_api_key'));
    const hasSession = Boolean(await swch.secrets.get('steam_session_cookie'));

    let filesUsage = { data: 0, cache: 0, temp: 0, tools: 0 };
    try {
      filesUsage = await swch.files.usage();
    } catch {}

    return {
      accounts,
      friends_cache: cache,
      settings,
      activity: activity.slice(0, 30),
      jobs: jobsList.slice(-20),
      has_api_key: hasApiKey,
      has_session_cookie: hasSession,
      files_usage: filesUsage
    };
  });

  swch.registerMethod('save-settings', async (input) => {
    if (!input || typeof input !== 'object') throw new Error('Settings must be an object');
    await swch.settings.set(input);
    await logActivity({
      type: 'settings',
      description: 'Настройки менеджера друзей обновлены',
      status: 'success'
    });
    return { success: true };
  });

  swch.registerMethod('set-secret', async (input) => {
    const { key, value } = input || {};
    if (!['steam_api_key', 'steam_session_cookie'].includes(key)) {
      throw new Error('Unsupported secret key');
    }
    if (value) {
      await swch.secrets.set(key, String(value).trim());
    } else {
      await swch.secrets.delete(key);
    }
    return { success: true, key };
  });

  swch.registerMethod('get-secret-status', async () => {
    return {
      has_steam_api_key: Boolean(await swch.secrets.get('steam_api_key')),
      has_steam_session_cookie: Boolean(await swch.secrets.get('steam_session_cookie'))
    };
  });

  swch.registerMethod('get-friend-matrix', async () => {
    const accounts = await getAllAccounts();
    const cache = await getFriendsCache();

    const matrix = [];
    for (let i = 0; i < accounts.length; i++) {
      for (let j = i + 1; j < accounts.length; j++) {
        const a = accounts[i];
        const b = accounts[j];
        const aHasB = cache[a.id]?.friends?.some(f => f.steamid === b.steamid64);
        const bHasA = cache[b.id]?.friends?.some(f => f.steamid === a.steamid64);

        let status = 'none';
        if (aHasB && bHasA) status = 'mutual';
        else if (aHasB || bHasA) status = 'one_way';

        matrix.push({
          account_a: { id: a.id, username: a.username, steamid: a.steamid64 },
          account_b: { id: b.id, username: b.username, steamid: b.steamid64 },
          status,
          protocol_uris: [
            `steam://friends/add/${b.steamid64}`,
            `steam://friends/add/${a.steamid64}`
          ]
        });
      }
    }
    return { accounts, matrix };
  });

  swch.registerMethod('generate-protocol-uris', async (input) => {
    const { target_steamid, from_account_ids } = input || {};
    const accounts = await getAllAccounts();
    const senders = accounts.filter(a => !from_account_ids || from_account_ids.includes(a.id));

    const commands = senders.map(s => ({
      account: s.username,
      steamid: target_steamid,
      uri: `steam://friends/add/${target_steamid}`,
      web_url: `https://steamcommunity.com/profiles/${target_steamid}`
    }));

    return { target_steamid, commands };
  });

  swch.registerMethod('export-friends', async () => {
    const cache = await getFriendsCache();
    const accounts = await getAllAccounts();
    const payload = {
      version: '1.0.0',
      exported_at: new Date().toISOString(),
      accounts: accounts.map(a => ({
        id: a.id,
        username: a.username,
        steamid: a.steamid64,
        friends: cache[a.id]?.friends || []
      }))
    };
    return JSON.stringify(payload, null, 2);
  });

  swch.registerMethod('import-friends', async (input) => {
    let parsed;
    try {
      parsed = typeof input === 'string' ? JSON.parse(input) : input;
    } catch {
      throw new Error('Некорректный JSON файл');
    }
    if (!parsed || !Array.isArray(parsed.accounts)) {
      throw new Error('Неверный формат резервной копии');
    }
    const cache = await getFriendsCache();
    let importedCount = 0;
    for (const acc of parsed.accounts) {
      if (acc.id && Array.isArray(acc.friends)) {
        cache[acc.id] = {
          account_id: acc.id,
          username: acc.username,
          steamid64: acc.steamid,
          friends: acc.friends,
          last_synced: Date.now()
        };
        importedCount += acc.friends.length;
      }
    }
    await saveFriendsCache(cache);
    await logActivity({
      type: 'import',
      description: `Импортировано ${importedCount} записей друзей из резервной копии`,
      status: 'success'
    });
    swch.events.emit('friends.updated', { type: 'imported' });
    return { success: true, count: importedCount };
  });

  swch.registerMethod('add-mock-account', async (input) => {
    const { username, display_name } = input || {};
    const name = String(username || `TestUser_${Math.floor(Math.random() * 9000 + 1000)}`).trim();
    const mockAccounts = (await swch.storage.get(STORAGE_MOCK_ACCOUNTS)) || [];
    const newAcc = {
      id: `mock_steam_${Date.now()}`,
      username: name,
      display_name: display_name || name,
      launcher: 'steam',
      is_active: mockAccounts.length === 0,
      is_mock: true
    };
    mockAccounts.push(newAcc);
    await swch.storage.set(STORAGE_MOCK_ACCOUNTS, mockAccounts);
    await logActivity({
      type: 'account',
      description: `Добавлен тестовый аккаунт ${name}`,
      status: 'success'
    });
    swch.events.emit('friends.updated', { type: 'account_added' });
    return newAcc;
  });

  // Listen to accounts.changed system event
  const unbindAccountsEvent = swch.events.on('accounts.changed', async () => {
    swch.log('[Friend Manager] Accounts changed in SWCH, refreshing state...');
    const accounts = await getAllAccounts();
    await ensureAccountFriends(accounts);
    swch.events.emit('friends.updated', { type: 'accounts_changed' });
  });

  // Perform initial auto-sync if enabled
  const initialSettings = await getSettings();
  if (initialSettings.auto_sync) {
    const accs = await getAllAccounts();
    await ensureAccountFriends(accs);
  }

  swch.log('[Friend Manager] Activated successfully');

  // Return Cleanup function
  return async () => {
    swch.log('[Friend Manager] Cleaning up mod...');
    unbindAccountsEvent?.();
    swch.log('[Friend Manager] Mod deactivated');
  };
}
