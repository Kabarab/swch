import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createSDK } from '../mod-sdk/index.mjs';
import { inspectDirectory, inspectZipFile, writeZip } from '../mod-sdk/validation.mjs';

const clone = val => structuredClone(val);

function createMockHost() {
  const store = new Map();
  const secrets = new Map();
  const settingsStore = { value: {} };
  const records = new Map();
  const accounts = [
    { id: 'steam_acc_1', username: 'MainGamer', display_name: 'Main', launcher: 'steam', is_active: true },
    { id: 'steam_acc_2', username: 'SmurfPro', display_name: 'Smurf', launcher: 'steam', is_active: false },
    { id: 'steam_acc_3', username: 'AltHero', display_name: 'Alt', launcher: 'steam', is_active: false }
  ];

  let bridge;
  const registrations = { launchers: [], tabs: [], actions: [], job_types: [] };

  const changed = job => queueMicrotask(() => void bridge.onEvent('jobs.changed', clone(job)));

  bridge = createSDK(async (method, params) => {
    // Accounts & Library
    if (method === 'accounts.list') return clone(accounts);
    if (method === 'library.list') return [];
    if (method === 'library.refreshSteam') return { success: true };

    // Storage
    if (method === 'storage.get') return store.has(params.key) ? clone(store.get(params.key)) : null;
    if (method === 'storage.set') { store.set(params.key, clone(params.value)); return null; }
    if (method === 'storage.delete') { store.delete(params.key); return null; }

    // Secrets
    if (method === 'secrets.get') return secrets.has(params.key) ? secrets.get(params.key) : null;
    if (method === 'secrets.set') { secrets.set(params.key, String(params.value)); return null; }
    if (method === 'secrets.delete') { secrets.delete(params.key); return null; }

    // Settings
    if (method === 'settings.get') return clone(settingsStore.value);
    if (method === 'settings.set') { settingsStore.value = clone(params.value); return null; }

    // Files
    if (method === 'files.paths') return { data: 'C:/temp/data', cache: 'C:/temp/cache', temp: 'C:/temp/temp', tools: 'C:/temp/tools' };
    if (method === 'files.usage') return { data: 4096, cache: 2048, temp: 0, tools: 0 };
    if (method === 'files.clearCache') return null;

    // Clipboard
    if (method === 'clipboard.writeText') return null;
    if (method === 'clipboard.readText') return '';

    // Jobs Host
    if (method === 'jobs.recover') {
      return clone([...records.values()].filter(j => j.status === 'queued'));
    }
    if (method === 'jobs.list') return clone([...records.values()]);
    if (method === 'jobs.start') {
      const job = {
        id: `job-${records.size + 1}`,
        type: params.type,
        input: params.input,
        status: 'queued',
        progress: null,
        checkpoint: null,
        result: null,
        error: null,
        attempt: 0,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString()
      };
      records.set(job.id, job);
      changed(job);
      return clone(job);
    }
    if (method === 'jobs.get') {
      const job = records.get(params.id);
      assert.ok(job, `Job not found: ${params.id}`);
      return clone(job);
    }
    if (method === 'jobs.cancel') {
      const job = records.get(params.id);
      if (job && !['completed', 'failed', 'cancelled'].includes(job.status)) {
        job.status = 'cancelled';
        job.updated_at = new Date().toISOString();
        changed(job);
      }
      return clone(job);
    }
    if (method === 'jobs.update') {
      const job = records.get(params.id);
      assert.ok(job, `Missing job: ${params.id}`);
      if (!['completed', 'failed', 'cancelled'].includes(job.status)) {
        if (params.status === 'running' && job.status !== 'running') job.attempt++;
        for (const [k, v] of Object.entries(params)) {
          if (k !== 'id') job[k] = clone(v);
        }
        job.updated_at = new Date().toISOString();
        changed(job);
      }
      return clone(job);
    }

    throw new Error(`Unexpected SDK method: ${method}`);
  }, () => {}, registrations);

  return { bridge, registrations, records, store, secrets, settingsStore, accounts };
}

async function waitForJobCompletion(records, jobId, timeoutMs = 4000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const job = records.get(jobId);
    if (job && ['completed', 'failed', 'cancelled'].includes(job.status)) {
      return job;
    }
    await new Promise(r => setImmediate(r));
  }
  throw new Error(`Job ${jobId} timed out. Last state: ${JSON.stringify(records.get(jobId))}`);
}

