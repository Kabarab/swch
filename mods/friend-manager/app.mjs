/**
 * SWCH Friend Manager - Frontend Application
 * Handles UI interactions, tab switching, friend matrix, batch unfriend,
 * broadcast friend invites, durable background job monitoring, and settings.
 */

// Fallback bridge for standalone browser testing outside SWCH iframe
const swch = window.swch || {
  tab: 'friends',
  call: async (method, params) => {
    console.warn(`[Mock UIBridge] call('${method}',`, params, ')');
    if (method === 'method' && params?.id === 'get-state') {
      return {
        accounts: [
          { id: 'mock_1', username: 'MainAccount', display_name: 'ProGamer', launcher: 'steam', steamid64: '76561198000000001', friend_code: '39734273' },
          { id: 'mock_2', username: 'SmurfAccount', display_name: 'SmurfOne', launcher: 'steam', steamid64: '76561198000000002', friend_code: '39734274' }
        ],
        friends_cache: {
          mock_1: {
            friends: [
              { steamid: '76561198000000002', name: 'SmurfOne', friend_code: '39734274', relationship: 'friend', last_online: Date.now(), is_mutual: true },
              { steamid: '76561198000000099', name: 'артем', friend_code: '337700012', relationship: 'friend', last_online: Date.now() - 40 * 86400 * 1000, is_mutual: false }
            ]
          }
        },
        settings: { request_delay_ms: 1200, simulated_mode: false },
        activity: [],
        jobs: [],
        has_api_key: false,
        has_session_cookie: false,
        files_usage: { data: 1024, cache: 512, temp: 0, tools: 0 }
      };
    }
    if (method === 'method' && params?.id === 'get-friend-matrix') {
      return { accounts: [], matrix: [] };
    }
    if (method === 'jobs.list') return [];
    return {};
  },
  on: (event, listener) => () => {}
};

// Application State
let appState = {
  accounts: [],
  friends_cache: {},
  settings: {},
  activity: [],
  jobs: [],
  has_api_key: false,
  has_session_cookie: false,
  files_usage: { data: 0, cache: 0, temp: 0, tools: 0 }
};

let activeTab = 'cross';
let batchFilter = 'all';
let batchSearchQuery = '';
let selectedBatchFriendIds = new Set();
let activeJobId = null;

// DOM Element references
const els = {
  // Navigation
  tabBtns: document.querySelectorAll('.tab-btn'),
  tabPanels: document.querySelectorAll('.tab-panel'),

  // Stats
  statAccounts: document.getElementById('stat-accounts-count'),
  statMutual: document.getElementById('stat-mutual-count'),
  statFriends: document.getElementById('stat-friends-count'),
  statJobs: document.getElementById('stat-jobs-count'),

  // Active Job Banner
  jobBanner: document.getElementById('active-job-banner'),
  jobBannerDesc: document.getElementById('job-banner-desc'),
  jobBannerProgress: document.getElementById('job-banner-progress'),
  jobBannerStatusText: document.getElementById('job-banner-status-text'),
  jobBannerPercent: document.getElementById('job-banner-percent'),
  btnCancelJob: document.getElementById('btn-cancel-active-job'),

  // Cross Friending
  crossMatrixBody: document.getElementById('cross-matrix-body'),
  crossPairsSummary: document.getElementById('cross-pairs-summary'),
  btnRunCross: document.getElementById('btn-run-cross-friend'),
  btnCopyCrossUris: document.getElementById('btn-copy-cross-uris'),
  btnQuickCross: document.getElementById('btn-quick-cross'),
  btnSyncAll: document.getElementById('btn-sync-all'),

  // Batch Unfriend
  batchSessionBanner: document.getElementById('batch-session-banner'),
  batchStatusIcon: document.getElementById('batch-status-icon'),
  batchStatusTitle: document.getElementById('batch-status-title'),
  batchStatusDesc: document.getElementById('batch-status-desc'),
  btnBatchOpenSteamManage: document.getElementById('btn-batch-open-steam-manage'),
  btnBatchSetupSession: document.getElementById('btn-batch-setup-session'),
  batchAccountSelect: document.getElementById('batch-account-select'),
  batchSearchInput: document.getElementById('batch-search-input'),
  batchFilterBtns: document.querySelectorAll('[data-filter]'),
  batchFriendsBody: document.getElementById('batch-friends-body'),
  batchMasterCheckbox: document.getElementById('batch-master-checkbox'),
  batchSelectedCount: document.getElementById('batch-selected-count'),
  btnBatchSelectAll: document.getElementById('btn-batch-select-all'),
  btnBatchInvert: document.getElementById('btn-batch-invert'),
  btnBatchDelete: document.getElementById('btn-batch-delete-selected'),

  // Broadcast
  broadcastModeRadios: document.querySelectorAll('input[name="broadcast-mode"]'),
  broadcastInputBox: document.getElementById('broadcast-input-box'),
  broadcastCloneBox: document.getElementById('broadcast-clone-box'),
  broadcastTargetInput: document.getElementById('broadcast-target-input'),
  broadcastTargetName: document.getElementById('broadcast-target-name'),
  broadcastSourceAccount: document.getElementById('broadcast-source-account'),
  broadcastSourceFriend: document.getElementById('broadcast-source-friend'),
  broadcastSendersList: document.getElementById('broadcast-senders-list'),
  btnRunBroadcast: document.getElementById('btn-run-broadcast'),

  // Lists & Inspector
  listsAccountSelect: document.getElementById('lists-account-select'),
  listsFriendsBody: document.getElementById('lists-friends-body'),
  btnExportJson: document.getElementById('btn-export-json'),
  btnCopyClipboard: document.getElementById('btn-copy-clipboard'),
  btnImportJson: document.getElementById('btn-import-json'),

  // Jobs & Logs
  jobsListBody: document.getElementById('jobs-list-body'),
  activityLogBody: document.getElementById('activity-log-body'),

  // Settings
  apiKeyInput: document.getElementById('setting-steam-api-key'),
  sessionInput: document.getElementById('setting-steam-session'),
  delayInput: document.getElementById('setting-delay'),
  simulatedModeCheckbox: document.getElementById('setting-simulated-mode'),
  btnSaveApiKey: document.getElementById('btn-save-api-key'),
  btnSaveSession: document.getElementById('btn-save-session'),
  btnSaveSettings: document.getElementById('btn-save-general-settings'),
  badgeApiKey: document.getElementById('badge-api-key-status'),
  badgeSession: document.getElementById('badge-session-status'),
  btnTestSession: document.getElementById('btn-test-session'),
  sessionTestResult: document.getElementById('session-test-result'),
  btnCopyCookieSnippet: document.getElementById('btn-copy-cookie-snippet'),

  // File management
  usageData: document.getElementById('usage-data'),
  usageCache: document.getElementById('usage-cache'),
  usageTemp: document.getElementById('usage-temp'),
  btnOpenData: document.getElementById('btn-open-data-folder'),
  btnClearCache: document.getElementById('btn-clear-cache'),
  btnAddMockAccount: document.getElementById('btn-add-mock-account'),

  // Confirmation Modal
  confirmModal: document.getElementById('confirm-modal'),
  modalTitle: document.getElementById('modal-title'),
  modalMessage: document.getElementById('modal-message'),
  btnModalConfirm: document.getElementById('btn-modal-confirm'),
  btnModalCancel: document.getElementById('btn-modal-cancel'),

  // Toast
  toastContainer: document.getElementById('toast-container')
};

