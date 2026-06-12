// 1. Импорты (все функции из Go)
import {
    RemoveGame,
    GetLibrary,
    ToggleGamePin,
    LaunchGame,
    UpdateGameNote,
    ToggleGameAccountHidden,
    GetLaunchers,
    SwitchToAccount,
    SaveEpicAccount,
    LogoutEpic,
    DeleteAccount,
    SelectImage,
    UpdateAccountData,
    SelectExe,
    AddCustomGame,
    SetGameImage,
    // EPIC NATIVE
    EpicLogin,
    SaveRiotAccount,
    GetClipboardText,
    GetEpicAuthURL,
    AuthorizeEpicWithCode,
    OpenEpicAuthPage,
    SaveRobloxAccount,
    InstallAllEpicGames,
    // PLAYNITE-STYLE LIBRARY
    ToggleGameHidden,
    SetGameCategories,
    SetGameCompletionStatus,
    SetGameUserScore,
    FetchGameMetadata
} from '../wailsjs/go/app/App';

import { EventsOn } from '../wailsjs/runtime/runtime.js';

// --- Глобальные переменные ---
let globalGames = [];
let editingTagTarget = { username: '', platform: '', gameId: '' };
let editingAccountTarget = { username: '', platform: '' };
let currentModalGameId = '';
let showHiddenAccounts = false;

// Переменные для контекстного меню
let selectedGameId = null;
let selectedPlatform = null;

// Категории: состояние модалки редактирования
let editingCategoriesGameId = null;
let editingCategoriesList = [];

// Состояние фильтров
let filterState = {
    disabledPlatforms: [],   // платформы строятся динамически; по умолчанию все включены
    onlyInstalled: false,
    onlyMac: false,
    onlyFavorites: false,
    onlyHidden: false,       // режим "только скрытые игры"
    category: '',
    sortBy: 'name',
    searchQuery: ''
};

// --- Справочники (Playnite-style) ---
const COMPLETION_STATUSES = [
    { value: '', label: 'Без статуса' },
    { value: 'planToPlay', label: 'В планах' },
    { value: 'playing', label: 'Играю' },
    { value: 'played', label: 'Сыграно' },
    { value: 'beaten', label: 'Пройдена' },
    { value: 'completed', label: '100% завершена' },
    { value: 'onHold', label: 'Отложена' },
    { value: 'abandoned', label: 'Заброшена' }
];

const STATUS_COLORS = {
    planToPlay: '#4fa3ff',
    playing: '#3dbd7d',
    played: '#a1a1a1',
    beaten: '#7b85e6',
    completed: '#FFD700',
    onHold: '#999',
    abandoned: '#c92a2a'
};

const PLATFORM_COLORS = {
    'Steam': '#1b2838',
    'Epic': '#333',
    'Riot': '#d13639',
    'Battle.net': '#148eff',
    'Ubisoft Connect': '#0070ff',
    'EA App': '#ff4747',
    'GOG Galaxy': '#86328a',
    'Wargaming.net': '#b30000',
    'Roblox': '#666',
    'Custom': '#555',
    'Torrent': '#555'
};

const PLATFORM_ICONS = {
    'Steam': 'fa-brands fa-steam',
    'Epic': 'fa-solid fa-bolt',
    'Riot': 'fa-solid fa-gamepad',
    'Battle.net': 'fa-solid fa-cloud',
    'Ubisoft Connect': 'fa-solid fa-circle-nodes',
    'EA App': 'fa-solid fa-e',
    'GOG Galaxy': 'fa-solid fa-compact-disc',
    'Wargaming.net': 'fa-solid fa-shield-halved',
    'Roblox': 'fa-solid fa-square',
    'Custom': 'fa-solid fa-folder'
};

function statusLabel(value) {
    const s = COMPLETION_STATUSES.find(s => s.value === value);
    return s ? s.label : value;
}

// --- Форматирование (Playnite-style) ---
function formatPlaytime(seconds) {
    if (!seconds || seconds <= 0) return '—';
    if (seconds < 60) return 'меньше минуты';
    const totalMin = Math.floor(seconds / 60);
    if (totalMin < 60) return `${totalMin} мин`;
    const h = Math.floor(totalMin / 60);
    const m = totalMin % 60;
    return m > 0 ? `${h} ч ${m} мин` : `${h} ч`;
}

function formatPlaytimeShort(seconds) {
    if (!seconds || seconds <= 0) return '';
    const totalMin = Math.floor(seconds / 60);
    if (totalMin < 60) return `${Math.max(totalMin, 1)} мин`;
    return `${Math.floor(totalMin / 60)} ч`;
}

function formatLastPlayed(unix) {
    if (!unix || unix <= 0) return 'никогда';
    const d = new Date(unix * 1000);
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const day = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    const diffDays = Math.round((today - day) / 86400000);
    if (diffDays === 0) return 'сегодня';
    if (diffDays === 1) return 'вчера';
    return d.toLocaleDateString('ru-RU');
}

function escapeHtml(str) {
    return (str || '').replace(/&/g, '&amp;').replace(/</g, '&lt;')
        .replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

// Переключение аккаунта
window.switchAccount = async function (username, platform) {
    console.log('[Frontend] switchAccount called:', username, platform);
    try {
        const result = await SwitchToAccount(username, platform);
        console.log('[Frontend] SwitchToAccount result:', result);
        if (result.startsWith('Error')) {
            alert('Switch error: ' + result);
        } else {
            alert(result);
        }
    } catch (e) {
        console.error('[Frontend] switchAccount exception:', e);
        alert('Exception during account switch: ' + e);
    }
}

// Удаление аккаунта
// Удаление аккаунта
window.prepareDeleteAccount = function (event, username, platform) {
    if (event) event.stopPropagation();
    window.deleteAccount(username, platform);
}

window.deleteAccount = async function (username, platform) {
    console.log('[DEBUG] deleteAccount ENTRY:', username, platform);
    // confirm() is blocked in Wails - skip confirmation for now
    try {
        console.log('[Frontend] Call DeleteAccount:', username, platform);
        const res = await DeleteAccount(username, platform);
        console.log('[Frontend] DeleteAccount Res:', res);
        if (res === "Success") {
            loadAccounts();
        } else {
            alert("Error deleting account: " + res);
        }
    } catch (e) {
        alert('An error occurred while deleting the account: ' + e);
    }
}

// Скрытие аккаунта (Soft Delete)
// Скрытие аккаунта (Soft Delete)
window.prepareHideAccount = function (event, username, platform) {
    if (event) event.stopPropagation();
    window.hideAccount(username, platform);
}

window.hideAccount = async function (username, platform) {
    console.log('[DEBUG] hideAccount called for:', username, platform);
    if (!username || !platform) {
        console.error('[DEBUG] Missing username or platform');
        return;
    }

    // confirm() is blocked in Wails - skip confirmation for now

    const comment = "_swch_hidden_";

    try {
        console.log('[DEBUG] Calling UpdateAccountData...');
        const res = await UpdateAccountData(username, platform, comment, "");
        console.log('[DEBUG] UpdateAccountData result:', res);
        loadAccounts();
    } catch (e) {
        console.error('[DEBUG] hideAccount error:', e);
        alert('An error occurred while hiding the account: ' + e);
    }
}

window.restoreAccount = async function (username, platform) {
    try {
        const res = await UpdateAccountData(username, platform, "", "");
        if (res && (res.startsWith("Error") || res.startsWith("Ошибка"))) {
            alert("Error restoring account: " + res);
        } else {
            loadAccounts();
        }
    } catch (e) {
        alert('An error occurred while restoring the account: ' + e);
    }
}

// Редактирование аккаунта
window.openEditAccount = function (username, platform, currentComment) {
    editingAccountTarget = { username, platform };
    document.getElementById('edit-comment').value = currentComment;
    document.getElementById('edit-avatar-path').value = '';
    document.getElementById('edit-account-modal').style.display = 'flex';
}

window.selectNewAvatar = async function () {
    const path = await SelectImage();
    if (path) document.getElementById('edit-avatar-path').value = path;
}

window.saveAccountChanges = async function () {
    await UpdateAccountData(
        editingAccountTarget.username,
        editingAccountTarget.platform,
        document.getElementById('edit-comment').value,
        document.getElementById('edit-avatar-path').value
    );
    closeModal('edit-account-modal');
    loadAccounts();
}

// --- Инициализация ---

// Запуск при старте страницы
document.addEventListener("DOMContentLoaded", () => {
    loadLibrary();
    document.querySelector('.nav-item[onclick="switchTab(\'library\')"]').classList.add('active');

    setupGameDetailsModal();
    setupCategoriesModal();

    // Горячая клавиша поиска (Ctrl+F / Cmd+F), как Quick Search в Playnite
    document.addEventListener('keydown', (e) => {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
            e.preventDefault();
            switchTab('library');
            const input = document.getElementById('filter-search');
            if (input) { input.focus(); input.select(); }
        }
    });

    // Бэкенд сообщает об окончании игровой сессии (обновилось время в игре)
    try {
        EventsOn('library-updated', () => {
            loadLibrary();
        });
    } catch (e) {
        console.warn('EventsOn недоступен:', e);
    }

    const launchersList = document.getElementById('launchers-list');
    if (launchersList) {
        launchersList.addEventListener('click', function (event) {
            const target = event.target;

            // Проверяем, был ли клик на кнопках действий (у них есть inline onclick, пропускаем)
            const actionBtn = target.closest('.action-icon-btn');
            if (actionBtn) {
                // Inline onclick handlers уже обрабатывают эти кнопки
                return;
            }

            // Клик по строке аккаунта для переключения
            const accountRow = target.closest('.account-row');
            if (accountRow) {
                const username = accountRow.dataset.username;
                const platform = accountRow.dataset.platform;
                const isHidden = accountRow.dataset.hidden === 'true';

                if (username && platform) {
                    if (isHidden) {
                        // confirm() auto-closes on macOS Wails - skip confirmation
                        window.restoreAccount(username, platform);
                    } else {
                        window.switchAccount(username, platform);
                    }
                }
            }
        });
    }
});

