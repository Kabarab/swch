let globalGames = [];
        let editingTagTarget = { username: '', platform: '', gameId: '' };
        let editingAccountTarget = { username: '', platform: '' };
        let currentModalGameId = '';

        function switchTab(tab) {
            document.querySelectorAll('.view-section').forEach(e => e.style.display = 'none');
            document.querySelectorAll('.nav-item').forEach(e => e.classList.remove('active'));
            if(tab === 'library') {
                document.getElementById('view-library').style.display = 'block';
                loadLibrary();
            } else {
                document.getElementById('view-accounts').style.display = 'block';
                loadAccounts();
            }
        }

        function closeModal(id) { document.getElementById(id).style.display = 'none'; }

        async function loadLibrary() {
            const list = document.getElementById('games-grid');
            // не стираем innerHTML сразу, чтобы не моргало слишком сильно, если возможно
            const games = await window['go']['app']['App']['GetLibrary']();
            globalGames = games;
            list.innerHTML = '';

            if(!games.length) list.innerHTML = "No games found.";

            games.forEach(game => {
                const card = document.createElement('div');
                card.className = 'card';
                if (!game.isInstalled) card.classList.add('game-not-installed');

                card.onclick = () => openGameModal(game.id);
                
                let platColor = '#0074e4';
                if(game.platform === 'Steam') platColor = '#1b2838';
                if(game.platform === 'Epic') platColor = '#333';
                if(game.platform === 'Custom') platColor = '#2d8c58';

                const img = game.iconUrl || 'https://via.placeholder.com/300x169?text=' + encodeURIComponent(game.name);
                
                // Кнопка закрепления
                const pinClass = game.isPinned ? 'active' : '';
                const pinBtn = `<div class="pin-btn ${pinClass}" onclick="togglePin(event, '${game.id}')"><i class="fa-solid fa-thumbtack"></i></div>`;
                
                let overlay = '';
                if (!game.isInstalled) overlay = '<div class="install-overlay"><i class="fa-solid fa-download"></i></div>';

                card.innerHTML = `
                    <div class="platform-badge" style="background:${platColor}">${game.platform}</div>
                    ${pinBtn}
                    ${overlay}
                    <img src="${img}" class="card-img" style="height:150px; width:100%; object-fit:cover;">
                    <div class="card-info"><div class="game-title" style="font-weight:bold;">${game.name}</div></div>
                `;
                list.appendChild(card);
            });
        }

        async function togglePin(e, gameId) {
            e.stopPropagation();
            await window['go']['app']['App']['ToggleGamePin'](gameId);
            loadLibrary();
        }

        function openGameModal(gameId) {
            currentModalGameId = gameId;
            document.getElementById('show-hidden-accs').checked = false; // reset
            renderGameModalContent(gameId);
            document.getElementById('account-modal').style.display = 'flex';
        }

        function refreshGameModal() {
            if(currentModalGameId) renderGameModalContent(currentModalGameId);
        }

        function renderGameModalContent(gameId) {
            const game = globalGames.find(g => g.id === gameId);
            if(!game) return;
            
            // Если Custom игра - сразу запускаем
            if (game.platform === 'Custom') {
                launch('', game.id, game.platform, game.exePath);
                document.getElementById('account-modal').style.display = 'none';
                return;
            }

            const actionVerb = game.isInstalled ? "Launch" : "Install";
            const actionIcon = game.isInstalled ? "fa-play" : "fa-download";
            const showHidden = document.getElementById('show-hidden-accs').checked;

            document.getElementById('modal-game-title').innerText = `${actionVerb} ${game.name}`;
            const list = document.getElementById('modal-accounts-list');
            list.innerHTML = '';

            if (!game.availableOn || game.availableOn.length === 0) {
                list.innerHTML = `<div class="modal-item" onclick="launch('', '${game.id}', '${game.platform}', '')">
                    <div class="acc-name">${actionVerb} Game</div><div class="acc-meta">Current Account</div>
                </div>`;
            } else {
                game.availableOn.forEach(acc => {
                    // Фильтр скрытых
                    if (acc.isHidden && !showHidden) return;

                    const item = document.createElement('div');
                    item.className = 'modal-item';
                    if(acc.isHidden) item.classList.add('acc-hidden-row');
                    
                    let noteHtml = '';
                    if (acc.note === 'Main') noteHtml = `<span class="badge badge-main"><i class="fa-solid fa-crown"></i> MAIN</span>`;
                    else if (acc.note === 'Smurf') noteHtml = `<span class="badge badge-smurf"><i class="fa-solid fa-child"></i> SMURF</span>`;
                    else if (acc.note === 'VAC') noteHtml = `<span class="badge badge-vac"><i class="fa-solid fa-ban"></i> VAC BAN</span>`;
                    else if (acc.note) noteHtml = `<span class="badge badge-custom"><i class="fa-solid fa-note-sticky"></i> ${acc.note}</span>`;

                    // Кнопка удаления/восстановления
                    const hideIcon = acc.isHidden ? "fa-rotate-left" : "fa-trash";
                    const hideClass = acc.isHidden ? "toggle-restore-btn" : "toggle-hidden-btn";
                    const hideTitle = acc.isHidden ? "Restore account" : "Remove from list";

                    item.innerHTML = `
                        <div style="flex:1" onclick="launch('${acc.username}', '${game.id}', '${game.platform}', '')">
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

        async function toggleGameAccountHidden(username, platform, gameId) {
            await window['go']['app']['App']['ToggleGameAccountHidden'](username, platform, gameId);
            // Нужно обновить данные библиотеки, чтобы получить актуальный статус isHidden
            const games = await window['go']['app']['App']['GetLibrary']();
            globalGames = games;
            renderGameModalContent(gameId);
        }

        // Остальные функции без изменений
        async function launch(account, gameId, platform, exePath) {
            const res = await window['go']['app']['App']['LaunchGame'](account, gameId, platform, exePath);
            if(res && !res.startsWith("Launched") && !res.startsWith("Success")) alert(res);
            else closeModal('account-modal');
        }

        function openNoteSelector(username, platform, gameId, currentNote) {
            editingTagTarget = { username, platform, gameId };
            const isPredefined = ['Main','Smurf','VAC'].includes(currentNote);
            document.getElementById('custom-note-input').value = isPredefined ? '' : currentNote;
            document.getElementById('note-select-modal').style.display = 'flex';
        }
        async function savePredefinedNote(noteType) { await applyNote(noteType); }
        async function saveCustomNote() { const text = document.getElementById('custom-note-input').value; await applyNote(text); }
        async function applyNote(noteText) {
            const { username, platform, gameId } = editingTagTarget;
            await window['go']['app']['App']['UpdateGameNote'](username, platform, gameId, noteText);
            const games = await window['go']['app']['App']['GetLibrary']();
            globalGames = games;
            closeModal('note-select-modal');
            renderGameModalContent(gameId);
        }

        async function loadAccounts() {
            const container = document.getElementById('launchers-list');
            container.innerHTML = '<div style="padding:20px">Loading accounts...</div>';
            const launchers = await window['go']['app']['App']['GetLaunchers']();
            container.innerHTML = '';
            launchers.forEach(group => {
                const section = document.createElement('div');
                section.className = 'launcher-section';
                let accountsHtml = '';
                group.accounts.forEach(acc => {
                    let avatarHtml = `<div class="acc-avatar">${acc.displayName.charAt(0)}</div>`;
                    if(acc.avatarUrl) avatarHtml = `<div class="acc-avatar" style="background-image: url('${acc.avatarUrl.replace(/\\/g, '/')}'); background-size: cover; color: transparent;"></div>`;
                    const commentHtml = acc.comment ? `<div class="acc-comment">${acc.comment}</div>` : '';
                    accountsHtml += `
                        <div class="account-row interactable" onclick="switchAccount('${acc.username}', '${group.platform}')">
                            ${avatarHtml}
                            <div class="acc-details">
                                <div class="acc-nick">${acc.displayName}${commentHtml}</div>
                                <div class="acc-login">${acc.username}</div>
                            </div>
                            <div class="acc-actions" onclick="event.stopPropagation()">
                                <div class="action-icon-btn" onclick="openEditAccount('${acc.username}', '${group.platform}', '${acc.comment || ''}')"><i class="fa-solid fa-pen"></i></div>
                                <div class="action-icon-btn delete-btn" onclick="deleteAccount('${acc.username}', '${group.platform}')"><i class="fa-solid fa-trash"></i></div>
                            </div>
                            <div class="acc-action-icon"><i class="fa-solid fa-arrow-right-to-bracket"></i></div>
                        </div>`;
                });
                let footerHtml = '';
                if(group.platform === 'Epic') {
                    footerHtml = `<div style="padding:10px; text-align:center; border-top:1px solid #2a2a2a;"><button onclick="openSaveEpicModal()" style="background:transparent; border:1px dashed #444; color:#888; width:100%; padding:8px; cursor:pointer;"><i class="fa-solid fa-plus"></i> Save Current Epic Account</button></div>`;
                }
                section.innerHTML = `<div class="launcher-header"><i class="fa-brands fa-${group.platform.toLowerCase()}"></i> ${group.name} <span class="count">${group.accounts.length}</span></div><div class="launcher-accounts">${accountsHtml}</div>${footerHtml}`;
                container.appendChild(section);
            });
        }

        async function switchAccount(username, platform) { alert(await window['go']['app']['App']['SwitchToAccount'](username, platform)); }
        function openSaveEpicModal() { document.getElementById('epic-save-name').value = ''; document.getElementById('save-epic-modal').style.display = 'flex'; }
        async function doSaveEpic() { const name = document.getElementById('epic-save-name').value; if(!name) return; alert(await window['go']['app']['App']['SaveEpicAccount'](name)); closeModal('save-epic-modal'); loadAccounts(); }
        async function deleteAccount(username, platform) { if(confirm(`Hide account ${username}?`)) { await window['go']['app']['App']['DeleteAccount'](username, platform); loadAccounts(); } }
        function openEditAccount(username, platform, currentComment) { editingAccountTarget = { username, platform }; document.getElementById('edit-comment').value = currentComment; document.getElementById('edit-avatar-path').value = ''; document.getElementById('edit-account-modal').style.display = 'flex'; }
        async function selectNewAvatar() { const path = await window['go']['app']['App']['SelectImage'](); if(path) document.getElementById('edit-avatar-path').value = path; }
        async function saveAccountChanges() { await window['go']['app']['App']['UpdateAccountData'](editingAccountTarget.username, editingAccountTarget.platform, document.getElementById('edit-comment').value, document.getElementById('edit-avatar-path').value); closeModal('edit-account-modal'); loadAccounts(); }
        function openAddGameModal() { document.getElementById('add-game-modal').style.display = 'flex'; }
        async function browseFile() { const path = await window['go']['app']['App']['SelectExe'](); if(path) document.getElementById('custom-path').value = path; }
        async function saveCustomGame() { const res = await window['go']['app']['App']['AddCustomGame'](document.getElementById('custom-name').value, document.getElementById('custom-path').value); if(res === 'Success') { closeModal('add-game-modal'); loadLibrary(); document.getElementById('custom-name').value=''; document.getElementById('custom-path').value=''; } else { alert(res); } }

        loadLibrary();

import { RemoveGame, GetLibrary } from '../wailsjs/go/app/App';

// Переменные для хранения состояния
let selectedGameId = null;
let selectedPlatform = null;
const contextMenu = document.getElementById('context-menu');
const deleteBtn = document.getElementById('ctx-delete');

// 1. Скрытие меню при клике в любом месте
document.addEventListener('click', () => {
    if (contextMenu) contextMenu.style.display = 'none';
});

// 2. Обработка правого клика (открытие меню)
// Предполагаем, что ваши игры находятся в контейнере с id="app" или "game-list"
// Лучше вешать обработчик на документ, но фильтровать по классу карточки
document.addEventListener('contextmenu', (e) => {
    // Ищем ближайший элемент с классом, например, 'game-card' или 'card'
    // (ЗАМЕНИТЕ 'card' НА КЛАСС ВАШЕЙ КАРТОЧКИ ИГРЫ, если он другой)
    const card = e.target.closest('.card'); 

    if (card) {
        e.preventDefault(); // Блокируем стандартное меню браузера

        // Получаем ID и платформу из атрибутов карточки
        // ВАЖНО: При рендеринге вы должны были добавить эти атрибуты (см. шаг 4)
        selectedGameId = card.getAttribute('data-id');
        selectedPlatform = card.getAttribute('data-platform');

        // Если это не торрент/кастом игра, кнопку удаления можно скрыть или заблокировать
        if (selectedPlatform !== 'Torrent' && selectedPlatform !== 'Custom') {
            deleteBtn.style.display = 'none'; // Скрываем для Steam/Epic
        } else {
            deleteBtn.style.display = 'block'; // Показываем для своих игр
        }

        // Позиционируем меню
        contextMenu.style.top = `${e.pageY}px`;
        contextMenu.style.left = `${e.pageX}px`;
        contextMenu.style.display = 'block';
    } else {
        contextMenu.style.display = 'none';
    }
});

// 3. Логика удаления
deleteBtn.addEventListener('click', async () => {
    if (!selectedGameId || !selectedPlatform) return;

    // Подтверждение
    const confirmed = await confirm(`Удалить эту игру?`); 
    if (!confirmed) return;

    try {
        const result = await RemoveGame(selectedGameId, selectedPlatform);
        if (result === "Success") {
            console.log("Игра удалена");
            // ОБНОВИТЕ СПИСОК ИГР ЗДЕСЬ
            // Например: renderLibrary(); или window.location.reload();
            location.reload(); // Простой вариант: перезагрузить страницу
        } else {
            alert("Ошибка: " + result);
        }
    } catch (err) {
        console.error(err);
    }
});