'use strict';
(() => {
  if (window !== window.top || location.pathname !== '/id/api/redirect'
      || new URL(location.href).searchParams.get('clientId') !== '34a02cf8f4414e29b15921876da36f9a') return;
  const ext = globalThis.browser || chrome;
  let sent = false;
  function readResult() {
    if (sent) return;
    // Read only the JSON result, never sign-in forms, cookies or saved passwords.
    const text = document.querySelector('pre')?.textContent || document.body?.textContent || '';
    if (text.length > 65536) return;
    let result;
    try { result = JSON.parse(text); } catch { return; }
    const code = result?.authorizationCode;
    if (typeof code !== 'string' || !/^[a-f0-9]{32}$/i.test(code)) return;
    sent = true;
    observer.disconnect();
    ext.runtime.sendMessage({type: 'swch-epic-complete', code});
  }
  const observer = new MutationObserver(readResult);
  observer.observe(document.documentElement, {childList: true, subtree: true, characterData: true});
  readResult();
  setTimeout(() => observer.disconnect(), 600000);
})();
