import {
    LegendaryLogin,
    LegendaryCheckStatus,
    GetLegendaryLibrary,
    LegendaryInstallGame,
    LegendaryLaunchGame,
    LegendaryLogout,
    LegendaryLoginWithBrowser
} from '../wailsjs/go/app/App';

let legendaryGames = [];

export function setupLegendary() {
    console.log("Setting up Legendary tab...");
    // Initial status check
    checkLegendaryStatus();

    // Add event listeners for buttons
    document.getElementById('legendary-login-btn').addEventListener('click', loginWithLegendary);
    document.getElementById('legendary-login-browser-btn').addEventListener('click', loginWithBrowser);
    document.getElementById('legendary-logout-btn').addEventListener('click', logoutFromLegendary);
    document.getElementById('legendary-refresh-btn').addEventListener('click', fetchLegendaryGames);

    // Initial games list fetch
    fetchLegendaryGames();
}

async function checkLegendaryStatus() {
    const loggedIn = await LegendaryCheckStatus();
    const loginView = document.getElementById('legendary-login-view');
    const mainView = document.getElementById('legendary-main-view');
    
    if (loggedIn) {
        loginView.style.display = 'none';
        mainView.style.display = 'block';
        fetchLegendaryGames();
    } else {
        loginView.style.display = 'block';
        mainView.style.display = 'none';
    }
}

async function loginWithLegendary() {
    const sid = document.getElementById('legendary-sid-input').value;
    if (!sid) {
        alert("Please enter your SID.");
        return;
    }
    const result = await LegendaryLogin(sid);
    if (result === "Success") {
        alert("Login successful!");
        checkLegendaryStatus();
    } else {
        alert("Login failed: " + result);
    }
}

async function loginWithBrowser() {
    alert("Your browser will now open to the Epic Games login page. Please log in to continue.");
    const result = await LegendaryLoginWithBrowser();
    if (result === "Success") {
        alert("Login successful!");
        checkLegendaryStatus();
    } else {
        alert("Login failed: " + result);
    }
}

async function logoutFromLegendary() {
    await LegendaryLogout();
    alert("Logged out.");
    checkLegendaryStatus();
    const gamesList = document.getElementById('legendary-games-list');
    gamesList.innerHTML = '';
}

async function fetchLegendaryGames() {
    const statusDiv = document.getElementById('legendary-status');
    statusDiv.textContent = 'Loading games...';
    try {
        const games = await GetLegendaryLibrary();
        legendaryGames = games || [];
        renderLegendaryGames();
        statusDiv.textContent = '';
    } catch (err) {
        console.error("Error fetching Legendary games:", err);
        statusDiv.textContent = 'Failed to load games.';
    }
}

function renderLegendaryGames() {
    const list = document.getElementById('legendary-games-list');
    list.innerHTML = '';

    if (legendaryGames.length === 0) {
        list.innerHTML = '<p>No games found.</p>';
        return;
    }

    legendaryGames.forEach(game => {
        const item = document.createElement('div');
        item.className = 'legendary-game-item';

        const actionBtn = game.installed
            ? `<button class="game-btn launch" data-appid="${game.id}">Launch</button>`
            : `<button class="game-btn install" data-appid="${game.id}">Install</button>`;

        item.innerHTML = `
            <img src="${game.image}" class="game-image">
            <div class="game-info">
                <div class="game-title">${game.title}</div>
                <div class="game-status">${game.installed ? 'Installed' : 'Not Installed'}</div>
            </div>
            ${actionBtn}
        `;
        list.appendChild(item);
    });

    // Add event listeners for the new buttons
    list.querySelectorAll('.game-btn').forEach(btn => {
        btn.addEventListener('click', (e) => {
            const appid = e.target.dataset.appid;
            if (btn.classList.contains('launch')) {
                launchLegendaryGame(appid);
            } else {
                installLegendaryGame(appid);
            }
        });
    });
}

async function launchLegendaryGame(appid) {
    alert(`Launching ${appid}...`);
    const result = await LegendaryLaunchGame(appid);
    if (result !== "Launched") {
        alert("Failed to launch game: " + result);
    }
}

async function installLegendaryGame(appid) {
    alert(`Starting installation for ${appid}...`);
    const result = await LegendaryInstallGame(appid);
    if (result === "Installation Started") {
        alert("Installation has started. This may take a while.");
        // Refresh the list after some time
        setTimeout(fetchLegendaryGames, 5000); 
    } else {
        alert("Failed to start installation: " + result);
    }
}