test('Friend Manager: package validates and packages into clean ZIP', async () => {
  const modDir = path.resolve('mods/friend-manager');
  const { manifest, files } = inspectDirectory(modDir);

  assert.equal(manifest.id, 'swch.friend-manager');
  assert.equal(manifest.api, '^1.1.0');
  assert.equal(manifest.backend, 'backend.mjs');
  assert.equal(manifest.ui, 'index.html');
  assert.equal(manifest.theme, 'theme.json');
  assert.ok(files.has('backend.mjs'));
  assert.ok(files.has('index.html'));
  assert.ok(files.has('style.css'));
  assert.ok(files.has('app.mjs'));
  assert.ok(files.has('theme.json'));

  const tempZip = path.resolve('dist/test-friend-manager.zip');
  const packed = await writeZip(files, tempZip);
  assert.ok(fs.existsSync(packed.path));
  assert.equal(typeof packed.sha256, 'string');
  assert.equal(packed.sha256.length, 64);

  const inspected = await inspectZipFile(tempZip);
  assert.equal(inspected.manifest.id, 'swch.friend-manager');
  assert.equal(inspected.sha256, packed.sha256);
  fs.rmSync(tempZip, { force: true });
});

test('Friend Manager: activate registers tab, actions, methods, and jobs', async () => {
  const { bridge, registrations, records } = createMockHost();
  const backendUrl = pathToFileURL(path.resolve('mods/friend-manager/backend.mjs')).href;
  const { default: activate } = await import(backendUrl);

  const cleanup = await activate(bridge.sdk);
  assert.equal(typeof cleanup, 'function', 'activate should return a cleanup function');

  // Verify tab registration
  assert.deepEqual(registrations.tabs, [{ id: 'friends', name: 'Менеджер друзей' }]);

  // Verify card actions registration
  const actionIds = registrations.actions.map(a => a.id);
  assert.ok(actionIds.includes('cross-friend-all'));
  assert.ok(actionIds.includes('sync-all-friends'));
  assert.ok(actionIds.includes('clear-friend-cache'));

  // Verify durable background job types registration
  assert.ok(registrations.job_types.includes('cross_friend'));
  assert.ok(registrations.job_types.includes('batch_remove_friends'));
  assert.ok(registrations.job_types.includes('broadcast_add_friend'));
  assert.ok(registrations.job_types.includes('sync_friends'));

  // Cleanup
  await cleanup();
});

test('Friend Manager: cross_friend job connects all accounts with checkpoints', async () => {
  const { bridge, records } = createMockHost();
  const backendUrl = pathToFileURL(path.resolve('mods/friend-manager/backend.mjs')).href;
  const { default: activate } = await import(backendUrl);
  const cleanup = await activate(bridge.sdk);

  await bridge.recoverJobs();

  // Start cross_friend job with minimal delay for fast test
  const job = await bridge.sdk.jobs.start('cross_friend', {
    account_ids: ['steam_acc_1', 'steam_acc_2', 'steam_acc_3'],
    delay_ms: 10
  });

  const completed = await waitForJobCompletion(records, job.id);
  assert.equal(completed.status, 'completed');
  assert.equal(completed.result.success, true);
  assert.equal(completed.result.accounts_count, 3);
  assert.equal(completed.result.total_pairs, 6); // 3 * 2 ordered pairs
  assert.equal(completed.result.pairs.length, 6);

  // Check state via internal method get-friend-matrix
  const matrix = await bridge.dispatch('method', { id: 'get-friend-matrix', input: {} });
  assert.equal(matrix.accounts.length, 3);
  assert.equal(matrix.matrix.length, 3); // 3 unique undirected pairs (1-2, 1-3, 2-3)
  for (const pair of matrix.matrix) {
    assert.equal(pair.status, 'mutual', 'Pairs should be mutually connected');
  }

  await cleanup();
});

test('Friend Manager: batch_remove_friends job removes selected friends in batches', async () => {
  const { bridge, records } = createMockHost();
  const backendUrl = pathToFileURL(path.resolve('mods/friend-manager/backend.mjs')).href;
  const { default: activate } = await import(backendUrl);
  const cleanup = await activate(bridge.sdk);

  await bridge.recoverJobs();

  // Initialize state so account has friends
  const state = await bridge.dispatch('method', { id: 'get-state', input: {} });
  const accountId = 'steam_acc_1';
  const initialFriends = state.friends_cache[accountId].friends;
  assert.ok(initialFriends.length >= 5);

  const toRemove = [initialFriends[0].steamid, initialFriends[1].steamid, initialFriends[2].steamid];

  const job = await bridge.sdk.jobs.start('batch_remove_friends', {
    account_id: accountId,
    friend_ids: toRemove,
    delay_ms: 5
  });

  const completed = await waitForJobCompletion(records, job.id);
  assert.equal(completed.status, 'completed');
  assert.equal(completed.result.removed_count, 3);

  // Verify removed friends are no longer in account's friends list
  const updatedState = await bridge.dispatch('method', { id: 'get-state', input: {} });
  const remainingSteamIds = updatedState.friends_cache[accountId].friends.map(f => f.steamid);
  for (const removedId of toRemove) {
    assert.ok(!remainingSteamIds.includes(removedId), `Friend ${removedId} should be removed`);
  }

  await cleanup();
});