// Formatting Utilities
function formatBytes(bytes = 0) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
}

function formatDate(epochSec) {
  if (!epochSec) return '—';
  const d = new Date(Number(epochSec) * 1000);
  return d.toLocaleDateString('ru-RU', { day: '2-digit', month: '2-digit', year: 'numeric' });
}

function formatRelativeTime(ms) {
  if (!ms) return 'Неизвестно';
  const diff = Date.now() - ms;
  const hours = Math.floor(diff / 3600000);
  if (hours < 1) return 'В сети только что';
  if (hours < 24) return `${hours} ч. назад`;
  const days = Math.floor(hours / 24);
  if (days === 1) return 'Вчера';
  if (days < 30) return `${days} дн. назад`;
  const months = Math.floor(days / 30);
  return `${months} мес. назад`;
}

function getAvatarColor(name = '') {
  const colors = [
    'linear-gradient(135deg, #ff784f, #ff4500)',
    'linear-gradient(135deg, #3b82f6, #1d4ed8)',
    'linear-gradient(135deg, #10b981, #047857)',
    'linear-gradient(135deg, #8b5cf6, #6d28d9)',
    'linear-gradient(135deg, #ec4899, #be185d)',
    'linear-gradient(135deg, #f59e0b, #b45309)'
  ];
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = name.charCodeAt(i) + ((hash << 5) - hash);
  return colors[Math.abs(hash) % colors.length];
}

function createAvatarElement(friend) {
  const avatar = document.createElement('div');
  avatar.className = 'friend-avatar';

  if (friend?.avatar) {
    const img = document.createElement('img');
    img.src = friend.avatar;
    img.alt = friend.name || 'Friend';
    img.loading = 'lazy';
    img.onerror = () => {
      img.remove();
      avatar.style.background = getAvatarColor(friend.name || 'F');
      avatar.textContent = (friend.name || 'F').charAt(0).toUpperCase();
    };
    avatar.appendChild(img);
  } else {
    avatar.style.background = getAvatarColor(friend?.name || 'F');
    avatar.textContent = (friend?.name || 'F').charAt(0).toUpperCase();
  }
  return avatar;
}

// Toast Notifications
function showToast(message, type = 'info') {
  const toast = document.createElement('div');
  toast.className = `toast toast-${type}`;

  const icon = type === 'success' ? '✅' : type === 'error' ? '❌' : type === 'warning' ? '⚠️' : 'ℹ️';
  toast.textContent = `${icon} ${message}`;

  els.toastContainer.appendChild(toast);
  setTimeout(() => {
    toast.style.transition = 'opacity 0.3s ease, transform 0.3s ease';
    toast.style.opacity = '0';
    toast.style.transform = 'translateY(10px)';
    setTimeout(() => toast.remove(), 300);
  }, 4000);
}

// Modal Dialog Confirmation
let activeConfirmResolve = null;

function hideConfirmModal(result = false) {
  if (els.confirmModal) {
    els.confirmModal.hidden = true;
    els.confirmModal.style.display = 'none';
  }
  if (activeConfirmResolve) {
    const resolve = activeConfirmResolve;
    activeConfirmResolve = null;
    resolve(result);
  }
}

function showConfirm(title, message) {
  return new Promise((resolve) => {
    activeConfirmResolve = resolve;
    els.modalTitle.textContent = title;
    els.modalMessage.textContent = message;
    els.confirmModal.hidden = false;
    els.confirmModal.style.display = 'flex';
  });
}

// 1. Tab Switching
function switchTab(tabId) {
  activeTab = tabId;
  els.tabBtns.forEach(btn => {
    const isTarget = btn.id === `tab-btn-${tabId}`;
    btn.classList.toggle('active', isTarget);
    btn.setAttribute('aria-selected', isTarget ? 'true' : 'false');
  });
  els.tabPanels.forEach(panel => {
    const isTarget = panel.id === `panel-${tabId}`;
    panel.hidden = !isTarget;
    panel.style.display = isTarget ? 'block' : 'none';
  });

  if (tabId === 'cross') renderCrossMatrix();
  if (tabId === 'batch') renderBatchFriends();
  if (tabId === 'broadcast') renderBroadcastOptions();
  if (tabId === 'lists') renderListsView();
  if (tabId === 'jobs') renderJobsView();
  if (tabId === 'settings') renderSettings();
}