// --- Функции навигации и интерфейса ---

window.toggleHiddenAccounts = function () {
    showHiddenAccounts = !showHiddenAccounts;
    loadAccounts();
}

// Переключение вкладок (Библиотека / Аккаунты / Статистика)
window.switchTab = function (tab) {
    document.querySelectorAll('.view-section').forEach(e => e.style.display = 'none');
    document.querySelectorAll('.nav-item').forEach(e => e.classList.remove('active'));

    // Подсветка активной кнопки
    if (tab === 'library') {
        document.getElementById('view-library').style.display = 'block';
        document.querySelector('.nav-item[onclick="switchTab(\'library\')"]').classList.add('active');
        // При переключении на библиотеку лучше обновить данные, но фильтры сохранить
        loadLibrary();
    } else if (tab === 'accounts') {
        document.getElementById('view-accounts').style.display = 'block';
        document.querySelector('.nav-item[onclick="switchTab(\'accounts\')"]').classList.add('active');
        loadAccounts();
    } else if (tab === 'stats') {
        document.getElementById('view-stats').style.display = 'block';
        document.querySelector('.nav-item[onclick="switchTab(\'stats\')"]').classList.add('active');
        if (globalGames.length === 0) {
            GetLibrary().then(games => {
                globalGames = games || [];
                renderStats();
            }).catch(err => console.error('Ошибка загрузки библиотеки:', err));
        } else {
            renderStats();
        }
    }
}

// Управление модальными окнами
window.closeModal = function (id) {
    document.getElementById(id).style.display = 'none';
}

window.openAddGameModal = function () {
    document.getElementById('add-game-modal').style.display = 'flex';
}

// --- Логика Библиотеки (Library) и Фильтрации ---

// Главная функция загрузки (запрашивает данные из Go)
async function loadLibrary() {
    try {
        const games = await GetLibrary();
        globalGames = games || [];
        renderPlatformFilters();
        renderCategoryFilter();
        applyFilters(); // Фильтруем и рисуем

        // Если открыта модалка игры или статистика — обновляем и их
        if (currentModalGameId && document.getElementById('account-modal').style.display === 'flex') {
            renderGameModalContent(currentModalGameId);
        }
        const statsView = document.getElementById('view-stats');
        if (statsView && statsView.style.display !== 'none') renderStats();
    } catch (err) {
        console.error("Ошибка загрузки библиотеки:", err);
    }
}

// Строит кнопки платформ динамически из содержимого библиотеки
function renderPlatformFilters() {
    const container = document.getElementById('platform-filters');
    if (!container) return;

    const platforms = [...new Set(globalGames.map(g =>
        g.platform === 'Torrent' ? 'Custom' : g.platform
    ))].sort();

    container.innerHTML = '';
    platforms.forEach(p => {
        const btn = document.createElement('button');
        btn.className = 'filter-tag';
        if (!filterState.disabledPlatforms.includes(p)) btn.classList.add('active');
        const icon = PLATFORM_ICONS[p] || 'fa-solid fa-gamepad';
        btn.innerHTML = `<i class="${icon}"></i> ${escapeHtml(p)}`;
        btn.onclick = () => toggleFilter(btn, p);
        container.appendChild(btn);
    });
}

// Заполняет выпадающий список категорий (Playnite-style categories)
function renderCategoryFilter() {
    const select = document.getElementById('filter-category');
    if (!select) return;

    const cats = new Set();
    globalGames.forEach(g => (g.categories || []).forEach(c => cats.add(c)));

    const current = filterState.category;
    select.innerHTML = '<option value="">Все категории</option>';
    [...cats].sort((a, b) => a.localeCompare(b, 'ru')).forEach(c => {
        const opt = document.createElement('option');
        opt.value = c;
        opt.textContent = c;
        select.appendChild(opt);
    });
    // Восстанавливаем выбор, если категория ещё существует
    select.value = cats.has(current) ? current : '';
    filterState.category = select.value;
}

// Управление фильтрами платформ
window.toggleFilter = function (btn, platform) {
    btn.classList.toggle('active');
    if (btn.classList.contains('active')) {
        filterState.disabledPlatforms = filterState.disabledPlatforms.filter(p => p !== platform);
    } else if (!filterState.disabledPlatforms.includes(platform)) {
        filterState.disabledPlatforms.push(platform);
    }
    applyFilters();
}

// Управление булевыми фильтрами (Installed / macOS / Избранное / Скрытые)
window.toggleBooleanFilter = function (btn) {
    btn.classList.toggle('active');
    const on = btn.classList.contains('active');
    if (btn.id === 'filter-installed') filterState.onlyInstalled = on;
    if (btn.id === 'filter-macos') filterState.onlyMac = on;
    if (btn.id === 'filter-favorites') filterState.onlyFavorites = on;
    if (btn.id === 'filter-hidden') filterState.onlyHidden = on;
    applyFilters();
}

// Сортировка библиотеки (Playnite-style sorting)
function sortGames(games, sortBy) {
    const sorted = [...games];
    switch (sortBy) {
        case 'lastPlayed':
            sorted.sort((a, b) => (b.lastPlayed || 0) - (a.lastPlayed || 0));
            break;
        case 'playtime':
            sorted.sort((a, b) => (b.playtimeSeconds || 0) - (a.playtimeSeconds || 0));
            break;
        case 'playCount':
            sorted.sort((a, b) => (b.playCount || 0) - (a.playCount || 0));
            break;
        case 'userScore':
            sorted.sort((a, b) => (b.userScore || 0) - (a.userScore || 0));
            break;
        case 'platform':
            sorted.sort((a, b) => a.platform.localeCompare(b.platform) || a.name.localeCompare(b.name, 'ru'));
            break;
        default: // name: избранные сверху (как было), затем по алфавиту
            sorted.sort((a, b) => {
                if (a.isPinned !== b.isPinned) return a.isPinned ? -1 : 1;
                if (a.isInstalled !== b.isInstalled) return a.isInstalled ? -1 : 1;
                return a.name.localeCompare(b.name, 'ru');
            });
    }
    return sorted;
}

