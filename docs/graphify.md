# Graphify для SWCH

Установлен официальный пакет [graphifyy](https://github.com/Graphify-Labs/graphify)
версии **0.9.72**, Python **3.12.13**. Окружение находится в
`.graphify-tools/venv`, отдельный Python — в `.graphify-tools/python`.
Зависимости приложения Rust не изменяются.

## Использование

Из корня проекта в PowerShell:

```powershell
# Построить или обновить граф кода, включая Cargo workspace
.\scripts\graphify.ps1 extract . --code-only --cargo --max-workers 4
# Создать отчёт и HTML без вызовов LLM
.\scripts\graphify.ps1 cluster-only . --no-label
# Найти связи или объяснить символ
.\scripts\graphify.ps1 query "steam session accounts" --budget 1500
.\scripts\graphify.ps1 explain "SteamSessionManager"
# Открыть интерактивный граф
Start-Process .\graphify-out\graph.html
```

Результаты: `graphify-out/graph.json`, `GRAPH_REPORT.md`, `graph.html`.
Окружение и результаты исключены из Git. `.graphifyignore` исключает сборки,
сторонний WebView2 SDK, локальные БД, ключи и служебные файлы агентов.
Документы и изображения не включены: настройка индексирует код локально,
без API-ключей и платных вызовов моделей. HTML может загружать библиотеки с CDN.

Навык Codex находится в `.agents/skills/graphify/`, инструкции — в `AGENTS.md`.
В новом чате проекта можно вызвать `$graphify`. Команды из официального навыка
адаптируются к PowerShell и локальному интерпретатору согласно `AGENTS.md`.
Git hooks и фоновый процесс не устанавливались; граф обновляется командами выше.

## Повторная установка окружения

Нужен `uv`. Из корня проекта:

```powershell
$env:UV_CACHE_DIR = Join-Path $PWD '.graphify-tools\cache'
$env:UV_PYTHON_INSTALL_DIR = Join-Path $PWD '.graphify-tools\python'
uv venv .graphify-tools/venv --python 3.12.13 --managed-python
uv pip install --python .graphify-tools/venv/Scripts/python.exe graphifyy==0.9.72
```

Навык и его восемь файлов `references/` скопированы из установленного
официального пакета: штатная установка навыка в этой среде Windows завершилась
ошибкой `WinError 5` при переименовании `references.tmp`. Содержимое пакета
не изменялось. Глобальная конфигурация Codex не менялась.

## Ограничения

Graphify предупреждает о совпадающих идентификаторах импортов Cocoa и WebKit
в двух Swift-файлах. Для каждого такого импорта сохраняется один узел;
при анализе macOS-кода сверяйте импорты с исходниками.
Имена сообществ оставлены автоматическими (`Community N`).