test('Friend Manager: broadcast_add_friend job dispatches invites from multiple accounts', async () => {
  const { bridge, records } = createMockHost();
  const backendUrl = pathToFileURL(path.resolve('mods/friend-manager/backend.mjs')).href;
  const { default: activate } = await import(backendUrl);
  const cleanup = await activate(bridge.sdk);

  await bridge.recoverJobs();

  const targetSteamId = '76561198999888777';
  const job = await bridge.sdk.jobs.start('broadcast_add_friend', {
    target_input: targetSteamId,
    target_name: 'TargetBuddy',
    from_account_ids: ['steam_acc_1', 'steam_acc_2'],
    delay_ms: 5
  });

  const completed = await waitForJobCompletion(records, job.id);
  assert.equal(completed.status, 'completed');
  assert.equal(completed.result.sent_count, 2);
  assert.equal(completed.result.target_steamid, targetSteamId);

  // Verify both accounts now have the target in their friends list
  const state = await bridge.dispatch('method', { id: 'get-state', input: {} });
  assert.ok(state.friends_cache['steam_acc_1'].friends.some(f => f.steamid === targetSteamId));
  assert.ok(state.friends_cache['steam_acc_2'].friends.some(f => f.steamid === targetSteamId));

  await cleanup();
});

test('Friend Manager: export and import backup preserves friend data', async () => {
  const { bridge } = createMockHost();
  const backendUrl = pathToFileURL(path.resolve('mods/friend-manager/backend.mjs')).href;
  const { default: activate } = await import(backendUrl);
  const cleanup = await activate(bridge.sdk);

  // Get initial state
  await bridge.dispatch('method', { id: 'get-state', input: {} });

  // Export
  const exported = await bridge.dispatch('method', { id: 'export-friends', input: {} });
  assert.equal(typeof exported, 'string');
  const parsed = JSON.parse(exported);
  assert.equal(parsed.version, '1.0.0');
  assert.ok(Array.isArray(parsed.accounts));

  // Modify and Import
  parsed.accounts[0].friends.push({
    steamid: '76561198111222333',
    name: 'BackupImportedFriend',
    relationship: 'friend',
    friend_since: 12345678
  });

  const importResult = await bridge.dispatch('method', { id: 'import-friends', input: JSON.stringify(parsed) });
  assert.equal(importResult.success, true);

  const state = await bridge.dispatch('method', { id: 'get-state', input: {} });
  const accFriends = state.friends_cache[parsed.accounts[0].id].friends;
  assert.ok(accFriends.some(f => f.steamid === '76561198111222333'));

  await cleanup();
});

test('Friend Manager: card actions and settings work properly', async () => {
  const { bridge, records } = createMockHost();
  const backendUrl = pathToFileURL(path.resolve('mods/friend-manager/backend.mjs')).href;
  const { default: activate } = await import(backendUrl);
  const cleanup = await activate(bridge.sdk);

  await bridge.recoverJobs();

  // Test card action: cross-friend-all
  const actionRes = await bridge.dispatch('action', { id: 'cross-friend-all', input: {} });
  assert.match(actionRes.message, /Запущено задание/);

  // Test settings save
  await bridge.dispatch('method', { id: 'save-settings', input: { request_delay_ms: 1500, simulated_mode: true } });
  const state = await bridge.dispatch('method', { id: 'get-state', input: {} });
  assert.equal(state.settings.request_delay_ms, 1500);
  assert.equal(state.settings.simulated_mode, true);

  // Test secret set and status
  await bridge.dispatch('method', { id: 'set-secret', input: { key: 'steam_api_key', value: 'TEST_SECRET_API_KEY_123' } });
  const secretStatus = await bridge.dispatch('method', { id: 'get-secret-status', input: {} });
  assert.equal(secretStatus.has_steam_api_key, true);

  // Test clear cache action
  const clearRes = await bridge.dispatch('action', { id: 'clear-friend-cache', input: {} });
  assert.equal(clearRes.message, 'Кэш друзей успешно очищен');

  await cleanup();
});