// Применяет фильтры к globalGames и вызывает рендер
window.applyFilters = function () {
    const searchInput = document.getElementById('filter-search');
    filterState.searchQuery = searchInput ? searchInput.value.toLowerCase() : '';

    const sortSelect = document.getElementById('sort-select');
    filterState.sortBy = sortSelect ? sortSelect.value : 'name';

    const catSelect = document.getElementById('filter-category');
    filterState.category = catSelect ? catSelect.value : '';

    const q = filterState.searchQuery;

    const filtered = globalGames.filter(game => {
        // 1. Фильтр платформ (Torrent объединён с Custom)
        const pCheck = game.platform === 'Torrent' ? 'Custom' : game.platform;
        if (filterState.disabledPlatforms.includes(pCheck)) return false;

        // 2. Скрытые игры: по умолчанию прячем, в режиме "Скрытые" показываем только их
        if (filterState.onlyHidden) {
            if (!game.isHidden) return false;
        } else if (game.isHidden) {
            return false;
        }

        // 3. Только установленные
        if (filterState.onlyInstalled && !game.isInstalled) return false;

        // 4. Поддержка macOS
        if (filterState.onlyMac && !game.isMacSupported) return false;

        // 5. Только избранные
        if (filterState.onlyFavorites && !game.isPinned) return false;

        // 6. Категория
        if (filterState.category && !(game.categories || []).includes(filterState.category)) return false;

        // 7. Поиск: название, жанры, категории
        if (q) {
            const inName = game.name.toLowerCase().includes(q);
            const inGenres = (game.genres || []).some(g => g.toLowerCase().includes(q));
            const inCats = (game.categories || []).some(c => c.toLowerCase().includes(q));
            if (!inName && !inGenres && !inCats) return false;
        }

        return true;
    });

    renderGrid(sortGames(filtered, filterState.sortBy));
}

// Отрисовка сетки игр
function renderGrid(games) {
    const list = document.getElementById('games-grid');
    if (!list) return;
    list.innerHTML = '';

    if (!games || games.length === 0) {
        list.innerHTML = '<div style="color:#aaa; padding:20px; grid-column: 1/-1; text-align:center;">Игры не найдены по заданным фильтрам.</div>';
        return;
    }

    games.forEach(game => {
        const card = document.createElement('div');
        card.className = 'card';
        if (!game.isInstalled) card.classList.add('game-not-installed');

        // Атрибуты для контекстного меню
        card.setAttribute('data-id', game.id);
        card.setAttribute('data-platform', game.platform);

        // Клик по карточке открывает детали
        card.onclick = () => openGameModal(game.id);

        // Цвет плашки платформы
        const platColor = PLATFORM_COLORS[game.platform] || '#0074e4';

        // Обработка картинки
        let img = 'https://via.placeholder.com/300x169?text=' + encodeURIComponent(game.name);
        if (game.iconUrl) {
            img = game.iconUrl.replace(/\\/g, '/');
        }

        const pinClass = game.isPinned ? 'active' : '';
        const pinBtn = `<div class="pin-btn ${pinClass}" onclick="window.togglePin(event, '${game.id}')" title="Избранное"><i class="fa-solid fa-star"></i></div>`;

        let overlay = '';
        if (!game.isInstalled) overlay = '<div class="install-overlay"><i class="fa-solid fa-download"></i></div>';

        // Иконка Apple если игра поддерживает macOS
        let macBadge = '';
        if (game.isMacSupported) macBadge = `<i class="fa-brands fa-apple" style="margin-right:5px;" title="macOS Supported"></i>`;

        // Маркер скрытой игры
        let hiddenBadge = '';
        if (game.isHidden) hiddenBadge = `<div class="hidden-badge" title="Игра скрыта"><i class="fa-solid fa-eye-slash"></i></div>`;

        // Статус прохождения (цветная плашка внизу обложки)
        let statusChip = '';
        if (game.completionStatus) {
            const color = STATUS_COLORS[game.completionStatus] || '#888';
            statusChip = `<div class="status-chip" style="background:${color}">${statusLabel(game.completionStatus)}</div>`;
        }

        // Нижняя строка: аккаунты + оценка + время в игре
        const accCount = game.availableOn ? game.availableOn.length : 0;
        const playtime = formatPlaytimeShort(game.playtimeSeconds);
        const scoreHtml = game.userScore > 0
            ? `<span class="card-score" title="Моя оценка"><i class="fa-solid fa-star"></i> ${game.userScore}</span>` : '';
        const playtimeHtml = playtime
            ? `<span class="card-playtime" title="Время в игре"><i class="fa-solid fa-clock"></i> ${playtime}</span>` : '';

        card.innerHTML = `
            <div class="card-img-wrapper">
                <img src="${img}" class="card-img" loading="lazy">
                <div class="platform-badge" style="background:${platColor}">
                    ${macBadge}${escapeHtml(game.platform)}
                </div>
                ${pinBtn}
                ${hiddenBadge}
                ${statusChip}
                ${overlay}
            </div>
            <div class="card-info">
                <div class="game-title" title="${escapeHtml(game.name)}">${escapeHtml(game.name)}</div>
                <div class="account-count card-meta-row">
                    <span>${accCount} акк.</span>
                    <span class="card-meta-right">${scoreHtml}${playtimeHtml}
                        ${game.isPinned ? '<i class="fa-solid fa-star" style="color:#FFD700; font-size:10px;"></i>' : ''}
                    </span>
                </div>
            </div>
        `;
        list.appendChild(card);
    });
}

// Избранное (Favorite в терминах Playnite)
window.togglePin = async function (e, gameId) {
    if (e) e.stopPropagation();
    await ToggleGamePin(gameId);
    // Обновляем локально без полного пересканирования библиотеки
    const game = globalGames.find(g => g.id === gameId);
    if (game) game.isPinned = !game.isPinned;
    applyFilters();
    if (currentModalGameId === gameId && document.getElementById('account-modal').style.display === 'flex') {
        renderGameModalContent(gameId);
    }
}

// Скрытие игры во всей библиотеке (Playnite-style hidden games)
window.toggleHidden = async function (gameId) {
    await ToggleGameHidden(gameId);
    const game = globalGames.find(g => g.id === gameId);
    if (game) game.isHidden = !game.isHidden;
    applyFilters();
    if (currentModalGameId === gameId && document.getElementById('account-modal').style.display === 'flex') {
        renderGameModalContent(gameId);
    }
}

// Открытие модального окна игры
window.openGameModal = function (gameId) {
    currentModalGameId = gameId;
    const checkbox = document.getElementById('show-hidden-accs');
    if (checkbox) checkbox.checked = false;

    renderGameModalContent(gameId);
    document.getElementById('account-modal').style.display = 'flex';
}

window.refreshGameModal = function () {
    if (currentModalGameId) renderGameModalContent(currentModalGameId);
}

// --- Панель деталей игры (Playnite-style game details) ---

// Одноразовая привязка обработчиков статичных кнопок модалки
function setupGameDetailsModal() {
    const playBtn = document.getElementById('gd-play');
    if (playBtn) playBtn.onclick = () => playCurrentGame();

    const favBtn = document.getElementById('gd-fav');
    if (favBtn) favBtn.onclick = () => togglePin(null, currentModalGameId);

    const hideBtn = document.getElementById('gd-hide');
    if (hideBtn) hideBtn.onclick = () => toggleHidden(currentModalGameId);

    const metaBtn = document.getElementById('gd-meta');
    if (metaBtn) metaBtn.onclick = () => downloadMetadata();

    const completion = document.getElementById('gd-completion');
    if (completion) {
        completion.onchange = async () => {
            const game = globalGames.find(g => g.id === currentModalGameId);
            if (!game) return;
            await SetGameCompletionStatus(game.id, completion.value);
            game.completionStatus = completion.value;
            applyFilters();
        };
    }
}

