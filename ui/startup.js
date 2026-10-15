export function initStartup({toast, demo}) {
  const toggle = document.getElementById('startup-toggle');
  const modes = document.getElementById('startup-modes');
  const choices = modes?.querySelectorAll('input[name="startup-mode"]');
  const note = document.getElementById('startup-note');
  const bridge = window.chrome?.webview;
  if (!toggle || !modes || !note) return () => {};
  if (demo || !bridge?.postMessage || !bridge?.addEventListener) {
    note.textContent = demo ? 'Автозапуск недоступен в деморежиме.' : 'Доступно в приложении SWCH для Windows.';
    return () => {};
  }

  let mode = 'off', lastEnabledMode = 'open', pending = null, timer, lastFailure = null;
  const render = (displayMode = mode, disabled = false) => {
    toggle.checked = displayMode !== 'off';
    toggle.disabled = disabled;
    modes.hidden = displayMode === 'off';
    modes.disabled = disabled || modes.hidden;
    for (const choice of choices) choice.checked = choice.value === (displayMode === 'off' ? lastEnabledMode : displayMode);
  };
  const description = () => mode === 'off'
    ? 'SWCH не запускается автоматически. Выбор сохраняется сразу.'
    : `После входа в Windows SWCH запустится ${t(mode === 'minimized' ? 'свёрнутым на панель задач' : 'открытым окном')}. Выбор сохраняется сразу.`;
  const fail = message => {
    lastFailure = typeof message === 'function' ? message : () => message;
    const text = lastFailure();
    const wasSaving = pending === 'save';
    clearTimeout(timer);
    pending = null;
    // After a failed read, allow reopening settings to retry, but don't guess the state.
    render(mode, !wasSaving);
    note.textContent = text;
    if (wasSaving) toast(text);
  };
  const request = (kind, message) => {
    if (pending) return;
    pending = kind;
    render(kind === 'save' ? message.slice('startup:set:'.length) : mode, true);
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
    if (mode !== 'off') lastEnabledMode = mode;
    render();
    note.textContent = description();
    if (wasSaving) toast(mode === 'off' ? 'Автозапуск выключен' : 'Автозапуск сохранён');
  });
  toggle.addEventListener('change', () => {
    request('save', `startup:set:${toggle.checked ? lastEnabledMode : 'off'}`);
  });
  modes.addEventListener('change', event => {
    if (event.target.matches('input[name="startup-mode"]') && ['open', 'minimized'].includes(event.target.value)) {
      request('save', `startup:set:${event.target.value}`);
    }
  });
  document.addEventListener('swch-language-change', () => {
    note.textContent = pending ? (pending === 'save' ? 'Сохраняем автозапуск…' : 'Проверяем автозапуск…') : lastFailure ? lastFailure() : description();
  });
  return () => request('read', 'startup:get');
}
import {t} from './i18n.js';