// 2. Load & Refresh State
async function refreshState() {
  try {
    const res = await swch.call('method', { id: 'get-state', input: {} });
    if (res) {
      appState = res;
      renderOverviewStats();
      renderActiveJob();

      if (activeTab === 'cross') renderCrossMatrix();
      else if (activeTab === 'batch') renderBatchFriends();
      else if (activeTab === 'broadcast') renderBroadcastOptions();
      else if (activeTab === 'lists') renderListsView();
      else if (activeTab === 'jobs') renderJobsView();
      else if (activeTab === 'settings') renderSettings();
    }
  } catch (err) {
    console.error('Failed to refresh state:', err);
  }
}

// 3. Render Overview Statistics
function renderOverviewStats() {
  const accountsCount = appState.accounts.length;
  els.statAccounts.textContent = accountsCount.toString();

  let totalFriends = 0;
  let mutualPairs = 0;
  const friendSet = new Set();

  for (const acc of appState.accounts) {
    const friends = appState.friends_cache[acc.id]?.friends || [];
    totalFriends += friends.length;
    for (const f of friends) {
      if (f.is_mutual) mutualPairs++;
      friendSet.add(f.steamid);
    }
  }

  els.statMutual.textContent = Math.floor(mutualPairs / 2).toString();
  els.statFriends.textContent = friendSet.size.toString();

  const activeJobs = appState.jobs.filter(j => ['queued', 'running'].includes(j.status));
  els.statJobs.textContent = activeJobs.length.toString();
}

// 4. Render Active Job Tracker
function renderActiveJob() {
  const running = appState.jobs.find(j => j.status === 'running' || j.status === 'queued');
  if (!running) {
    els.jobBanner.hidden = true;
    els.jobBanner.style.display = 'none';
    activeJobId = null;
    return;
  }

  activeJobId = running.id;
  els.jobBanner.hidden = false;
  els.jobBanner.style.display = 'flex';

  const typeLabels = {
    cross_friend: 'Взаимная дружба аккаунтов',
    batch_remove_friends: 'Пакетное удаление друзей',
    broadcast_add_friend: 'Массовая рассылка приглашений',
    sync_friends: 'Синхронизация списков друзей'
  };

  els.jobBannerDesc.textContent = typeLabels[running.type] || running.type;
  const progress = running.progress || {};
  const percent = progress.percent || 0;

  els.jobBannerProgress.style.width = `${percent}%`;
  els.jobBannerPercent.textContent = `${percent}%`;
  els.jobBannerStatusText.textContent = progress.status_text || `Выполнение шага (попытка #${running.attempt || 1})`;
}

// 5. Render Tab 1: Cross Friending Matrix
async function renderCrossMatrix() {
  try {
    const data = await swch.call('method', { id: 'get-friend-matrix', input: {} });
    const { accounts, matrix } = data || { accounts: [], matrix: [] };

    els.crossPairsSummary.textContent = `${matrix.length} возможных пар`;
    els.crossMatrixBody.replaceChildren();

    if (matrix.length === 0) {
      const tr = document.createElement('tr');
      const td = document.createElement('td');
      td.colSpan = 5;
      td.className = 'empty-box';
      td.textContent = accounts.length < 2
        ? 'Требуется минимум 2 зарегистрированных аккаунта в SWCH для взаимной дружбы.'
        : 'Нет доступных пар аккаунтов.';
      tr.appendChild(td);
      els.crossMatrixBody.appendChild(tr);
      return;
    }

    for (const pair of matrix) {
      const tr = document.createElement('tr');

      // Account A
      const tdA = document.createElement('td');
      const itemA = document.createElement('div');
      itemA.className = 'friend-item';
      itemA.appendChild(createAvatarElement({ name: pair.account_a.username }));
      const metaA = document.createElement('div');
      metaA.className = 'friend-meta';
      const strongA = document.createElement('span');
      strongA.className = 'friend-name';
      strongA.textContent = pair.account_a.username;
      const smallA = document.createElement('small');
      smallA.textContent = pair.account_a.steamid;
      metaA.append(strongA, smallA);
      itemA.appendChild(metaA);
      tdA.appendChild(itemA);

      // Link Icon
      const tdLink = document.createElement('td');
      tdLink.style.textAlign = 'center';
      tdLink.textContent = pair.status === 'mutual' ? '⇄' : '→';
      tdLink.style.color = pair.status === 'mutual' ? '#4ade80' : 'var(--muted)';
      tdLink.style.fontSize = '18px';

      // Account B
      const tdB = document.createElement('td');
      const itemB = document.createElement('div');
      itemB.className = 'friend-item';
      itemB.appendChild(createAvatarElement({ name: pair.account_b.username }));
      const metaB = document.createElement('div');
      metaB.className = 'friend-meta';
      const strongB = document.createElement('span');
      strongB.className = 'friend-name';
      strongB.textContent = pair.account_b.username;
      const smallB = document.createElement('small');
      smallB.textContent = pair.account_b.steamid;
      metaB.append(strongB, smallB);
      itemB.appendChild(metaB);
      tdB.appendChild(itemB);

      // Friendship Status Badge
      const tdStatus = document.createElement('td');
      const badge = document.createElement('span');
      if (pair.status === 'mutual') {
        badge.className = 'badge badge-success';
        badge.textContent = 'Взаимно в друзьях';
      } else if (pair.status === 'one_way') {
        badge.className = 'badge badge-warning';
        badge.textContent = 'Односторонняя связь';
      } else {
        badge.className = 'badge badge-danger';
        badge.textContent = 'Не связаны';
      }
      tdStatus.appendChild(badge);

      // Action Button
      const tdAction = document.createElement('td');
      const btn = document.createElement('button');
      btn.className = 'btn btn-secondary btn-sm';
      btn.textContent = pair.status === 'mutual' ? '✓ Связаны' : '🔗 Связать пару';
      btn.disabled = pair.status === 'mutual';
      btn.onclick = async () => {
        try {
          const job = await swch.call('jobs.start', {
            type: 'cross_friend',
            input: { account_ids: [pair.account_a.id, pair.account_b.id] }
          });
          showToast(`Задание связи #${job.id} запущено`, 'success');
          await refreshState();
        } catch (e) {
          showToast(`Ошибка запуска: ${e.message}`, 'error');
        }
      };
      tdAction.appendChild(btn);

      tr.append(tdA, tdLink, tdB, tdStatus, tdAction);
      els.crossMatrixBody.appendChild(tr);
    }
  } catch (err) {
    console.error('Failed to render cross matrix:', err);
  }
}