// Кнопка "Играть": custom — запуск напрямую; один аккаунт — сразу с ним;
// несколько — подсвечиваем список аккаунтов
function playCurrentGame() {
    const game = globalGames.find(g => g.id === currentModalGameId);
    if (!game) return;

    if (game.platform === 'Custom' || game.platform === 'Torrent') {
        launch('', game.id, game.platform, game.exePath);
        return;
    }

    const visibleAccs = (game.availableOn || []).filter(a => !a.isHidden);
    if (visibleAccs.length === 1) {
        launch(visibleAccs[0].username, game.id, game.platform, '');
        return;
    }
    if (visibleAccs.length === 0 && (!game.availableOn || game.availableOn.length === 0)) {
        launch('', game.id, game.platform, '');
        return;
    }

    // Несколько аккаунтов — просим выбрать
    const list = document.getElementById('modal-accounts-list');
    if (list) {
        list.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
        list.classList.remove('flash');
        void list.offsetWidth; // перезапуск css-анимации
        list.classList.add('flash');
    }
}

// Загрузка метаданных (описание, жанры) — для Steam через Store API
async function downloadMetadata() {
    const game = globalGames.find(g => g.id === currentModalGameId);
    if (!game) return;

    const btn = document.getElementById('gd-meta');
    if (btn) btn.innerHTML = '<i class="fa-solid fa-spinner fa-spin"></i>';

    try {
        const meta = await FetchGameMetadata(game.id, game.platform);
        if (meta && meta.found) {
            game.description = meta.description;
            game.genres = meta.genres;
            game.releaseDate = meta.releaseDate;
            game.developers = meta.developers;
            renderGameModalContent(game.id);
            applyFilters();
        } else {
            alert(meta && meta.error ? meta.error : 'Метаданные не найдены');
        }
    } catch (e) {
        alert('Ошибка загрузки метаданных: ' + e);
    } finally {
        if (btn) btn.innerHTML = '<i class="fa-solid fa-cloud-arrow-down"></i>';
    }
}

// Рендер звёзд оценки (1-10, повторный клик по текущей — сброс)
function renderScoreStars(game) {
    const wrap = document.getElementById('gd-score');
    if (!wrap) return;
    wrap.innerHTML = '';
    for (let i = 1; i <= 10; i++) {
        const star = document.createElement('i');
        star.className = 'fa-solid fa-star gd-star' + (i <= (game.userScore || 0) ? ' filled' : '');
        star.onclick = async () => {
            const newScore = (game.userScore === i) ? 0 : i;
            await SetGameUserScore(game.id, newScore);
            game.userScore = newScore;
            renderScoreStars(game);
            applyFilters();
        };
        wrap.appendChild(star);
    }
}

// Рендер чипов категорий в модалке
function renderModalCategories(game) {
    const wrap = document.getElementById('gd-categories');
    if (!wrap) return;
    wrap.innerHTML = '';
    (game.categories || []).forEach(c => {
        const chip = document.createElement('span');
        chip.className = 'cat-chip';
        chip.textContent = c;
        wrap.appendChild(chip);
    });
    const edit = document.createElement('span');
    edit.className = 'cat-chip cat-chip-add';
    edit.innerHTML = '<i class="fa-solid fa-tags"></i> ' + ((game.categories || []).length ? 'Изменить' : 'Категории');
    edit.onclick = () => openCategoriesModal(game.id);
    wrap.appendChild(edit);
}

// Рендер содержимого модального окна (детали игры + список аккаунтов)
function renderGameModalContent(gameId) {
    const game = globalGames.find(g => g.id === gameId);
    if (!game) return;

    const isCustom = game.platform === 'Custom' || game.platform === 'Torrent';
    const actionVerb = game.isInstalled ? "Launch" : "Install";
    const actionIcon = game.isInstalled ? "fa-play" : "fa-download";

    // --- Шапка ---
    document.getElementById('modal-game-title').innerText = game.name;

    const cover = document.getElementById('gd-cover');
    let img = 'https://via.placeholder.com/300x169?text=' + encodeURIComponent(game.name);
    if (game.iconUrl) img = game.iconUrl.replace(/\\/g, '/');
    cover.src = img;

    // Бейджи: платформа + жанры
    const badges = document.getElementById('gd-badges');
    const platColor = PLATFORM_COLORS[game.platform] || '#0074e4';
    let badgesHtml = `<span class="gd-platform" style="background:${platColor}">${escapeHtml(game.platform)}</span>`;
    (game.genres || []).forEach(g => {
        badgesHtml += `<span class="gd-genre">${escapeHtml(g)}</span>`;
    });
    badges.innerHTML = badgesHtml;

    // Подзаголовок: дата выхода и разработчик
    const subParts = [];
    if (game.releaseDate) subParts.push(game.releaseDate);
    if (game.developers && game.developers.length) subParts.push(game.developers.join(', '));
    document.getElementById('gd-sub').textContent = subParts.join(' • ');

    // Статистика
    document.getElementById('gd-playtime').textContent = formatPlaytime(game.playtimeSeconds);
    document.getElementById('gd-playcount').textContent = game.playCount || 0;
    document.getElementById('gd-lastplayed').textContent = formatLastPlayed(game.lastPlayed);

    // Статус прохождения
    const completion = document.getElementById('gd-completion');
    completion.innerHTML = '';
    COMPLETION_STATUSES.forEach(s => {
        const opt = document.createElement('option');
        opt.value = s.value;
        opt.textContent = s.label;
        completion.appendChild(opt);
    });
    completion.value = game.completionStatus || '';

    renderScoreStars(game);
    renderModalCategories(game);

    // Кнопки действий
    const playBtn = document.getElementById('gd-play');
    playBtn.innerHTML = game.isInstalled || isCustom
        ? '<i class="fa-solid fa-play"></i> Играть'
        : '<i class="fa-solid fa-download"></i> Установить';

    const favBtn = document.getElementById('gd-fav');
    favBtn.classList.toggle('gd-btn-active', !!game.isPinned);
    favBtn.title = game.isPinned ? 'Убрать из избранного' : 'В избранное';

    const hideBtn = document.getElementById('gd-hide');
    hideBtn.classList.toggle('gd-btn-active', !!game.isHidden);
    hideBtn.innerHTML = game.isHidden ? '<i class="fa-solid fa-eye"></i>' : '<i class="fa-solid fa-eye-slash"></i>';
    hideBtn.title = game.isHidden ? 'Показать игру' : 'Скрыть игру';

    // Метаданные доступны только для Steam
    document.getElementById('gd-meta').style.display = game.platform === 'Steam' ? '' : 'none';

    // Описание
    const desc = document.getElementById('gd-description');
    if (game.description) {
        desc.textContent = game.description;
        desc.classList.remove('empty');
    } else {
        desc.textContent = game.platform === 'Steam'
            ? 'Нет описания. Нажмите кнопку загрузки метаданных, чтобы получить его из Steam.'
            : 'Нет описания.';
        desc.classList.add('empty');
    }

    // --- Список аккаунтов ---
    const accountsHeader = document.getElementById('gd-accounts-header');
    const footerOptions = document.querySelector('#account-modal .modal-footer-options');
    const list = document.getElementById('modal-accounts-list');

    if (isCustom) {
        // У custom-игр нет аккаунтов
        accountsHeader.style.display = 'none';
        footerOptions.style.display = 'none';
        list.style.display = 'none';
        list.innerHTML = '';
        return;
    }
    accountsHeader.style.display = '';
    footerOptions.style.display = '';
    list.style.display = '';

    // Проверяем галочку "Show Hidden"
    const checkbox = document.getElementById('show-hidden-accs');
    const showHidden = checkbox ? checkbox.checked : false;

    list.innerHTML = '';

    if (!game.availableOn || game.availableOn.length === 0) {
        // Если аккаунтов нет, показываем кнопку запуска "Current Account"
        list.innerHTML = `<div class="modal-item interactable" onclick="launch('', '${game.id}', '${game.platform}', '')">
            <div class="acc-name">${actionVerb} Game</div>
            <div class="acc-meta">Current Account (or not logged in)</div>
        </div>`;
    } else {
        game.availableOn.forEach(acc => {
            // Фильтр скрытых
            if (acc.isHidden && !showHidden) return;

            const item = document.createElement('div');
            item.className = 'modal-item';
            if (acc.isHidden) item.classList.add('acc-hidden-row');

            // Бейджики для заметок
            let noteHtml = '';
            if (acc.note === 'Main') noteHtml = `<span class="badge badge-main"><i class="fa-solid fa-crown"></i> MAIN</span>`;
            else if (acc.note === 'Smurf') noteHtml = `<span class="badge badge-smurf"><i class="fa-solid fa-child"></i> SMURF</span>`;
            else if (acc.note === 'VAC') noteHtml = `<span class="badge badge-vac"><i class="fa-solid fa-ban"></i> VAC BAN</span>`;
            else if (acc.note) noteHtml = `<span class="badge badge-custom"><i class="fa-solid fa-note-sticky"></i> ${acc.note}</span>`;

            const hideIcon = acc.isHidden ? "fa-rotate-left" : "fa-eye-slash";
            const hideClass = acc.isHidden ? "toggle-restore-btn" : "toggle-hidden-btn";
            const hideTitle = acc.isHidden ? "Restore account" : "Hide from this game";

            item.innerHTML = `
                <div style="flex:1; cursor:pointer;" onclick="launch('${acc.username}', '${game.id}', '${game.platform}', '')">
                    <div class="acc-name" style="font-weight:bold; display:flex; align-items:center;">
                        <i class="fa-solid ${actionIcon}" style="margin-right:8px; font-size:12px; color:#aaa;"></i>
                        ${acc.displayName}
                        ${noteHtml}
                    </div>
                    <div class="acc-meta" style="font-size:12px; color:#aaa;">Login: ${acc.username}</div>
                </div>
                
                <div class="edit-note-btn" onclick="openNoteSelector('${acc.username}', '${game.platform}', '${game.id}', '${acc.note || ''}')" title="Tag Account">
                    <i class="fa-solid fa-tag"></i>
                </div>
                
                <div class="toggle-hidden-btn ${hideClass}" onclick="toggleGameAccountHidden('${acc.username}', '${game.platform}', '${game.id}')" title="${hideTitle}">
                    <i class="fa-solid ${hideIcon}"></i>
                </div>
            `;
            list.appendChild(item);
        });
    }
}

