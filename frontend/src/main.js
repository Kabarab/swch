// 1. Импорты (все функции из Go)
<<<<<<< HEAD
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
=======
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
    DeleteAccount, 
    SelectImage, 
    UpdateAccountData, 
    SelectExe, 
    AddCustomGame, 
    AddCustomGame, 
>>>>>>> 680a380e908c9ccfac745e3ec2a614537e65a566
    SetGameImage,
    // EPIC NATIVE
    EpicLogin,
    EpicLoginAuto,
    SaveRiotAccount,
<<<<<<< HEAD
    OpenLegendaryLoginPage,
    GetClipboardText,
    GetEpicAuthURL,
    AuthorizeEpicWithCode,

    OpenEpicAuthPage,
    SaveRobloxAccount
=======
    GetClipboardText
>>>>>>> 680a380e908c9ccfac745e3ec2a614537e65a566
} from '../wailsjs/go/app/App';

// --- Глобальные переменные ---
let globalGames = [];
let editingTagTarget = { username: '', platform: '', gameId: '' };
let editingAccountTarget = { username: '', platform: '' };
let currentModalGameId = '';
let showHiddenAccounts = false;

// Переменные для контекстного меню
let selectedGameId = null;
let selectedPlatform = null;

// Состояние фильтров
let filterState = {
    platforms: ['Steam', 'Epic', 'Riot', 'Custom', 'Torrent'], // По умолчанию все включены
    onlyInstalled: false,
    onlyMac: false,
    searchQuery: ''
};

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

// Переключение вкладок (Библиотека / Аккаунты)
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
        applyFilters(); // Фильтруем и рисуем
    } catch (err) {
        console.error("Ошибка загрузки библиотеки:", err);
    }
}

// Управление фильтрами платформ
window.toggleFilter = function (btn, platform) {
    btn.classList.toggle('active');

    // Custom включает в себя и Torrent для удобства пользователя
    const platformsToCheck = platform === 'Custom' ? ['Custom', 'Torrent'] : [platform];

    if (btn.classList.contains('active')) {
        // Добавляем платформу(ы) в массив, если их там нет
        platformsToCheck.forEach(p => {
            if (!filterState.platforms.includes(p)) filterState.platforms.push(p);
        });
    } else {
        // Удаляем
        filterState.platforms = filterState.platforms.filter(p => !platformsToCheck.includes(p));
    }
    applyFilters();
}

// Управление булевыми фильтрами (Installed / macOS)
window.toggleBooleanFilter = function (btn) {
    btn.classList.toggle('active');
    if (btn.id === 'filter-installed') filterState.onlyInstalled = btn.classList.contains('active');
    if (btn.id === 'filter-macos') filterState.onlyMac = btn.classList.contains('active');
    applyFilters();
}

// Применяет фильтры к globalGames и вызывает рендер
window.applyFilters = function () {
    const searchInput = document.getElementById('filter-search');
    filterState.searchQuery = searchInput ? searchInput.value.toLowerCase() : '';

    const filtered = globalGames.filter(game => {
        // 1. Фильтр платформ
        let pCheck = game.platform;
        if (!filterState.platforms.includes(pCheck)) return false;

        // 2. Только установленные
        if (filterState.onlyInstalled && !game.isInstalled) return false;

        // 3. Поддержка macOS
        if (filterState.onlyMac && !game.isMacSupported) return false;

        // 4. Поиск по названию
        if (filterState.searchQuery && !game.name.toLowerCase().includes(filterState.searchQuery)) return false;

        return true;
    });

    renderGrid(filtered);
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
        let platColor = '#0074e4';
        if (game.platform === 'Steam') platColor = '#1b2838';
        if (game.platform === 'Epic') platColor = '#333';

        // Обработка картинки
        let img = 'https://via.placeholder.com/300x169?text=' + encodeURIComponent(game.name);
        if (game.iconUrl) {
            img = game.iconUrl.replace(/\\/g, '/');
        }

        const pinClass = game.isPinned ? 'active' : '';
        const pinBtn = `<div class="pin-btn ${pinClass}" onclick="window.togglePin(event, '${game.id}')"><i class="fa-solid fa-thumbtack"></i></div>`;

        let overlay = '';
        if (!game.isInstalled) overlay = '<div class="install-overlay"><i class="fa-solid fa-download"></i></div>';

        // Иконка Apple если игра поддерживает macOS
        let macBadge = '';
        if (game.isMacSupported) macBadge = `<i class="fa-brands fa-apple" style="margin-right:5px;" title="macOS Supported"></i>`;

        card.innerHTML = `
            <div class="platform-badge" style="background:${platColor}">
                ${macBadge}${game.platform}
            </div>
            ${pinBtn}
            ${overlay}
            <img src="${img}" class="card-img" style="height:150px; width:100%; object-fit:cover;">
            <div class="card-info">
                <div class="game-title" title="${game.name}">${game.name}</div>
            </div>
        `;
        list.appendChild(card);
    });
}