// 6. Render Tab 2: Batch Unfriend (Bulk Remove)
function renderBatchSessionStatus() {
  if (!els.batchStatusIcon) return;
  if (appState.has_session_cookie) {
    els.batchStatusIcon.textContent = '🟢';
    els.batchStatusTitle.textContent = 'Автоматическое удаление: АКТИВНО (Steam Web API)';
    els.batchStatusDesc.textContent = 'Выбранные друзья будут удалены из Steam автоматически в фоновом режиме.';
    els.batchSessionBanner.style.background = 'rgba(34, 197, 94, 0.08)';
    els.batchSessionBanner.style.borderColor = 'rgba(34, 197, 94, 0.3)';
    if (els.btnBatchSetupSession) els.btnBatchSetupSession.style.display = 'none';
  } else {
    els.batchStatusIcon.textContent = '🟡';
    els.batchStatusTitle.textContent = 'Автоматическое удаление: Требуется сессия Steam';
    els.batchStatusDesc.textContent = 'Для прямого удаления без участия пользователя сохраните сессию в Настройках, либо используйте удаление в клиенте Steam.';
    els.batchSessionBanner.style.background = 'rgba(255, 120, 79, 0.08)';
    els.batchSessionBanner.style.borderColor = 'rgba(255, 120, 79, 0.3)';
    if (els.btnBatchSetupSession) els.btnBatchSetupSession.style.display = 'inline-flex';
  }
}

function renderBatchAccountSelect() {
  const current = els.batchAccountSelect.value;
  els.batchAccountSelect.replaceChildren();

  for (const acc of appState.accounts) {
    const opt = document.createElement('option');
    opt.value = acc.id;
    const count = appState.friends_cache[acc.id]?.friends?.length || 0;
    const activeMark = acc.is_active ? ' • Активен' : '';
    opt.textContent = `${acc.display_name || acc.username}${activeMark} (${acc.launcher.toUpperCase()}, ${count} друзей)`;
    els.batchAccountSelect.appendChild(opt);
  }
  if (current && appState.accounts.some(a => a.id === current)) {
    els.batchAccountSelect.value = current;
  }
}

function renderBatchFriends() {
  renderBatchSessionStatus();
  renderBatchAccountSelect();
  const accountId = els.batchAccountSelect.value;
  els.batchFriendsBody.replaceChildren();

  if (!accountId) {
    const tr = document.createElement('tr');
    tr.innerHTML = '<td colspan="6" class="empty-box">Нет доступных аккаунтов</td>';
    els.batchFriendsBody.appendChild(tr);
    return;
  }

  const accountFriends = appState.friends_cache[accountId]?.friends || [];

  // Update filter buttons labels with counts
  const allBtn = document.querySelector('[data-filter="all"]');
  if (allBtn) allBtn.textContent = `Все (${accountFriends.length})`;

  // Filter friends
  const now = Date.now();
  const filtered = accountFriends.filter(f => {
    if (batchSearchQuery) {
      const q = batchSearchQuery.toLowerCase();
      const matchName = f.name?.toLowerCase().includes(q);
      const matchId = f.steamid?.includes(q) || f.friend_code?.includes(q);
      if (!matchName && !matchId) return false;
    }
    if (batchFilter === 'inactive30') {
      return (now - (f.last_online || 0)) > 30 * 86400 * 1000;
    }
    if (batchFilter === 'inactive90') {
      return (now - (f.last_online || 0)) > 90 * 86400 * 1000;
    }
    if (batchFilter === 'non-swch') {
      return !f.is_mutual;
    }
    return true;
  });

  if (filtered.length === 0) {
    const tr = document.createElement('tr');
    tr.innerHTML = '<td colspan="6" class="empty-box">Нет друзей, соответствующих выбранному фильтру</td>';
    els.batchFriendsBody.appendChild(tr);
    updateBatchSelectedCount();
    return;
  }

  for (const f of filtered) {
    const tr = document.createElement('tr');
    tr.style.cursor = 'pointer';

    // Checkbox
    const tdCheck = document.createElement('td');
    const chk = document.createElement('input');
    chk.type = 'checkbox';
    chk.value = f.steamid;
    chk.checked = selectedBatchFriendIds.has(f.steamid);
    chk.addEventListener('change', (e) => {
      e.stopPropagation();
      if (chk.checked) selectedBatchFriendIds.add(f.steamid);
      else selectedBatchFriendIds.delete(f.steamid);
      updateBatchSelectedCount();
    });
    tdCheck.appendChild(chk);

    // Row click toggles checkbox
    tr.addEventListener('click', (e) => {
      if (e.target !== chk) {
        chk.checked = !chk.checked;
        chk.dispatchEvent(new Event('change'));
      }
    });

    // Friend Name & Avatar
    const tdFriend = document.createElement('td');
    const item = document.createElement('div');
    item.className = 'friend-item';
    const avatar = createAvatarElement(f);
    const meta = document.createElement('div');
    meta.className = 'friend-meta';
    const name = document.createElement('span');
    name.className = 'friend-name';
    name.textContent = f.name || 'Пользователь Steam';

    meta.appendChild(name);
    item.append(avatar, meta);
    tdFriend.appendChild(item);

    // SteamID / Friend Code
    const tdId = document.createElement('td');
    const codeEl = document.createElement('code');
    codeEl.textContent = f.friend_code || f.steamid;
    tdId.appendChild(codeEl);

    // Relationship
    const tdRel = document.createElement('td');
    tdRel.textContent = f.relationship === 'friend' ? 'Друг' : (f.relationship || 'Друг');

    // Last Online
    const tdOnline = document.createElement('td');
    tdOnline.textContent = formatRelativeTime(f.last_online);

    // Is Mutual in SWCH
    const tdMutual = document.createElement('td');
    const mutualBadge = document.createElement('span');
    if (f.is_mutual) {
      mutualBadge.className = 'badge badge-success';
      mutualBadge.textContent = 'Взаимный (SWCH)';
    } else {
      mutualBadge.className = 'badge badge-info';
      mutualBadge.textContent = 'Обычный друг';
    }
    tdMutual.appendChild(mutualBadge);

    tr.append(tdCheck, tdFriend, tdId, tdRel, tdOnline, tdMutual);
    els.batchFriendsBody.appendChild(tr);
  }

  updateBatchSelectedCount();
}