// Запуск игры
window.launch = async function (account, gameId, platform, exePath) {
    try {
        const res = await LaunchGame(account, gameId, platform, exePath);
        // Если вернулось не "Launched...", значит ошибка
        if (res && !res.startsWith("Launched") && !res.startsWith("Success")) {
            alert(res);
        } else {
            document.getElementById('account-modal').style.display = 'none';
            // Бэкенд уже записал запуск — отражаем это локально, не пересканируя библиотеку
            const game = globalGames.find(g => g.id === gameId);
            if (game) {
                game.playCount = (game.playCount || 0) + 1;
                game.lastPlayed = Math.floor(Date.now() / 1000);
                applyFilters();
            }
        }
    } catch (e) {
        alert("Launch error: " + e);
    }
}

// Скрытие/Показ аккаунта для конкретной игры
window.toggleGameAccountHidden = async function (username, platform, gameId) {
    await ToggleGameAccountHidden(username, platform, gameId);
    // Обновляем данные глобально
    await loadLibrary();
    // Перерисовываем модалку
    renderGameModalContent(gameId);
}

// --- Управление заметками (Notes) ---

window.openNoteSelector = function (username, platform, gameId, currentNote) {
    editingTagTarget = { username, platform, gameId };
    const isPredefined = ['Main', 'Smurf', 'VAC'].includes(currentNote);
    document.getElementById('custom-note-input').value = isPredefined ? '' : currentNote;
    document.getElementById('note-select-modal').style.display = 'flex';
}

window.savePredefinedNote = async function (noteType) {
    await applyNote(noteType);
}

window.saveCustomNote = async function () {
    const text = document.getElementById('custom-note-input').value;
    await applyNote(text);
}

async function applyNote(noteText) {
    const { username, platform, gameId } = editingTagTarget;
    await UpdateGameNote(username, platform, gameId, noteText);
    await loadLibrary();
    document.getElementById('note-select-modal').style.display = 'none';
    renderGameModalContent(gameId);
}


// --- Логика вкладки Аккаунты (Accounts) ---

