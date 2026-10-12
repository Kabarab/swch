export function initStartup({toast, demo}) {
  const select = document.getElementById('startup-mode');
  const note = document.getElementById('startup-note');
  const bridge = window.chrome?.webview;
  if (!select || !note) return () => {};
  if (demo || !bridge?.postMessage || !bridge?.addEventListener) {
    note.textContent = demo ? 'Автозапуск недоступен в деморежиме.' : 'Доступно в приложении SWCH для Windows.';
    return () => {};
  }

  let mode = 'off', pending = null, timer, lastFailure = null;
  const description = () => mode === 'off'
    ? 'SWCH не запускается автоматически. Выбор сохраняется сразу.'
    : `После входа в Windows SWCH запустится ${t(mode === 'minimized' ? 'свёрнутым на панель задач' : 'открытым окном')}. Выбор сохраняется сразу.`;
  const fail = message => {
    lastFailure = typeof message === 'function' ? message : () => message;
    const text = lastFailure();
    const wasSaving = pending === 'save';
    clearTimeout(timer);
    pending = null;
    select.value = mode;
    // After a failed read, allow reopening settings to retry, but don't guess the state.
    select.disabled = !wasSaving;
    note.textContent = text;
    if (wasSaving) toast(text);
  };
  const request = (kind, message) => {
    if (pending) return;
    pending = kind;
    select.disabled = true;
    note.textContent = kind === 'save' ? 'Сохраняем автозапуск…' : 'Проверяем автозапуск…';
    timer = setTimeout(() => fail('Не удалось получить ответ приложения. Открой настройки ещё раз.'), 5000);
    try { bridge.postMessage(message); }
    catch { fail('Не удалось связаться с приложением. Открой настройки ещё раз.'); }
  };
  bridge.addEventListener('message', event => {
    const data = event.data;
    if (data?.type !== 'swch-startup' || !pending) return;
    if (data.errorCode || !['off', 'open', 'minimized'].includes(data.mode)) {
      const operation = pending === 'save' ? 'сохранить' : 'прочитать';
      fail(() => `Не удалось ${t(operation)} автозапуск${data.errorCode ? t(` (код Windows: ${data.errorCode})`) : ''}.`);
      return;
    }
    const wasSaving = pending === 'save';
    clearTimeout(timer);
    pending = null;
    lastFailure = null;
    mode = data.mode;
    select.value = mode;
    select.disabled = false;
    note.textContent = description();
    if (wasSaving) toast(mode === 'off' ? 'Автозапуск выключен' : 'Автозапуск сохранён');
  });
  select.addEventListener('change', () => {
    if (['off', 'open', 'minimized'].includes(select.value)) request('save', `startup:set:${select.value}`);
  });
  document.addEventListener('swch-language-change', () => {
    note.textContent = pending ? (pending === 'save' ? 'Сохраняем автозапуск…' : 'Проверяем автозапуск…') : lastFailure ? lastFailure() : description();
  });
  return () => request('read', 'startup:get');
}
import {t} from './i18n.js';