function updateBatchSelectedCount() {
  const count = selectedBatchFriendIds.size;
  els.batchSelectedCount.textContent = count.toString();
  els.btnBatchDelete.disabled = count === 0;
}

// 7. Render Tab 3: Broadcast Add Friend
function renderBroadcastOptions() {
  els.broadcastSourceAccount.replaceChildren();
  for (const acc of appState.accounts) {
    const opt = document.createElement('option');
    opt.value = acc.id;
    const count = appState.friends_cache[acc.id]?.friends?.length || 0;
    opt.textContent = `${acc.display_name || acc.username} (${acc.launcher.toUpperCase()}, ${count} друзей)`;
    els.broadcastSourceAccount.appendChild(opt);
  }

  updateBroadcastSourceFriends();

  // Render Senders Checkboxes
  els.broadcastSendersList.replaceChildren();
  for (const acc of appState.accounts) {
    const label = document.createElement('label');
    label.className = 'account-pick-card';

    const chk = document.createElement('input');
    chk.type = 'checkbox';
    chk.name = 'broadcast-sender';
    chk.value = acc.id;
    chk.checked = true;

    const span = document.createElement('span');
    span.style.fontSize = '12px';
    span.style.fontWeight = '600';
    span.textContent = `${acc.display_name || acc.username} (${acc.launcher.toUpperCase()})`;

    label.append(chk, span);
    els.broadcastSendersList.appendChild(label);
  }
}

function updateBroadcastSourceFriends() {
  const sourceId = els.broadcastSourceAccount.value;
  els.broadcastSourceFriend.replaceChildren();
  const friends = appState.friends_cache[sourceId]?.friends || [];

  for (const f of friends) {
    const opt = document.createElement('option');
    opt.value = f.steamid;
    opt.textContent = `${f.name} (Код: ${f.friend_code || f.steamid})`;
    els.broadcastSourceFriend.appendChild(opt);
  }
}

// 8. Render Tab 4: Lists & Inspector
function renderListsView() {
  const current = els.listsAccountSelect.value;
  els.listsAccountSelect.replaceChildren();

  for (const acc of appState.accounts) {
    const opt = document.createElement('option');
    opt.value = acc.id;
    opt.textContent = `${acc.display_name || acc.username} (${acc.launcher.toUpperCase()})`;
    els.listsAccountSelect.appendChild(opt);
  }

  if (current && appState.accounts.some(a => a.id === current)) {
    els.listsAccountSelect.value = current;
  }

  updateListsTable();
}

function updateListsTable() {
  const accountId = els.listsAccountSelect.value;
  els.listsFriendsBody.replaceChildren();

  if (!accountId) {
    const tr = document.createElement('tr');
    tr.innerHTML = '<td colspan="5" class="empty-box">Выберите аккаунт для просмотра</td>';
    els.listsFriendsBody.appendChild(tr);
    return;
  }

  const friends = appState.friends_cache[accountId]?.friends || [];
  if (friends.length === 0) {
    const tr = document.createElement('tr');
    tr.innerHTML = '<td colspan="5" class="empty-box">Список друзей пуст</td>';
    els.listsFriendsBody.appendChild(tr);
    return;
  }

  for (const f of friends) {
    const tr = document.createElement('tr');

    const tdName = document.createElement('td');
    const item = document.createElement('div');
    item.className = 'friend-item';
    item.appendChild(createAvatarElement(f));
    const span = document.createElement('span');
    span.className = 'friend-name';
    span.textContent = f.name;
    item.appendChild(span);
    tdName.appendChild(item);

    const tdCode = document.createElement('td');
    const code = document.createElement('code');
    code.textContent = f.friend_code || f.steamid;
    tdCode.appendChild(code);

    const tdRel = document.createElement('td');
    tdRel.textContent = f.relationship === 'friend' ? 'Друг' : (f.relationship || 'Друг');

    const tdDate = document.createElement('td');
    tdDate.textContent = formatDate(f.friend_since);

    const tdActions = document.createElement('td');
    const btn = document.createElement('button');
    btn.className = 'btn btn-secondary btn-sm';
    btn.textContent = '📋 Код';
    btn.onclick = async () => {
      await swch.call('clipboard.writeText', { text: f.friend_code || f.steamid });
      showToast('Код друга скопирован в буфер', 'success');
    };
    tdActions.appendChild(btn);

    tr.append(tdName, tdCode, tdRel, tdDate, tdActions);
    els.listsFriendsBody.appendChild(tr);
  }
}