async function loadAccounts() {
    const container = document.getElementById('launchers-list');
    if (!container) return;

    container.innerHTML = '<div style="padding:20px">Loading accounts...</div>';

    try {
        const launchers = await GetLaunchers();
        container.innerHTML = '';

        if (!launchers || launchers.length === 0) {
            container.innerHTML = '<div style="padding:20px">No launchers found.</div>';
            return;
        }

        launchers.forEach(group => {
            const section = document.createElement('div');
            section.className = 'launcher-section';

            let accountsHtml = '';
            const accounts = group.accounts || [];

            accounts.forEach(acc => {
                // Не показывать скрытые аккаунты, если флаг выключен
                const isHidden = acc.comment && acc.comment.startsWith('_swch_hidden_');
                if (isHidden && !showHiddenAccounts) return;

                let avatarHtml = `<div class="acc-avatar">${acc.displayName.charAt(0)}</div>`;
                if (acc.avatarUrl) {
                    avatarHtml = `<div class="acc-avatar" style="background-image: url('${acc.avatarUrl.replace(/\\/g, '/')}'); background-size: cover; color: transparent;"></div>`;
                }

                let displayComment = acc.comment;
                if (isHidden) {
                    const parts = acc.comment.split(':');
                    const reason = parts.slice(1).join(':');
                    displayComment = reason ? `Hidden: ${reason}` : 'Hidden';
                }
                const commentHtml = displayComment ? `<div class="acc-comment">${displayComment}</div>` : '';
                const rowStyle = isHidden ? 'opacity: 0.6;' : '';

                const hideBtnClass = isHidden ? 'restore-account-btn' : 'hide-account-btn';
                const hideBtnIcon = isHidden ? 'fa-rotate-left' : 'fa-eye-slash';
                const hideBtnTitle = isHidden ? 'Restore Account' : 'Hide Account';

                // Функция для экранирования строк в JS (для inline onclick)
                const escapeForJs = (str) => (str || '').replace(/\\/g, '\\\\').replace(/'/g, "\\'");
                const safeUsername = escapeForJs(acc.username);
                const safePlatform = escapeForJs(group.platform);
                const safeComment = escapeForJs(acc.comment);

                // Для скрытых аккаунтов используем restoreAccount, для обычных - hideAccount
                const hideAction = isHidden
                    ? `event.stopPropagation(); window.restoreAccount('${safeUsername}', '${safePlatform}')`
                    : `event.stopPropagation(); window.hideAccount('${safeUsername}', '${safePlatform}')`;
                const deleteAction = `event.stopPropagation(); window.deleteAccount('${safeUsername}', '${safePlatform}')`;
                const editAction = `event.stopPropagation(); window.openEditAccount('${safeUsername}', '${safePlatform}', '${safeComment}')`;

                accountsHtml += `
                    <div class="account-row interactable" style="${rowStyle}" data-username="${acc.username}" data-platform="${group.platform}" data-hidden="${isHidden}">
                        ${avatarHtml}
                        <div class="acc-details">
                            <div class="acc-nick">${acc.displayName}${commentHtml}</div>
                            <div class="acc-login">${acc.username}</div>
                        </div>
                        <div class="acc-actions">
                            <div class="action-icon-btn edit-account-btn" onclick="${editAction}"><i class="fa-solid fa-pen"></i></div>
                            <div class="action-icon-btn ${hideBtnClass}" onclick="${hideAction}" title="${hideBtnTitle}"><i class="fa-solid ${hideBtnIcon}"></i></div>
                            <div class="action-icon-btn delete-btn" onclick="${deleteAction}"><i class="fa-solid fa-trash"></i></div>
                        </div>
                        <div class="acc-action-icon"><i class="fa-solid fa-arrow-right-to-bracket"></i></div>
                    </div>`;
            });


            // Кнопка для добавления (Footer)
            let footerHtml = '';

            const hiddenText = showHiddenAccounts ? "Hide Hidden" : "Show Hidden";
            const hiddenIcon = showHiddenAccounts ? "fa-eye-slash" : "fa-eye";
            const showHiddenBtn = `<button onclick="toggleHiddenAccounts()" style="cursor:pointer; background:none; color:#aaa; border:1px dashed #444; padding:8px; border-radius:4px;">
                        <i class="fa-solid ${hiddenIcon}"></i> ${hiddenText}
                    </button>`;

            // Для Epic Games (Official)
            if (group.platform === 'Epic') {
                footerHtml = `<div style="padding:10px; text-align:center; border-top:1px solid #2a2a2a; display:grid; grid-template-columns: repeat(5, 1fr); gap:6px;">
                    <button onclick="openEpicSaveModal()" title="Save Current Account" style="cursor:pointer; background:none; color:#aaa; border:1px dashed #444; padding:6px; border-radius:4px;">
                        <i class="fa-solid fa-plus"></i>
                    </button>
                    <button onclick="openEpicAuthModal()" title="Login via Auth Code" style="cursor:pointer; background:none; color:#4f4; border:1px dashed #4f4; padding:6px; border-radius:4px;">
                        <i class="fa-solid fa-key"></i>
                    </button>
                    <button onclick="installAllEpicGames()" title="Minimal Install All Games" style="cursor:pointer; background:none; color:#4af; border:1px dashed #4af; padding:6px; border-radius:4px;">
                        <i class="fa-solid fa-cloud-arrow-down"></i>
                    </button>
                    <button onclick="logoutEpic()" title="Logout" style="cursor:pointer; background:none; color:#f44; border:1px dashed #f44; padding:6px; border-radius:4px;">
                        <i class="fa-solid fa-right-from-bracket"></i>
                    </button>
                    <button onclick="toggleHiddenAccounts()" title="${hiddenText}" style="cursor:pointer; background:none; color:#aaa; border:1px dashed #444; padding:6px; border-radius:4px;">
                        <i class="fa-solid ${hiddenIcon}"></i>
                    </button>
                </div>`;
            } else if (group.platform === 'Riot') {
                footerHtml = `<div style="padding:10px; text-align:center; border-top:1px solid #2a2a2a; display:grid; grid-template-columns: 1fr 1fr; gap:10px;">
                    <button onclick="openRiotSaveModal()" style="cursor:pointer; background:none; color:#aaa; border:1px dashed #444; padding:8px; border-radius:4px;">
                        <i class="fa-solid fa-plus"></i> Save Current
                    </button>
                     ${showHiddenBtn}
                </div>`;
            } else if (group.platform === 'Roblox') {
                footerHtml = `<div style="padding:10px; text-align:center; border-top:1px solid #2a2a2a; display:grid; grid-template-columns: 1fr 1fr; gap:10px;">
                    <button onclick="openRobloxSaveModal()" style="cursor:pointer; background:none; color:#aaa; border:1px dashed #444; padding:8px; border-radius:4px;">
                        <i class="fa-solid fa-plus"></i> Save Current
                    </button>
                     ${showHiddenBtn}
                </div>`;
            } else {
                // Steam и другие
                footerHtml = `<div style="padding:10px; text-align:center; border-top:1px solid #2a2a2a; display:grid; grid-template-columns: 1fr; gap:10px;">
                     ${showHiddenBtn}
                </div>`;
            }
            let iconClass = "fa-gamepad";
            if (group.platform === "Steam") iconClass = "fa-steam";
            if (group.platform === "Epic") iconClass = "fa-bolt";
            if (group.platform === "Roblox") iconClass = "fa-square"; // Иконка для Roblox

            section.innerHTML = `
                <div class="launcher-header">
                    <i class="fa-brands ${iconClass}"></i> ${group.name} 
                    <span class="count">${accounts.length}</span>
                </div>
                <div class="launcher-accounts">${accountsHtml}</div>
                ${footerHtml}
            `;
            container.appendChild(section);
        });
    } catch (e) {
        container.innerHTML = `<div style="color:red; padding:20px;">Error loading accounts: ${e}</div>`;
        console.error("Load Accounts Error:", e);
    }
}

// --- Добавление игр (Custom / Torrent) ---

window.browseFile = async function () {
    const path = await SelectExe();
    if (path) document.getElementById('custom-path').value = path;
}

window.saveCustomGame = async function () {
    const res = await AddCustomGame(
        document.getElementById('custom-name').value,
        document.getElementById('custom-path').value
    );

    if (res === 'Success') {
        closeModal('add-game-modal');
        loadLibrary();
        document.getElementById('custom-name').value = '';
        document.getElementById('custom-path').value = '';
    } else {
        alert(res);
    }
}

// --- Добавление Epic Account ---

window.openEpicSaveModal = function () {
    document.getElementById('epic-save-name').value = '';
    document.getElementById('save-epic-modal').style.display = 'flex';
}

window.logoutEpic = async function () {
    // Skip confirm dialog - it auto-closes on macOS Wails
    try {
        const result = await LogoutEpic();
        alert(result);
        loadAccounts();
    } catch (e) {
        alert("Error: " + e);
    }
}

// Epic Device Auth flow
window.openEpicAuthModal = async function () {
    try {
        // Open Epic auth URL in system browser using Wails BrowserOpenURL
        await OpenEpicAuthPage();

        // Show modal for auth code input
        document.getElementById('epic-auth-modal').style.display = 'flex';
        document.getElementById('epic-auth-code').value = '';
        document.getElementById('epic-auth-name').value = '';
    } catch (e) {
        alert("Error: " + e);
    }
}

window.submitEpicAuthCode = async function () {
    // Strip quotes if user pasted them from JSON
    let authCode = document.getElementById('epic-auth-code').value.trim();
    if (authCode.startsWith('"') && authCode.endsWith('"')) {
        authCode = authCode.slice(1, -1);
    }
    const accountName = document.getElementById('epic-auth-name').value.trim();

    if (!authCode) {
        alert("Please enter the authorization code");
        return;
    }

    try {
        const result = await AuthorizeEpicWithCode(authCode, accountName);
        alert(result);
        document.getElementById('epic-auth-modal').style.display = 'none';
        loadAccounts();
    } catch (e) {
        alert("Error: " + e);
    }
}

window.doSaveEpic = async function () {
    const name = document.getElementById('epic-save-name').value;
    if (!name) {
        alert("Please enter a name");
        return;
    }

    try {
        const result = await SaveEpicAccount(name);
        if (result === "Success") {
            closeModal('save-epic-modal');
            alert("Epic account saved!");
            loadAccounts();
        } else {
            alert("Error: " + result);
        }
    } catch (e) {
        alert("System error: " + e);
        console.error(e);
    }
}


// --- ADD ACCOUNT LOGIC (NEW) ---

window.installAllEpicGames = async function () {
    if (!confirm("This will scan your Epic account and 'fake install' all owned games into your library (creating small stub files). Continue?")) {
        return;
    }
    
    // Show loading state?
    // alert("Starting process. Please wait..."); // alert blocks execution in some browsers but maybe ok here
    
    try {
        const result = await InstallAllEpicGames();
        alert(result);
        loadLibrary(); // Reload library to show new games
    } catch (e) {
        alert("Error: " + e);
    }
}

// --- ADD ACCOUNT LOGIC (NEW) ---

// Открытие главного меню добавления
window.openAddAccountModal = function () {
    document.getElementById('add-account-modal').style.display = 'flex';
}

// 1. Legendary Login
// 1. Epic Native Login (Replaces Legendary)
// 1. Epic Native Login
window.startEpicNativeLogin = function() {
    // Redirect to new Epic Auth Modal
    window.openEpicAuthModal();
}


// 2. Riot Save Logic (Аналогично Epic)
window.openRiotSaveModal = function () {
    document.getElementById('riot-save-name').value = '';
    document.getElementById('save-riot-modal').style.display = 'flex';
}

window.doSaveRiot = async function () {
    const name = document.getElementById('riot-save-name').value;
    if (!name) {
        alert("Please enter a name");
        return;
    }

    try {
        const result = await SaveRiotAccount(name);
        if (result === "Success") {
            closeModal('save-riot-modal');
            alert("Riot account saved!");
            loadAccounts();
        } else {
            alert("Error: " + result);
        }
    } catch (e) {
        alert("System error: " + e);
        console.error(e);
    }
}



// --- Логика контекстного меню (ПКМ) ---

const contextMenu = document.getElementById('context-menu');
const deleteBtn = document.getElementById('ctx-delete');
const changeIconBtn = document.getElementById('ctx-change-icon');
const ctxDetailsBtn = document.getElementById('ctx-details');
const ctxFavoriteBtn = document.getElementById('ctx-favorite');
const ctxHiddenBtn = document.getElementById('ctx-hidden');
const ctxCategoriesBtn = document.getElementById('ctx-categories');

// Закрываем меню при клике в любом месте
document.addEventListener('click', () => {
    if (contextMenu) contextMenu.style.display = 'none';
});

// Открытие меню
document.addEventListener('contextmenu', (e) => {
    const card = e.target.closest('.card');

    if (card) {
        e.preventDefault();

        // Получаем ID и платформу из data-атрибутов
        selectedGameId = card.getAttribute('data-id');
        selectedPlatform = card.getAttribute('data-platform');

        const game = globalGames.find(g => g.id === selectedGameId);

        // Подписи с учётом текущего состояния игры
        if (ctxFavoriteBtn) {
            ctxFavoriteBtn.innerHTML = game && game.isPinned
                ? '<i class="fa-regular fa-star"></i> Убрать из избранного'
                : '<i class="fa-solid fa-star"></i> В избранное';
        }
        if (ctxHiddenBtn) {
            ctxHiddenBtn.innerHTML = game && game.isHidden
                ? '<i class="fa-solid fa-eye"></i> Показать игру'
                : '<i class="fa-solid fa-eye-slash"></i> Скрыть игру';
        }

        // Логика: удалять и менять иконку можно только у Custom/Torrent игр
        const isCustom = (selectedPlatform === 'Torrent' || selectedPlatform === 'Custom');
        if (deleteBtn) deleteBtn.style.display = isCustom ? 'block' : 'none';
        if (changeIconBtn) changeIconBtn.style.display = isCustom ? 'block' : 'none';

        // Позиционирование меню
        contextMenu.style.top = `${e.pageY}px`;
        contextMenu.style.left = `${e.pageX}px`;
        contextMenu.style.display = 'block';
    } else {
        if (contextMenu) contextMenu.style.display = 'none';
    }
});

// Обработчики новых пунктов меню (Playnite-style)
if (ctxDetailsBtn) {
    ctxDetailsBtn.addEventListener('click', () => {
        if (selectedGameId) openGameModal(selectedGameId);
    });
}

if (ctxFavoriteBtn) {
    ctxFavoriteBtn.addEventListener('click', () => {
        if (selectedGameId) togglePin(null, selectedGameId);
    });
}

if (ctxHiddenBtn) {
    ctxHiddenBtn.addEventListener('click', () => {
        if (selectedGameId) toggleHidden(selectedGameId);
    });
}

if (ctxCategoriesBtn) {
    ctxCategoriesBtn.addEventListener('click', () => {
        if (selectedGameId) openCategoriesModal(selectedGameId);
    });
}

// Обработчик кнопки удаления
if (deleteBtn) {
    deleteBtn.addEventListener('click', async () => {
        if (!selectedGameId || !selectedPlatform) return;
        // confirm() auto-closes on macOS Wails - skip confirmation

        try {
            const result = await RemoveGame(selectedGameId, selectedPlatform);
            if (result === "Success") {
                loadLibrary();
            } else {
                alert("Ошибка: " + result);
            }
        } catch (err) {
            console.error(err);
        }
    });
}

// Обработчик смены иконки
if (changeIconBtn) {
    changeIconBtn.addEventListener('click', async () => {
        if (!selectedGameId || !selectedPlatform) return;

        const result = await SetGameImage(selectedGameId, selectedPlatform);

        if (result && !result.startsWith("Error") && result !== "Cancelled" && result !== "Not supported for this platform") {
            loadLibrary();
        } else if (result && result.startsWith("Error")) {
            alert(result);
        }
    });
}

// --- Roblox Logic ---
window.openRobloxSaveModal = function () {
    const el = document.getElementById('roblox-save-name');
    if (el) el.value = '';
    document.getElementById('save-roblox-modal').style.display = 'flex';
}

window.doSaveRoblox = async function () {
    const name = document.getElementById('roblox-save-name').value;
    if (!name) {
        alert("Please enter a name");
        return;
    }

    try {
        const result = await SaveRobloxAccount(name);
        if (result === "Success") {
            closeModal('save-roblox-modal');
            alert("Roblox account saved!");
            loadAccounts();
        } else {
            alert("Error: " + result);
        }
    } catch (e) {
        alert("System error: " + e);
        console.error(e);
    }
}

// --- Категории игр (Playnite-style categories) ---

function setupCategoriesModal() {
    const addBtn = document.getElementById('cat-add-btn');
    const input = document.getElementById('cat-input');
    const saveBtn = document.getElementById('cat-save-btn');

    const addFromInput = () => {
        const val = (input.value || '').trim();
        if (!val) return;
        if (!editingCategoriesList.some(c => c.toLowerCase() === val.toLowerCase())) {
            editingCategoriesList.push(val);
        }
        input.value = '';
        renderCategoriesEditor();
    };

    if (addBtn) addBtn.onclick = addFromInput;
    if (input) {
        input.addEventListener('keydown', (e) => {
            if (e.key === 'Enter') {
                e.preventDefault();
                addFromInput();
            }
        });
    }
    if (saveBtn) saveBtn.onclick = saveCategories;
}

window.openCategoriesModal = function (gameId) {
    const game = globalGames.find(g => g.id === gameId);
    if (!game) return;
    editingCategoriesGameId = gameId;
    editingCategoriesList = [...(game.categories || [])];
    const input = document.getElementById('cat-input');
    if (input) input.value = '';
    renderCategoriesEditor();
    document.getElementById('categories-modal').style.display = 'flex';
}

function renderCategoriesEditor() {
    const listEl = document.getElementById('cat-list');
    if (!listEl) return;
    listEl.innerHTML = '';

    if (editingCategoriesList.length === 0) {
        listEl.innerHTML = '<div style="color:#777; font-size:13px;">Категории не заданы</div>';
    }
    editingCategoriesList.forEach(c => {
        const chip = document.createElement('span');
        chip.className = 'cat-chip cat-chip-removable';
        chip.innerHTML = `${escapeHtml(c)} <i class="fa-solid fa-xmark"></i>`;
        chip.onclick = () => {
            editingCategoriesList = editingCategoriesList.filter(x => x !== c);
            renderCategoriesEditor();
        };
        listEl.appendChild(chip);
    });

    // Подсказки: категории других игр, которых нет у текущей
    const suggEl = document.getElementById('cat-suggestions');
    if (!suggEl) return;
    const used = new Set(editingCategoriesList.map(c => c.toLowerCase()));
    const all = new Set();
    globalGames.forEach(g => (g.categories || []).forEach(c => all.add(c)));
    const suggestions = [...all].filter(c => !used.has(c.toLowerCase())).sort((a, b) => a.localeCompare(b, 'ru'));

    suggEl.innerHTML = '';
    if (suggestions.length > 0) {
        const label = document.createElement('div');
        label.style.cssText = 'color:#777; font-size:12px; margin:12px 0 6px;';
        label.textContent = 'Существующие категории:';
        suggEl.appendChild(label);
        suggestions.forEach(c => {
            const chip = document.createElement('span');
            chip.className = 'cat-chip cat-chip-suggestion';
            chip.textContent = c;
            chip.onclick = () => {
                editingCategoriesList.push(c);
                renderCategoriesEditor();
            };
            suggEl.appendChild(chip);
        });
    }
}

async function saveCategories() {
    if (!editingCategoriesGameId) return;
    try {
        await SetGameCategories(editingCategoriesGameId, editingCategoriesList);
        const game = globalGames.find(g => g.id === editingCategoriesGameId);
        if (game) game.categories = [...editingCategoriesList];
        closeModal('categories-modal');
        renderCategoryFilter();
        applyFilters();
        if (currentModalGameId === editingCategoriesGameId &&
            document.getElementById('account-modal').style.display === 'flex') {
            renderGameModalContent(currentModalGameId);
        }
    } catch (e) {
        alert('Ошибка сохранения категорий: ' + e);
    }
}

// --- Статистика библиотеки (Playnite-style statistics) ---

function renderStats() {
    const container = document.getElementById('stats-content');
    if (!container) return;

    const games = globalGames;
    if (!games || games.length === 0) {
        container.innerHTML = '<div style="color:#aaa; padding:20px;">Библиотека пуста.</div>';
        return;
    }

    const total = games.length;
    const installed = games.filter(g => g.isInstalled).length;
    const favorites = games.filter(g => g.isPinned).length;
    const hidden = games.filter(g => g.isHidden).length;
    const totalPlaytime = games.reduce((s, g) => s + (g.playtimeSeconds || 0), 0);
    const totalLaunches = games.reduce((s, g) => s + (g.playCount || 0), 0);
    const neverPlayed = games.filter(g => !g.playCount).length;

    // Разбивка по платформам
    const byPlatform = {};
    games.forEach(g => {
        const p = g.platform === 'Torrent' ? 'Custom' : g.platform;
        if (!byPlatform[p]) byPlatform[p] = { count: 0, installed: 0, playtime: 0 };
        byPlatform[p].count++;
        if (g.isInstalled) byPlatform[p].installed++;
        byPlatform[p].playtime += (g.playtimeSeconds || 0);
    });

    // Топ по времени в игре
    const topPlayed = [...games].filter(g => (g.playtimeSeconds || 0) > 0)
        .sort((a, b) => b.playtimeSeconds - a.playtimeSeconds).slice(0, 10);

    // Недавно сыгранные
    const recent = [...games].filter(g => (g.lastPlayed || 0) > 0)
        .sort((a, b) => b.lastPlayed - a.lastPlayed).slice(0, 8);

    // Статусы прохождения
    const completionCounts = {};
    games.forEach(g => {
        if (g.completionStatus) {
            completionCounts[g.completionStatus] = (completionCounts[g.completionStatus] || 0) + 1;
        }
    });

    // --- Рендер ---
    const statCard = (icon, value, label) => `
        <div class="stat-card">
            <i class="fa-solid ${icon} stat-icon"></i>
            <div class="stat-value">${value}</div>
            <div class="stat-label">${label}</div>
        </div>`;

    let html = `<div class="stats-cards">
        ${statCard('fa-gamepad', total, 'всего игр')}
        ${statCard('fa-download', installed, 'установлено')}
        ${statCard('fa-clock', formatPlaytime(totalPlaytime), 'время в игре')}
        ${statCard('fa-rocket', totalLaunches, 'запусков')}
        ${statCard('fa-star', favorites, 'избранных')}
        ${statCard('fa-ghost', neverPlayed, 'ни разу не запущено')}
    </div>`;

    // Платформы
    const maxCount = Math.max(...Object.values(byPlatform).map(p => p.count));
    let platRows = '';
    Object.entries(byPlatform).sort((a, b) => b[1].count - a[1].count).forEach(([name, p]) => {
        const width = Math.max(4, Math.round((p.count / maxCount) * 100));
        const color = PLATFORM_COLORS[name] || '#5e6ad2';
        const playtimeStr = p.playtime > 0 ? ` • ${formatPlaytimeShort(p.playtime)}` : '';
        platRows += `
            <div class="plat-row">
                <div class="plat-name"><i class="${PLATFORM_ICONS[name] || 'fa-solid fa-gamepad'}"></i> ${escapeHtml(name)}</div>
                <div class="plat-bar-track">
                    <div class="plat-bar" style="width:${width}%; background:${color};"></div>
                </div>
                <div class="plat-count">${p.count} игр (${p.installed} уст.)${playtimeStr}</div>
            </div>`;
    });
    html += `<div class="stats-section"><h3><i class="fa-solid fa-layer-group"></i> Платформы</h3>${platRows}</div>`;

    // Статусы прохождения
    if (Object.keys(completionCounts).length > 0) {
        let chips = '';
        COMPLETION_STATUSES.forEach(s => {
            if (s.value && completionCounts[s.value]) {
                const color = STATUS_COLORS[s.value] || '#888';
                chips += `<span class="status-stat-chip" style="border-color:${color};">
                    <span class="dot" style="background:${color};"></span>${s.label}: <b>${completionCounts[s.value]}</b></span>`;
            }
        });
        html += `<div class="stats-section"><h3><i class="fa-solid fa-list-check"></i> Прохождение</h3>
            <div class="status-stats">${chips}</div></div>`;
    }

    // Топ по времени
    if (topPlayed.length > 0) {
        let rows = '';
        topPlayed.forEach((g, i) => {
            const img = g.iconUrl ? g.iconUrl.replace(/\\/g, '/') : '';
            rows += `
                <div class="top-game-row" data-id="${escapeHtml(g.id)}">
                    <span class="top-rank">${i + 1}</span>
                    ${img ? `<img src="${img}" class="top-img" loading="lazy">` : '<div class="top-img"></div>'}
                    <span class="top-name">${escapeHtml(g.name)}</span>
                    <span class="top-time">${formatPlaytime(g.playtimeSeconds)}</span>
                </div>`;
        });
        html += `<div class="stats-section"><h3><i class="fa-solid fa-trophy"></i> Топ по времени в игре</h3>${rows}</div>`;
    }

    // Недавно сыгранные
    if (recent.length > 0) {
        let rows = '';
        recent.forEach(g => {
            const img = g.iconUrl ? g.iconUrl.replace(/\\/g, '/') : '';
            rows += `
                <div class="top-game-row" data-id="${escapeHtml(g.id)}">
                    <span class="top-rank"><i class="fa-solid fa-clock-rotate-left"></i></span>
                    ${img ? `<img src="${img}" class="top-img" loading="lazy">` : '<div class="top-img"></div>'}
                    <span class="top-name">${escapeHtml(g.name)}</span>
                    <span class="top-time">${formatLastPlayed(g.lastPlayed)}</span>
                </div>`;
        });
        html += `<div class="stats-section"><h3><i class="fa-solid fa-clock-rotate-left"></i> Недавно сыгранные</h3>${rows}</div>`;
    }

    if (topPlayed.length === 0 && totalLaunches === 0) {
        html += `<div class="stats-section" style="color:#888; font-size:14px;">
            <i class="fa-solid fa-circle-info"></i>
            Запускайте игры через swch — время в игре и статистика будут считаться автоматически.
        </div>`;
    }

    container.innerHTML = html;

    // Клик по игре в списках — открыть её карточку
    container.querySelectorAll('.top-game-row').forEach(row => {
        row.addEventListener('click', () => {
            switchTab('library');
            openGameModal(row.getAttribute('data-id'));
        });
    });
}