// Закрепление игры
window.togglePin = async function (e, gameId) {
    if (e) e.stopPropagation();
    await ToggleGamePin(gameId);
    loadLibrary(); // Перезагружаем, чтобы обновить состояние isPinned с бэкенда
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

// Рендер содержимого модального окна (список аккаунтов)
function renderGameModalContent(gameId) {
    const game = globalGames.find(g => g.id === gameId);
    if (!game) return;

    // Если это Custom игра, сразу запускаем (там нет списка аккаунтов)
    if (game.platform === 'Custom' || game.platform === 'Torrent') {
        launch('', game.id, game.platform, game.exePath);
        document.getElementById('account-modal').style.display = 'none';
        return;
    }

    const actionVerb = game.isInstalled ? "Launch" : "Install";
    const actionIcon = game.isInstalled ? "fa-play" : "fa-download";

    // Проверяем галочку "Show Hidden"
    const checkbox = document.getElementById('show-hidden-accs');
    const showHidden = checkbox ? checkbox.checked : false;

    document.getElementById('modal-game-title').innerText = `${actionVerb} ${game.name}`;
    const list = document.getElementById('modal-accounts-list');
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
                footerHtml = `<div style="padding:10px; text-align:center; border-top:1px solid #2a2a2a; display:grid; grid-template-columns: 1fr 1fr 1fr 1fr; gap:10px;">
                    <button onclick="openEpicSaveModal()" style="cursor:pointer; background:none; color:#aaa; border:1px dashed #444; padding:8px; border-radius:4px;">
                        <i class="fa-solid fa-plus"></i> Current
                    </button>
                    <button onclick="openEpicAuthModal()" style="cursor:pointer; background:none; color:#4f4; border:1px dashed #4f4; padding:8px; border-radius:4px;">
                        <i class="fa-solid fa-key"></i> Auth
                    </button>
                    <button onclick="logoutEpic()" style="cursor:pointer; background:none; color:#f44; border:1px dashed #f44; padding:8px; border-radius:4px;">
                        <i class="fa-solid fa-right-from-bracket"></i> Logout
                    </button>
                    ${showHiddenBtn}
                </div>`;
<<<<<<< HEAD
            } else if (group.platform === 'Legendary') {
                footerHtml = `<div style="padding:10px; text-align:center; border-top:1px solid #2a2a2a; display:grid; grid-template-columns: 1fr 1fr; gap:10px;">
                    <button onclick="openLegendarySaveModal()" style="cursor:pointer; background:none; color:#aaa; border:1px dashed #444; padding:8px; border-radius:4px;">
                        <i class="fa-solid fa-plus"></i> Save Session
                    </button>
                    ${showHiddenBtn}
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

=======
            }
            
>>>>>>> 680a380e908c9ccfac745e3ec2a614537e65a566
            let iconClass = "fa-gamepad";
            if (group.platform === "Steam") iconClass = "fa-steam";
            if (group.platform === "Epic") iconClass = "fa-bolt";
            if (group.platform === "Legendary") iconClass = "fa-terminal"; // Иконка для Legendary
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
<<<<<<< HEAD
    );

    if (res === 'Success') {
        closeModal('add-game-modal');
        loadLibrary();
        document.getElementById('custom-name').value = '';
        document.getElementById('custom-path').value = '';
    } else {
        alert(res);
    }
=======
    ); 
    
    } else { 
        alert(res); 
    } 
>>>>>>> 680a380e908c9ccfac745e3ec2a614537e65a566
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

// Открытие главного меню добавления
window.openAddAccountModal = function () {
    document.getElementById('add-account-modal').style.display = 'flex';
}

<<<<<<< HEAD
// 1. Legendary Login
window.startLegendaryLogin = async function () {
=======
// 1. Epic Native Login
window.startEpicNativeLogin = async function() {
>>>>>>> 680a380e908c9ccfac745e3ec2a614537e65a566
    closeModal('add-account-modal');

    let modal = document.getElementById('epic-native-auth-modal');
    if (!modal) {
        modal = document.createElement('div');
        modal.id = 'epic-native-auth-modal';
        modal.style.cssText = "position:fixed; top:0; left:0; width:100%; height:100%; background:rgba(0,0,0,0.8); display:flex; justify-content:center; align-items:center; z-index:2000;";
        modal.innerHTML = `
        <div style="background:#1e1e1e; padding:25px; border-radius:8px; width:450px; color:#fff; font-family:sans-serif; border:1px solid #333; box-shadow: 0 4px 15px rgba(0,0,0,0.5);">
            <h3 style="margin-top:0; margin-bottom:15px; color:#0074e4;"><i class="fa-solid fa-bolt"></i> Epic Games Login</h3>
            
            <div style="margin-bottom:20px;">
                <p style="color:#ccc; font-size:14px; margin-bottom:15px;">Login to your Epic Games account to sync your library.</p>
                <button id="btn-auto-epic-native" style="width:100%; padding:15px; background:#0074e4; color:#fff; border:none; cursor:pointer; border-radius:4px; font-weight:bold; font-size:16px;">
                    <i class="fa-solid fa-magic"></i> Start Automatic Login
                </button>
            </div>
            
            <div style="margin-bottom:20px; border-top: 1px solid #333; padding-top:15px;">
                <p style="color:#888; font-size:12px; margin-bottom:5px;">Or paste SID manually if automatic login fails:</p>
                <input type="password" id="input-sid-native" placeholder='Paste SID here...' style="width:100%; background:#111; color:#fff; border:1px solid #444; border-radius:4px; padding:10px; margin-bottom:10px;">
                <button id="btn-submit-sid-native" style="width:100%; padding:10px; background:#333; color:#fff; border:1px solid #555; cursor:pointer; border-radius:4px;">Login with SID</button>
            </div>
            
            <div style="text-align:right;">
                <button id="btn-cancel-epic-native" style="padding:8px 20px; background:none; border:none; color:#aaa; cursor:pointer;">Cancel</button>
            </div>
        </div>`;
        document.body.appendChild(modal);

        document.getElementById('btn-auto-epic-native').onclick = async () => {
            const res = await EpicLoginAuto();
            if (res === "Success") {
                modal.style.display = 'none';
                loadLibrary();
                loadAccounts();
            } else {
                alert(res);
            }
        };
<<<<<<< HEAD
        document.getElementById('btn-cancel-sid').onclick = () => {
            modal.style.display = 'none';
            if (window.legendaryAuthInterval) clearInterval(window.legendaryAuthInterval);
        };
        document.getElementById('btn-submit-sid').onclick = async () => {
            const input = document.getElementById('input-sid').value.trim();
            if (!input) return;
=======
>>>>>>> 680a380e908c9ccfac745e3ec2a614537e65a566

        document.getElementById('btn-cancel-epic-native').onclick = () => {
            modal.style.display = 'none';
        };

        document.getElementById('btn-submit-sid-native').onclick = async () => {
            const sid = document.getElementById('input-sid-native').value.trim();
            if (!sid) return;
            const res = await EpicLogin(sid);
            if (res === "Success") {
                modal.style.display = 'none';
                loadLibrary();
                loadAccounts();
            } else {
                alert(res);
            }
        };
    }

    modal.style.display = 'flex';
<<<<<<< HEAD
    document.getElementById('input-sid').value = ''; // Очистка поля

    // Запускаем проверку буфера обмена
    if (window.legendaryAuthInterval) clearInterval(window.legendaryAuthInterval);
    window.legendaryAuthInterval = setInterval(async () => {
        if (modal.style.display === 'none') {
            clearInterval(window.legendaryAuthInterval);
            return;
        }
        try {
            const text = await GetClipboardText();
            if (text && (text.includes('"authorizationCode"') || text.includes('"sid"') || text.includes('"code"'))) {
                // Если нашли JSON с кодом
                document.getElementById('input-sid').value = text;

                // Автоматически нажимаем Login
                const btn = document.getElementById('btn-submit-sid');
                if (btn) {
                    clearInterval(window.legendaryAuthInterval);
                    btn.click();
                }
            }
        } catch (e) {
            // ignore
        }
    }, 1000);
=======
    document.getElementById('input-sid-native').value = '';
>>>>>>> 680a380e908c9ccfac745e3ec2a614537e65a566
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

// --- Логика сохранения Legendary (NEW) ---

window.openLegendarySaveModal = function () {
    document.getElementById('legendary-save-name').value = '';
    document.getElementById('save-legendary-modal').style.display = 'flex';
}

window.doSaveLegendary = async function () {
    const name = document.getElementById('legendary-save-name').value;
    if (!name) {
        alert("Please enter a name");
        return;
    }

    try {
        // Вызываем Go функцию SaveLegendaryAccount
        const result = await SaveLegendaryAccount(name);
        if (result === "Success") {
            closeModal('save-legendary-modal');
            alert("Legendary account saved!");
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

        // Логика: удалять и менять иконку можно только у Custom/Torrent игр
        const isCustom = (selectedPlatform === 'Torrent' || selectedPlatform === 'Custom');

        if (!isCustom) {
            if (deleteBtn) deleteBtn.style.display = 'none';
            if (changeIconBtn) changeIconBtn.style.display = 'none';
            if (!deleteBtn && !changeIconBtn) {
                contextMenu.style.display = 'none';
                return;
            }
        } else {
            if (deleteBtn) deleteBtn.style.display = 'block';
            if (changeIconBtn) changeIconBtn.style.display = 'block';
        }

        // Позиционирование меню
        contextMenu.style.top = `${e.pageY}px`;
        contextMenu.style.left = `${e.pageX}px`;
        contextMenu.style.display = 'block';
    } else {
        if (contextMenu) contextMenu.style.display = 'none';
    }
});

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