// 9. Render Tab 5: Jobs & Logs
function renderJobsView() {
  els.jobsListBody.replaceChildren();
  const jobs = [...appState.jobs].reverse();

  if (jobs.length === 0) {
    const tr = document.createElement('tr');
    tr.innerHTML = '<td colspan="6" class="empty-box">Задания ещё не запускались</td>';
    els.jobsListBody.appendChild(tr);
  } else {
    for (const j of jobs) {
      const tr = document.createElement('tr');

      const tdId = document.createElement('td');
      tdId.textContent = `#${j.id}`;

      const tdType = document.createElement('td');
      tdType.textContent = j.type;

      const tdStatus = document.createElement('td');
      const badge = document.createElement('span');
      if (j.status === 'completed') badge.className = 'badge badge-success';
      else if (j.status === 'running') badge.className = 'badge badge-info';
      else if (j.status === 'failed') badge.className = 'badge badge-danger';
      else badge.className = 'badge badge-warning';
      badge.textContent = j.status;
      tdStatus.appendChild(badge);

      const tdProgress = document.createElement('td');
      tdProgress.textContent = `${j.progress?.percent || (j.status === 'completed' ? 100 : 0)}%`;

      const tdDate = document.createElement('td');
      tdDate.textContent = new Date(j.created_at).toLocaleTimeString('ru-RU');

      const tdActions = document.createElement('td');
      if (['queued', 'running'].includes(j.status)) {
        const btnCancel = document.createElement('button');
        btnCancel.className = 'btn btn-danger btn-sm';
        btnCancel.textContent = 'Отменить';
        btnCancel.onclick = async () => {
          await swch.call('jobs.cancel', { id: j.id });
          showToast(`Задание #${j.id} отменено`, 'warning');
          await refreshState();
        };
        tdActions.appendChild(btnCancel);
      } else {
        tdActions.textContent = '—';
      }

      tr.append(tdId, tdType, tdStatus, tdProgress, tdDate, tdActions);
      els.jobsListBody.appendChild(tr);
    }
  }

  // Render Activity Log
  els.activityLogBody.replaceChildren();
  const logs = appState.activity || [];

  if (logs.length === 0) {
    const tr = document.createElement('tr');
    tr.innerHTML = '<td colspan="4" class="empty-box">Журнал активности пуст</td>';
    els.activityLogBody.appendChild(tr);
  } else {
    for (const l of logs) {
      const tr = document.createElement('tr');

      const tdTime = document.createElement('td');
      tdTime.textContent = new Date(l.timestamp).toLocaleTimeString('ru-RU');

      const tdType = document.createElement('td');
      const typeBadge = document.createElement('span');
      typeBadge.className = 'badge badge-info';
      typeBadge.textContent = l.type;
      tdType.appendChild(typeBadge);

      const tdDesc = document.createElement('td');
      tdDesc.textContent = l.description;

      const tdStatus = document.createElement('td');
      tdStatus.textContent = l.status === 'success' ? '✅' : '⚠️';

      tr.append(tdTime, tdType, tdDesc, tdStatus);
      els.activityLogBody.appendChild(tr);
    }
  }
}

// 10. Render Tab 6: Settings
function renderSettings() {
  els.delayInput.value = appState.settings.request_delay_ms || 1200;
  els.simulatedModeCheckbox.checked = Boolean(appState.settings.simulated_mode);

  if (appState.has_api_key) {
    els.badgeApiKey.className = 'badge badge-success';
    els.badgeApiKey.textContent = '🔒 Сохранён';
  } else {
    els.badgeApiKey.className = 'badge badge-warning';
    els.badgeApiKey.textContent = 'Не задан';
  }

  if (appState.has_session_cookie) {
    els.badgeSession.className = 'badge badge-success';
    els.badgeSession.textContent = '🔒 Активна';
  } else {
    els.badgeSession.className = 'badge badge-warning';
    els.badgeSession.textContent = 'Не задана';
  }

  els.usageData.textContent = formatBytes(appState.files_usage.data);
  els.usageCache.textContent = formatBytes(appState.files_usage.cache);
  els.usageTemp.textContent = formatBytes(appState.files_usage.temp);
}

