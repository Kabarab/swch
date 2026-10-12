import {MOD_PROMPT} from './mod-prompt.mjs';
export function modGuideHTML() {
  return `<details id="mod-author-guide" class="ux-disclosure mod-author-guide">
    <summary>Как создать и опубликовать мод</summary>
    <p>SWCH поддерживает темы, дополнительные вкладки и новые лаунчеры на JavaScript. Пересобирать приложение не нужно.</p>
    <ol>
      <li><strong>Подготовь инструменты.</strong> Установи Node.js 24 LTS для разработки и скачай папку <code>mod-sdk</code> из репозитория SWCH. Пользователям Node.js устанавливать не нужно.</li>
      <li><strong>Создай проект.</strong> Выбери шаблон: <code>theme</code> — оформление, <code>tab</code> — вкладка, <code>launcher</code> — платформа с играми и аккаунтами. Замени пример ID на свой уникальный ID.</li>
    </ol>
    <pre translate="no" dir="ltr">node mod-sdk/cli.mjs init my-mod --template tab --id username.my-mod
node mod-sdk/cli.mjs check my-mod
node mod-sdk/cli.mjs pack my-mod dist/mod.zip</pre>
    <ol start="3">
      <li><strong>Проверь в SWCH.</strong> Открой «Установка из ZIP и разработка», укажи полный путь к проекту и нажми «Подключить папку». После правок нажимай «Перезагрузить». Проверь работу и сохранение настроек после перезапуска.</li>
      <li><strong>Выпусти ZIP.</strong> Создай публичный репозиторий в личном аккаунте GitHub и загрузи проект вместе с созданным шаблоном GitHub Actions. Укажи версию в <code>manifest.json</code> и отправь тег <code>vX.Y.Z</code>. Workflow выпустит стабильный релиз с файлом <code>mod.zip</code>.</li>
      <li><strong>Добавь в каталог.</strong> От имени владельца репозитория открой заявку по ссылке ниже и укажи <code>owner/repository</code>. Автоматическая проверка добавит пакет в каталог; новые стабильные релизы проверяются каждые шесть часов.</li>
    </ol>
    <p>Для обновления увеличивай версию и выпускай новый релиз. Не заменяй ZIP уже опубликованной версии. Организации GitHub, зависимости от других модов и нативные Node.js-дополнения не поддерживаются.</p>
    <p>Автоматическая проверка подтверждает структуру пакета и владельца, но не безопасность кода. Мод работает с правами пользователя; устанавливай только моды, авторам которых доверяешь.</p>
    <div class="mod-guide-links">
      <a href="https://github.com/Kabarab/swch/tree/main/mod-sdk" target="_blank" rel="noopener noreferrer">SDK и шаблоны ↗</a>
      <a href="https://github.com/Kabarab/swch/blob/main/docs/mods.md" target="_blank" rel="noopener noreferrer">Полная документация ↗</a>
      <a href="https://github.com/Kabarab/swch/issues/new?template=publish-mod.yml" target="_blank" rel="noopener noreferrer">Отправить мод в каталог ↗</a>
    </div>
    <h3>Создать мод с помощью ИИ</h3>
    <p>Скопируй английский промпт, замени описание идеи, имя GitHub и ID мода, затем отправь его ИИ. Проверь созданный код и ZIP перед установкой и публикацией.</p>
    <button type="button" class="secondary" id="mod-copy-prompt">Скопировать промпт на английском</button>
    <p id="mod-prompt-status" role="status" aria-live="polite"></p>
    <label>Промпт на английском<textarea id="mod-ai-prompt" translate="no" dir="ltr" lang="en" readonly rows="10" spellcheck="false"></textarea></label>
  </details>`;
}
export function setupModGuide(view) {
  const field=view.querySelector('#mod-ai-prompt');if(!field)return;
  field.value=MOD_PROMPT;
  view.querySelector('#mod-copy-prompt').addEventListener('click',async()=>{
    const status=view.querySelector('#mod-prompt-status');
    try {await navigator.clipboard.writeText(MOD_PROMPT);status.textContent='Промпт скопирован';}
    catch {field.focus();field.select();status.textContent='Текст выделен. Скопируй его сочетанием Ctrl+C или ⌘C.';}
  });
}
