'use strict';
(() => {
  if (location.pathname !== '/epic/connect' || window !== window.top) return;
  const ext = globalThis.browser || chrome;
  const session_id = new URLSearchParams(location.hash.slice(1)).get('session');
  if (!/^[a-f0-9]{64}$/.test(session_id || '')) return;
  document.documentElement.dataset.swchEpicHelper = 'ready';
  window.dispatchEvent(new CustomEvent('swch-epic-helper', {detail: {status: 'ready'}}));
  ext.runtime.sendMessage({type: 'swch-epic-begin', session_id}, result => {
    const error = ext.runtime.lastError;
    window.dispatchEvent(new CustomEvent('swch-epic-helper', {
      detail: error ? {status: 'error', error: 'Расширение отключено. Перезагрузи страницу.'} : result,
    }));
  });
})();