// Event Listeners Initialization
function initEventListeners() {
  // Tab switching
  els.tabBtns.forEach(btn => {
    btn.addEventListener('click', () => {
      const id = btn.id.replace('tab-btn-', '');
      switchTab(id);
    });
  });

  // Top header quick actions
  els.btnQuickCross.addEventListener('click', async () => {
    try {
      const res = await swch.call('action', { id: 'cross-friend-all', input: {} });
      showToast(res?.message || 'Запущена взаимная дружба', 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка: ${e.message}`, 'error');
    }
  });

  els.btnSyncAll.addEventListener('click', async () => {
    try {
      const res = await swch.call('action', { id: 'sync-all-friends', input: {} });
      showToast(res?.message || 'Синхронизация запущена', 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка: ${e.message}`, 'error');
    }
  });

  // Cancel active job
  els.btnCancelJob.addEventListener('click', async () => {
    if (!activeJobId) return;
    try {
      await swch.call('jobs.cancel', { id: activeJobId });
      showToast('Задание отменяется...', 'warning');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка отмены: ${e.message}`, 'error');
    }
  });

  // Cross Friending Panel
  els.btnRunCross.addEventListener('click', async () => {
    const confirmed = await showConfirm(
      'Взаимная дружба',
      `Связать все ${appState.accounts.length} зарегистрированных аккаунтов между собой? Будут созданы взаимные приглашения.`
    );
    if (!confirmed) return;
    try {
      const job = await swch.call('jobs.start', {
        type: 'cross_friend',
        input: { account_ids: appState.accounts.map(a => a.id) }
      });
      showToast(`Задание взаимной дружбы #${job.id} запущено!`, 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка: ${e.message}`, 'error');
    }
  });

  els.btnCopyCrossUris.addEventListener('click', async () => {
    try {
      const data = await swch.call('method', { id: 'get-friend-matrix', input: {} });
      const uris = [];
      for (const pair of data?.matrix || []) {
        uris.push(...pair.protocol_uris);
      }
      if (uris.length === 0) {
        showToast('Нет доступных пар аккаунтов', 'warning');
        return;
      }
      await swch.call('clipboard.writeText', { text: uris.join('\n') });
      showToast(`Скопировано ${uris.length} ссылок steam:// в буфер обмена`, 'success');
    } catch (e) {
      showToast(`Ошибка копирования: ${e.message}`, 'error');
    }
  });

  // Batch Unfriend Panel
  if (els.btnBatchOpenSteamManage) {
    els.btnBatchOpenSteamManage.addEventListener('click', async () => {
      try {
        await swch.call('method', { id: 'open-steam-manage', input: {} });
        showToast('Открыто управление друзьями в клиенте Steam', 'success');
      } catch (e) {
        showToast(`Ошибка: ${e.message}`, 'error');
      }
    });
  }

  if (els.btnBatchSetupSession) {
    els.btnBatchSetupSession.addEventListener('click', () => {
      switchTab('settings');
      setTimeout(() => els.sessionInput?.focus(), 150);
    });
  }

  els.batchAccountSelect.addEventListener('change', () => {
    selectedBatchFriendIds.clear();
    renderBatchFriends();
  });

  els.batchSearchInput.addEventListener('input', (e) => {
    batchSearchQuery = e.target.value.trim();
    renderBatchFriends();
  });

  els.batchFilterBtns.forEach(btn => {
    btn.addEventListener('click', () => {
      els.batchFilterBtns.forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      batchFilter = btn.dataset.filter;
      renderBatchFriends();
    });
  });

  els.batchMasterCheckbox.addEventListener('change', (e) => {
    const checkboxes = els.batchFriendsBody.querySelectorAll('input[type="checkbox"]');
    checkboxes.forEach(chk => {
      chk.checked = e.target.checked;
      chk.dispatchEvent(new Event('change'));
    });
  });

  els.btnBatchSelectAll.addEventListener('click', () => {
    const accountId = els.batchAccountSelect.value;
    const friends = appState.friends_cache[accountId]?.friends || [];
    friends.forEach(f => selectedBatchFriendIds.add(f.steamid));
    renderBatchFriends();
  });

  els.btnBatchInvert.addEventListener('click', () => {
    const accountId = els.batchAccountSelect.value;
    const friends = appState.friends_cache[accountId]?.friends || [];
    friends.forEach(f => {
      if (selectedBatchFriendIds.has(f.steamid)) selectedBatchFriendIds.delete(f.steamid);
      else selectedBatchFriendIds.add(f.steamid);
    });
    renderBatchFriends();
  });

  els.btnBatchDelete.addEventListener('click', async () => {
    const count = selectedBatchFriendIds.size;
    if (count === 0) return;
    const accountId = els.batchAccountSelect.value;
    const confirmed = await showConfirm(
      'Пакетное удаление друзей',
      `Вы точно хотите удалить ${count} выбранных друзей из аккаунта? Это действие необратимо.`
    );
    if (!confirmed) return;

    try {
      const job = await swch.call('jobs.start', {
        type: 'batch_remove_friends',
        input: {
          account_id: accountId,
          friend_ids: Array.from(selectedBatchFriendIds)
        }
      });
      selectedBatchFriendIds.clear();
      updateBatchSelectedCount();
      showToast(`Задание удаления #${job.id} запущено!`, 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка: ${e.message}`, 'error');
    }
  });

  // Broadcast Mode Switching
  els.broadcastModeRadios.forEach(radio => {
    radio.addEventListener('change', () => {
      const isInput = radio.value === 'input';
      els.broadcastInputBox.hidden = !isInput;
      els.broadcastCloneBox.hidden = isInput;
    });
  });

  els.broadcastSourceAccount.addEventListener('change', () => {
    updateBroadcastSourceFriends();
  });

  els.btnRunBroadcast.addEventListener('click', async () => {
    const mode = document.querySelector('input[name="broadcast-mode"]:checked').value;
    let targetInput = '';
    let targetName = '';

    if (mode === 'input') {
      targetInput = els.broadcastTargetInput.value.trim();
      targetName = els.broadcastTargetName.value.trim();
    } else {
      targetInput = els.broadcastSourceFriend.value;
      const opt = els.broadcastSourceFriend.selectedOptions[0];
      targetName = opt ? opt.textContent : '';
    }

    if (!targetInput) {
      showToast('Укажите код друга или выберите друга для добавления', 'error');
      return;
    }

    const selectedSenders = Array.from(
      document.querySelectorAll('input[name="broadcast-sender"]:checked')
    ).map(chk => chk.value);

    if (selectedSenders.length === 0) {
      showToast('Выберите хотя бы один аккаунт для отправки', 'error');
      return;
    }

    const confirmed = await showConfirm(
      'Массовая рассылка приглашений',
      `Отправить приглашение другу (${targetName || targetInput}) с ${selectedSenders.length} ваших аккаунтов?`
    );
    if (!confirmed) return;

    try {
      const job = await swch.call('jobs.start', {
        type: 'broadcast_add_friend',
        input: {
          target_input: targetInput,
          target_name: targetName,
          from_account_ids: selectedSenders
        }
      });
      showToast(`Задание массового добавления #${job.id} запущено!`, 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка: ${e.message}`, 'error');
    }
  });

  // Lists & Inspector
  els.listsAccountSelect.addEventListener('change', () => {
    updateListsTable();
  });

  els.btnExportJson.addEventListener('click', async () => {
    try {
      const json = await swch.call('method', { id: 'export-friends', input: {} });
      const blob = new Blob([json], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `swch-friends-backup-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
      showToast('Файл резервной копии скачан', 'success');
    } catch (e) {
      showToast(`Ошибка экспорта: ${e.message}`, 'error');
    }
  });

  els.btnCopyClipboard.addEventListener('click', async () => {
    try {
      const json = await swch.call('method', { id: 'export-friends', input: {} });
      await swch.call('clipboard.writeText', { text: json });
      showToast('Резервная копия скопирована в буфер обмена', 'success');
    } catch (e) {
      showToast(`Ошибка копирования: ${e.message}`, 'error');
    }
  });

  els.btnImportJson.addEventListener('click', async () => {
    const input = prompt('Вставьте JSON резервной копии:');
    if (!input) return;
    try {
      const res = await swch.call('method', { id: 'import-friends', input });
      showToast(`Импортировано ${res.count} записей друзей`, 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка импорта: ${e.message}`, 'error');
    }
  });

  // Settings: API Key & Session
  els.btnSaveApiKey.addEventListener('click', async () => {
    const val = els.apiKeyInput.value.trim();
    try {
      await swch.call('method', { id: 'set-secret', input: { key: 'steam_api_key', value: val } });
      els.apiKeyInput.value = '';
      showToast(val ? 'Steam Web API Key сохранён в защищенном Keyring' : 'Ключ удалён', 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка сохранения: ${e.message}`, 'error');
    }
  });

  els.btnSaveSession.addEventListener('click', async () => {
    const val = els.sessionInput.value.trim();
    try {
      await swch.call('method', { id: 'set-secret', input: { key: 'steam_session_cookie', value: val } });
      els.sessionInput.value = '';
      showToast(val ? 'Сессионная кука сохранена в защищенном хранилище' : 'Кука удалена', 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка сохранения: ${e.message}`, 'error');
    }
  });

  if (els.btnTestSession) {
    els.btnTestSession.addEventListener('click', async () => {
      const inputVal = els.sessionInput.value.trim();
      els.sessionTestResult.style.display = 'block';
      els.sessionTestResult.style.background = 'rgba(255, 255, 255, 0.05)';
      els.sessionTestResult.style.color = 'var(--color-text)';
      els.sessionTestResult.textContent = '⏳ Проверка соединения со Steam Community...';

      try {
        const res = await swch.call('method', {
          id: 'test-steam-session',
          input: { session_cookie: inputVal || undefined }
        });

        if (res?.valid) {
          els.sessionTestResult.style.background = 'rgba(34, 197, 94, 0.15)';
          els.sessionTestResult.style.color = '#4ade80';
          els.sessionTestResult.textContent = `✅ Сессия Steam активна! Вы вошли как: ${res.persona_name || 'Steam User'}`;
          showToast('Сессия Steam успешно проверена и активна', 'success');
        } else {
          els.sessionTestResult.style.background = 'rgba(239, 68, 68, 0.15)';
          els.sessionTestResult.style.color = '#f87171';
          els.sessionTestResult.textContent = `❌ ${res?.error || 'Сессия недействительна или истёк срок действия'}`;
          showToast('Сессия Steam недействительна', 'error');
        }
      } catch (err) {
        els.sessionTestResult.style.background = 'rgba(239, 68, 68, 0.15)';
        els.sessionTestResult.style.color = '#f87171';
        els.sessionTestResult.textContent = `❌ Ошибка проверки: ${err.message}`;
      }
    });
  }

  if (els.btnCopyCookieSnippet) {
    els.btnCopyCookieSnippet.addEventListener('click', async () => {
      const code = `navigator.clipboard.writeText(document.cookie); alert("Cookie скопированы в буфер обмена!");`;
      try {
        await swch.call('clipboard.writeText', { text: code });
        showToast('Скрипт скопирован! Вставьте его в консоль (F12) на steamcommunity.com', 'success');
      } catch {
        prompt('Скопируйте скрипт и выполните в консоли браузера на steamcommunity.com:', code);
      }
    });
  }

  els.btnSaveSettings.addEventListener('click', async () => {
    try {
      await swch.call('method', {
        id: 'save-settings',
        input: {
          request_delay_ms: Number(els.delayInput.value) || 1200,
          simulated_mode: els.simulatedModeCheckbox.checked
        }
      });
      showToast('Настройки сохранены', 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка: ${e.message}`, 'error');
    }
  });

  // File management
  els.btnOpenData.addEventListener('click', async () => {
    try {
      const paths = await swch.call('files.paths');
      if (paths?.data) {
        await swch.call('shell.openFolder', { path: paths.data });
      }
    } catch (e) {
      showToast(`Ошибка: ${e.message}`, 'error');
    }
  });

  els.btnClearCache.addEventListener('click', async () => {
    try {
      await swch.call('action', { id: 'clear-friend-cache', input: {} });
      showToast('Кэш очищен', 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка: ${e.message}`, 'error');
    }
  });

  els.btnAddMockAccount.addEventListener('click', async () => {
    const name = prompt('Введите имя тестового аккаунта:', `TestUser_${Math.floor(Math.random() * 900 + 100)}`);
    if (!name) return;
    try {
      await swch.call('method', { id: 'add-mock-account', input: { username: name } });
      showToast(`Тестовый аккаунт ${name} добавлен!`, 'success');
      await refreshState();
    } catch (e) {
      showToast(`Ошибка: ${e.message}`, 'error');
    }
  });

  // Modal dialog buttons and dismissal
  els.btnModalConfirm.addEventListener('click', () => hideConfirmModal(true));
  els.btnModalCancel.addEventListener('click', () => hideConfirmModal(false));
  els.confirmModal.addEventListener('click', (e) => {
    if (e.target === els.confirmModal) {
      hideConfirmModal(false);
    }
  });
  window.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !els.confirmModal.hidden) {
      hideConfirmModal(false);
    }
  });

  // System & Mod Events
  swch.on('jobs.changed', (job) => {
    const idx = appState.jobs.findIndex(j => j.id === job.id);
    if (idx >= 0) appState.jobs[idx] = job;
    else appState.jobs.push(job);
    renderActiveJob();
    renderOverviewStats();
    if (activeTab === 'jobs') renderJobsView();
  });

  swch.on('friends.updated', () => {
    void refreshState();
  });

  swch.on('accounts.changed', () => {
    void refreshState();
  });
}

// Startup
async function init() {
  hideConfirmModal(false);
  initEventListeners();
  await refreshState();
}

init().catch(console.error);
