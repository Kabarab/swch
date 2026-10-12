const bridgeParams = new URLSearchParams(location.hash.slice(1));
const bridgeAccount = bridgeParams.get('account');
const bridgeButton = document.getElementById('steam-claim-connect');
const bridgeStatus = document.getElementById('steam-claim-status');
document.getElementById('steam-claim-account').textContent = `SteamID: ${bridgeAccount || 'не указан'}`;
window.addEventListener('message', event => {
  if (event.source !== window || event.origin !== location.origin) return;
  if (event.data?.type === 'SWCH_STEAM_EXTENSION_READY') {
    bridgeButton.disabled = false;
    bridgeStatus.textContent = 'Расширение подключено. Войди в выбранный аккаунт Steam и нажми кнопку.';
  }
  if (event.data?.type === 'SWCH_STEAM_CONNECT_RESULT') {
    bridgeButton.disabled = event.data.ok === true;
    bridgeStatus.textContent = event.data.ok ? 'Аккаунт подключён. Можно закрыть страницу и вернуться в SWCH.' : event.data.error || 'Не удалось подключить аккаунт.';
    if (event.data.ok) history.replaceState(null, '', location.pathname);
  }
});
bridgeButton.addEventListener('click', () => {
  bridgeButton.disabled = true;
  bridgeStatus.textContent = 'Проверяем вход Steam…';
  window.postMessage({type: 'SWCH_STEAM_CONNECT_REQUEST'}, location.origin);
});
