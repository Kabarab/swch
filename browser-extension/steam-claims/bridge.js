if (location.pathname === '/steam/claim-connect') {
  window.addEventListener('message', event => {
    if (event.source !== window || event.origin !== location.origin || event.data?.type !== 'SWCH_STEAM_CONNECT_REQUEST') return;
    chrome.runtime.sendMessage({type: 'SWCH_STEAM_CONNECT'}, response => {
      const error = chrome.runtime.lastError?.message;
      window.postMessage({type: 'SWCH_STEAM_CONNECT_RESULT', ok: !error && response?.ok === true,
        error: error || response?.error}, location.origin);
    });
  });
  window.postMessage({type: 'SWCH_STEAM_EXTENSION_READY'}, location.origin);
}
