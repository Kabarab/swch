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
    DeleteAccount, 
    SelectImage, 
    UpdateAccountData, 
    SelectExe, 
    AddCustomGame, 
    AddCustomGame, 
    SetGameImage,
    // EPIC NATIVE
    EpicLogin,
    EpicLoginAuto,
    SaveRiotAccount,
    GetClipboardText
} from '../wailsjs/go/app/App';

// --- Глобальные переменные ---
let globalGames = [];
let editingTagTarget = { username: '', platform: '', gameId: '' };
let editingAccountTarget = { username: '', platform: '' };
let currentModalGameId = '';
let showHiddenEpic = false;

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

// --- Инициализация ---

// Запуск при старте страницы
document.addEventListener("DOMContentLoaded", () => {
    loadLibrary();
    document.querySelector('.nav-item[onclick="switchTab(\'library\')"]').classList.add('active');

    const launchersList = document.getElementById('launchers-list');
    if (launchersList) {
        launchersList.addEventListener('click', function(event) {
            console.log('[DEBUG] Click detected on accounts list. Target:', event.target);
            const target = event.target;

            // 1. Проверяем кнопку удаления
            const deleteButton = target.closest('.delete-account-btn');
            if (deleteButton) {
                console.log('[DEBUG] Hide button clicked. Username:', deleteButton.dataset.username, 'Platform:', deleteButton.dataset.platform);
                const username = deleteButton.dataset.username;
                const platform = deleteButton.dataset.platform;
                window.hideAccount(username, platform); // Вызываем новую функцию
                return; // Останавливаем дальнейшую обработку
            }

            // 2. Проверяем кнопку редактирования
            const editButton = target.closest('.edit-account-btn');
            if (editButton) {
                console.log('[DEBUG] Edit button found. Username:', editButton.dataset.username);
                const username = editButton.dataset.username;
                const platform = editButton.dataset.platform;
                const comment = editButton.dataset.comment;
                window.openEditAccount(username, platform, comment);
                return; // Останавливаем дальнейшую обработку
            }

            // 3. Если не кнопка, проверяем клик по самой строке для переключения
            const accountRow = target.closest('.account-row');
            if (accountRow) {
                console.log('[DEBUG] Account row clicked. Username:', accountRow.dataset.username);
                const username = accountRow.dataset.username;
                const platform = accountRow.dataset.platform;
                const isHidden = accountRow.dataset.hidden === 'true';

                // Убедимся, что клик был по валидной строке с данными
                if (username && platform) { 
                    if (isHidden) {
                        if (confirm(`Restore account ${username}?`)) {
                            window.restoreAccount(username, platform);
                        }
                    } else {
                        window.switchAccount(username, platform);
                    }
                }
            }
        });
    }
});

// --- Функции навигации и интерфейса ---

window.toggleHiddenEpic = function() {
    showHiddenEpic = !showHiddenEpic;
    loadAccounts();
}

