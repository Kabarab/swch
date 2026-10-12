'use strict';
(() => {
  const id = new URLSearchParams(location.hash.slice(1)).get('session');
  const status = document.getElementById('bridge-status');
  const title = document.getElementById('bridge-title');
  const install = document.getElementById('helper-install');
  const cancel = document.getElementById('bridge-cancel');
  let stopped = false;
  if (!/^[a-f0-9]{64}$/.test(id || '')) {
    status.textContent = 'Начни вход кнопкой «Войти в Epic Games» в SWCH.';
    cancel.hidden = true;
    return;
  }
  async function call(path) {
    const response = await fetch(`/api/epic/auth/browser/${path}`, {
      method: 'POST', headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({session_id: id}), cache: 'no-store', signal: AbortSignal.timeout(10000),
    });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error || 'SWCH недоступен.');
    return data;
  }
  window.addEventListener('swch-epic-helper', event => {
    if (stopped) return;
    const state = event.detail?.status;
    if (state === 'ready' || state === 'opened') {
      install.hidden = true;
      status.textContent = 'Войди в Epic Games в открывшемся окне. Аккаунт появится в SWCH автоматически.';
    } else if (state === 'error') {
      status.textContent = event.detail.error || 'Не удалось открыть окно входа. Начни вход заново в SWCH.';
    }
  });
  setTimeout(() => {
    if (!stopped && document.documentElement.dataset.swchEpicHelper !== 'ready') {
      install.hidden = false;
      status.textContent = 'Для автоматического добавления нужно расширение SWCH. Также можно подключить аккаунт по коду в приложении.';
    }
  }, 1500);
  document.getElementById('helper-reload').addEventListener('click', () => location.reload());
  cancel.addEventListener('click', async () => {
    cancel.disabled = true;
    try { show(await call('cancel')); }
    catch (error) { status.textContent = error.message; cancel.disabled = false; }
  });
  function show(data) {
    if (data.status === 'authenticating') {
      status.textContent = 'Подключаем аккаунт к SWCH…'; cancel.disabled = true;
    } else if (data.status === 'connected') {
      title.textContent = 'Аккаунт подключён';
      status.textContent = `${data.account?.display_name || 'Epic Games'} добавлен в SWCH. Можно закрыть эту страницу.`;
      stopped = true; install.hidden = true; cancel.hidden = true;
    } else if (['cancelled', 'expired', 'failed'].includes(data.status)) {
      status.textContent = data.error || (data.status === 'cancelled' ? 'Вход отменён.' : 'Время входа истекло. Начни вход заново в SWCH.');
      stopped = true; install.hidden = true; cancel.hidden = true;
    }
  }
  async function poll() {
    if (stopped) return;
    try { show(await call('status')); }
    catch (error) { status.textContent = error.message; stopped = true; }
    if (!stopped) setTimeout(poll, 1000);
  }
  poll();
})();
