import {MOD_PROMPT} from './mod-prompt.mjs';
import {t} from './i18n.js';
export function modGuideHTML() {
  return `<section class="mod-prompt-card" aria-labelledby="mod-prompt-heading">
    <div class="mod-prompt-heading">
      <h3 id="mod-prompt-heading">Создать мод с помощью ИИ</h3>
      <button type="button" class="primary" id="mod-copy-prompt"><svg aria-hidden="true" viewBox="0 0 24 24"><rect x="8" y="8" width="12" height="13" rx="2"></rect><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3"></path></svg><span>Скопировать промпт на английском</span></button>
    </div>
    <p>Скопируй английский промпт, замени описание идеи, имя GitHub и ID мода, затем отправь его ИИ. Проверь созданный код и ZIP перед установкой и публикацией.</p>
    <p id="mod-prompt-status" role="status" aria-live="polite"></p>
    <details id="mod-prompt-preview" class="ux-disclosure">
      <summary>Промпт на английском</summary>
      <label>Промпт на английском<textarea id="mod-ai-prompt" translate="no" dir="ltr" lang="en" readonly rows="10" spellcheck="false"></textarea></label>
    </details>
  </section><details id="mod-author-guide" class="ux-disclosure mod-author-guide">
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
  </details>`;
}
export function setupModGuide(view) {
  const field=view.querySelector('#mod-ai-prompt');if(!field)return;
  field.value=MOD_PROMPT;
  field.addEventListener('click',()=>field.select());
  const button=view.querySelector('#mod-copy-prompt');
  button.addEventListener('click',async()=>{
    const status=view.querySelector('#mod-prompt-status');
    button.disabled=true;
    try {await navigator.clipboard.writeText(MOD_PROMPT);status.textContent=t('Промпт скопирован');}
    catch {
      const preview=view.querySelector('#mod-prompt-preview'),wasOpen=preview.open;
      preview.open=true;field.focus();field.select();
      let copied=false;
      try {copied=document.execCommand('copy');} catch {}
      if(copied){preview.open=wasOpen;button.focus();status.textContent=t('Промпт скопирован');}
      else status.textContent=t('Текст выделен. Скопируй его сочетанием Ctrl+C или ⌘C.');
    }
    finally {button.disabled=false;}
  });
}