// Переключение вкладок (Библиотека / Аккаунты)
window.switchTab = function(tab) {
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
window.closeModal = function(id) {
    document.getElementById(id).style.display = 'none';
}

window.openAddGameModal = function() {
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
window.toggleFilter = function(btn, platform) {
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
window.toggleBooleanFilter = function(btn) {
    btn.classList.toggle('active');
    if (btn.id === 'filter-installed') filterState.onlyInstalled = btn.classList.contains('active');
    if (btn.id === 'filter-macos') filterState.onlyMac = btn.classList.contains('active');
    applyFilters();
}

// Применяет фильтры к globalGames и вызывает рендер
window.applyFilters = function() {
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
window.togglePin = async function(e, gameId) {
    if (e) e.stopPropagation();
    await ToggleGamePin(gameId);
    loadLibrary(); // Перезагружаем, чтобы обновить состояние isPinned с бэкенда
}

// Открытие модального окна игры
window.openGameModal = function(gameId) {
    currentModalGameId = gameId;
    const checkbox = document.getElementById('show-hidden-accs');
    if (checkbox) checkbox.checked = false;
    
    renderGameModalContent(gameId);
    document.getElementById('account-modal').style.display = 'flex';
}

window.refreshGameModal = function() {
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
window.launch = async function(account, gameId, platform, exePath) {
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
window.toggleGameAccountHidden = async function(username, platform, gameId) {
    await ToggleGameAccountHidden(username, platform, gameId);
    // Обновляем данные глобально
    await loadLibrary();
    // Перерисовываем модалку
    renderGameModalContent(gameId);
}

// --- Управление заметками (Notes) ---

window.openNoteSelector = function(username, platform, gameId, currentNote) {
    editingTagTarget = { username, platform, gameId };
    const isPredefined = ['Main', 'Smurf', 'VAC'].includes(currentNote);
    document.getElementById('custom-note-input').value = isPredefined ? '' : currentNote;
    document.getElementById('note-select-modal').style.display = 'flex';
}

window.savePredefinedNote = async function(noteType) { 
    await applyNote(noteType); 
}

window.saveCustomNote = async function() { 
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
            container.innerHTML = '<div style="padd ing:20px">No launchers found.</div>';
            return;
        }
        
        launchers.forEach(group => {
            const section = document.createElement('div');
            section.className = 'launcher-section';
            
            let accountsHtml = '';
            const accounts = group.accounts || [];
            
            accounts.forEach(acc => {
                // Не показывать скрытые аккаунты
                const isHidden = acc.comment && acc.comment.startsWith('_swch_hidden_');
                if (isHidden) {
                    if (group.platform !== 'Epic' || !showHiddenEpic) return;
                }

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
                
                accountsHtml += `
                    <div class="account-row interactable" style="${rowStyle}" data-username="${acc.username}" data-platform="${group.platform}" data-hidden="${isHidden}">
                        ${avatarHtml}
                        <div class="acc-details">
                            <div class="acc-nick">${acc.displayName}${commentHtml}</div>
                            <div class="acc-login">${acc.username}</div>
                        </div>
                        <div class="acc-actions">
                            <div class="action-icon-btn edit-account-btn" data-username="${acc.username}" data-platform="${group.platform}" data-comment="${acc.comment || ''}"><i class="fa-solid fa-pen"></i></div>
                            <div class="action-icon-btn delete-btn delete-account-btn" data-username="${acc.username}" data-platform="${group.platform}"><i class="fa-solid fa-trash"></i></div>
                        </div>
                        <div class="acc-action-icon"><i class="fa-solid fa-arrow-right-to-bracket"></i></div>
                    </div>`;
            });
            
            // Кнопка для добавления (Footer)
            let footerHtml = '';
            
            // Для Epic Games (Official)
            if (group.platform === 'Epic') {
                const hiddenText = showHiddenEpic ? "Hide Hidden" : "Show Hidden";
                const hiddenIcon = showHiddenEpic ? "fa-eye-slash" : "fa-eye";

                footerHtml = `<div style="padding:10px; text-align:center; border-top:1px solid #2a2a2a; display:grid; grid-template-columns: 1fr 1fr; gap:10px;">
                    <button onclick="openEpicSaveModal()" style="cursor:pointer; background:none; color:#aaa; border:1px dashed #444; padding:8px; border-radius:4px;">
                        <i class="fa-solid fa-plus"></i> Add Current
                    </button>
                    <button onclick="toggleHiddenEpic()" style="cursor:pointer; background:none; color:#aaa; border:1px dashed #444; padding:8px; border-radius:4px;">
                        <i class="fa-solid ${hiddenIcon}"></i> ${hiddenText}
                    </button>
                </div>`;
            }
            
            let iconClass = "fa-gamepad";
            if (group.platform === "Steam") iconClass = "fa-steam";
            if (group.platform === "Epic") iconClass = "fa-bolt"; 
            if (group.platform === "Legendary") iconClass = "fa-terminal"; // Иконка для Legendary
            
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

// Переключение аккаунта
window.switchAccount = async function(username, platform) { 
    const result = await SwitchToAccount(username, platform);
    alert(result);
}

// Удаление аккаунта
// Скрывает аккаунт, а не удаляет, помечая его специальным комментарием
window.hideAccount = async function(username, platform) {
    const reason = prompt("Reason for hiding (optional, e.g. Ban):", "");
    if (reason === null) return;

    const comment = reason ? `_swch_hidden_:${reason}` : "_swch_hidden_";

    try {
        // Используем UpdateAccountData чтобы установить специальный комментарий
        // Пустой стринг для avatarPath, чтобы он не менялся
        const res = await UpdateAccountData(username, platform, comment, ""); 
        
        if (res && (res.startsWith("Error") || res.startsWith("Ошибка"))) {
            alert("Ошибка при скрытии аккаунта: " + res);
        } else {
            loadAccounts(); // Перезагружаем список, чтобы скрытый аккаунт исчез
        }
    } catch (e) {
        alert('An error occurred while hiding the account: ' + e);
    }
}

window.restoreAccount = async function(username, platform) {
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
window.openEditAccount = function(username, platform, currentComment) { 
    editingAccountTarget = { username, platform }; 
    document.getElementById('edit-comment').value = currentComment; 
    document.getElementById('edit-avatar-path').value = ''; 
    document.getElementById('edit-account-modal').style.display = 'flex'; 
}

window.selectNewAvatar = async function() { 
    const path = await SelectImage(); 
    if (path) document.getElementById('edit-avatar-path').value = path; 
}

window.saveAccountChanges = async function() { 
    await UpdateAccountData(
        editingAccountTarget.username, 
        editingAccountTarget.platform, 
        document.getElementById('edit-comment').value, 
        document.getElementById('edit-avatar-path').value
    ); 
    closeModal('edit-account-modal'); 
    loadAccounts(); 
}

// --- Добавление игр (Custom / Torrent) ---

window.browseFile = async function() { 
    const path = await SelectExe(); 
    if (path) document.getElementById('custom-path').value = path; 
}

window.saveCustomGame = async function() { 
    const res = await AddCustomGame(
        document.getElementById('custom-name').value, 
        document.getElementById('custom-path').value
    ); 
    
    } else { 
        alert(res); 
    } 
}

// --- Добавление Epic Account ---

window.openEpicSaveModal = function() {
    document.getElementById('epic-save-name').value = '';
    document.getElementById('save-epic-modal').style.display = 'flex';
}

window.doSaveEpic = async function() {
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
window.openAddAccountModal = function() {
    document.getElementById('add-account-modal').style.display = 'flex';
}

// 1. Epic Native Login
window.startEpicNativeLogin = async function() {
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
    document.getElementById('input-sid-native').value = '';
}

// 2. Riot Save Logic (Аналогично Epic)
window.openRiotSaveModal = function() {
    document.getElementById('riot-save-name').value = '';
    document.getElementById('save-riot-modal').style.display = 'flex';
}

window.doSaveRiot = async function() {
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

window.openLegendarySaveModal = function() {
    document.getElementById('legendary-save-name').value = '';
    document.getElementById('save-legendary-modal').style.display = 'flex';
}

window.doSaveLegendary = async function() {
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
        const confirmed = confirm(`Удалить эту игру из библиотеки?`);
        if (!confirmed) return;

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