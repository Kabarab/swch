chrome.runtime.onMessage.addListener((message, sender, reply) => {
  if (message?.type !== 'SWCH_STEAM_CONNECT') return;
  (async () => {
    const tab = new URL(sender.tab?.url || 'about:blank');
    if (tab.protocol !== 'http:' || !['127.0.0.1', 'localhost'].includes(tab.hostname) || tab.pathname !== '/steam/claim-connect') {
      throw new Error('Открой подключение из настроек SWCH.');
    }
    const params = new URLSearchParams(tab.hash.slice(1));
    const session = params.get('session'), account = params.get('account');
    if (!/^[a-f0-9]{64}$/.test(session || '') || !/^7656[0-9]{13}$/.test(account || '')) throw new Error('Некорректное подключение.');
    const url = 'https://store.steampowered.com/';
    const [login, csrf, region] = await Promise.all(['steamLoginSecure', 'sessionid', 'steamCountry'].map(name => chrome.cookies.get({url, name})));
    const refresh = await chrome.cookies.get({url:'https://login.steampowered.com/',name:'steamRefresh_steam'});
    if (!login?.value) throw new Error('Войди в магазин Steam в этом браузере и повтори подключение.');
    if (decodeURIComponent(login.value).split('||')[0] !== account) throw new Error('В браузере открыт другой аккаунт Steam. Войди в выбранный аккаунт.');
    const country = decodeURIComponent(region?.value || '').split('|')[0].toUpperCase();
    if (!/^[A-Z]{2}$/.test(country)) throw new Error('Открой страницу аккаунта Steam, чтобы магазин определил страну, и повтори.');
    const bytes = crypto.getRandomValues(new Uint8Array(12));
    const sessionId = csrf?.value || Array.from(bytes, n => n.toString(16).padStart(2, '0')).join('');
    let refreshToken = null;
    if (refresh?.value) {
      const value = decodeURIComponent(refresh.value).split('||');
      if (value.length === 2 && value[0] === account) refreshToken = value[1];
    }
    const response = await fetch(`${tab.origin}/api/free-games/steam/complete`, {
      method: 'POST', headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({session, account_id: account, login_secure: login.value, session_id: sessionId, country,refresh_token:refreshToken}),
      signal: AbortSignal.timeout(45000)
    });
    const data = await response.json();
    if (!response.ok || data.status !== 'connected') throw new Error(data.error || 'SWCH не подтвердил подключение.');
    reply({ok: true});
  })().catch(error => reply({ok: false, error: error.message || 'Не удалось подключить магазин Steam.'}));
  return true;
});
