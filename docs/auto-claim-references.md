# Что взято из Free Games Claimer и Remaster

Анализ зафиксирован на 8 октября 2026 года. Ниже разделены исходные возможности,
реализованные адаптации SWCH и оставшаяся работа. Наличие адаптера не означает
проверенного получения новой лицензии на реальном аккаунте.

## Источники

| Проект | Изученный commit | Основная логика |
| --- | --- | --- |
| [vogler/free-games-claimer][v-repo] | `99c1f05302aeece21a628797cfdffb561ee38956` | Node.js / Playwright, Epic, Prime, GOG, сохранение браузерного входа и результатов. |
| [Free-Games-Claimer-Remaster][r-repo] | `89f510ef4dcf809bd5410997aa8fe1a7f14574f3` | Python, SQLite, расписание, больше магазинов, GamerPower, новые страницы оформления Epic. |

Оба проекта указывают AGPL-3.0: [vogler][v-license], [Remaster][r-license].
SWCH использует независимо написанные адаптеры и собственное хранилище.
При включении исходных фрагментов чужого кода нужны сохранённые сведения
о происхождении и лицензии.

## Платформы

| Платформа | Что полезно в источнике | Что сделано в SWCH / что остаётся |
| --- | --- | --- |
| Steam | [Remaster][r-steam]: SteamDB free-to-keep, GamerPower, бесплатная лицензия. У [vogler][v-steam] Steam-модуль собирает статистику, а не игры. | Сохранён собственный автоматический вход из официального клиента, полный поиск Store и проверка бесплатной лицензии. Добавлены кандидаты GamerPower с повторной проверкой Store. SteamDB подключён как дополнительный источник с независимой повторной проверкой Store. |
| Epic PC | [vogler][v-epic]: Free Now → Get → checkout; [Remaster][r-epic]: современный Get → Add to library и прежний checkout iframe. | Официальный SSO из добавленного аккаунта, API и браузерный fallback, проверка одного заказа с итогом 0 и entitlements. Новый Add to library проверен на тестовых страницах. Реальный браузер пока останавливается на security check. |
| Epic Android / iOS | [Remaster][r-mobile]: мобильная discover API; оформление через Epic product page. Это detection, не самостоятельное доказательство лицензии. | Дополнительный discovery с проверкой исходной цены, срока и отдельного SKU. Неполные данные пропускаются с предупреждением. Реальное получение мобильной лицензии не проверено. |
| Prime / Amazon | [vogler][v-prime], [Remaster][r-prime]: собственные игры, linked rewards и внешние ключи. В изученном Remaster каталог находится на `luna.amazon.com/claims/home`. | Подключение Amazon и общий браузерный адаптер, очередь ключей. Нужна уже действующая Prime. Добавлен собственный обход полного каталога Luna с прокруткой, датами предложения и проверкой обычной платной цены. Каталог разделён по аккаунтам. Новое связывание стороннего игрового аккаунта требует участия пользователя. Это каталог раздач, не поддержка облачного запуска Luna. |
| GOG | [vogler][v-gog], [Remaster][r-gog]: giveaway banner → `/giveaway/claim`, активация кодов Prime. | Кандидаты GamerPower, сверка текущего banner и продукта, бесплатный endpoint, подтверждение владения через собственный GOG API. Отдельная web-сессия обязательна. Активация кода на явно выбранном GOG аккаунте добавлена; реальные giveaway / redemption ещё не проверены. |
| Microsoft / Xbox | [Remaster][r-microsoft]: GamerPower, StoreEdge, Get, отдельное purchase window; погашение кодов. | Подключение и общий браузерный адаптер кандидатов. Добавлены StoreEdge с проверкой продукта/обычной и текущей цены и отдельный Microsoft redeem FSM. До подтверждения проверяются аккаунт и полная игра; после — владение. Неподтверждённые формы останавливаются. Minecraft профиль не используется как доказательство входа. |
| Ubisoft | [Remaster][r-ubisoft]: freeevents, register.ubisoft.com, account confirmation; отдельно исключаются trial. | Подключение и браузерный адаптер кандидатов GamerPower. Добавлены собственный freeevents, конечные сроки, отбрасывание trials и регистрационная страница PC с подтверждением аккаунта. Непрозрачный launcher snapshot по-прежнему не является Store SSO. |
| Fanatical | [Remaster GamerPower][r-gamerpower]: Claim game → orders → reveal Steam key → отложенная Steam activation. Автор также не подтверждает end-to-end реальной раздачи. | Кандидаты, отдельное подключение, браузерная выдача ключа и явное назначение Steam аккаунту. Добавлены собственные orders/redeem вызовы, точный выбор одного товара и сохранение только нужного магазинного browser storage в хранилище ОС. Реальная проверка выдачи ключа остаётся. |
| Itch.io | [Remaster][r-gamerpower]: claim и проверка ownership; возможен download-only. | Подключение, кандидаты и браузерный адаптер. Download-only не считается лицензией; незнакомые страницы останавливают оформление. Реальный claim не проверен. |
| IndieGala | [Remaster][r-gamerpower]: Add to library → reload → ownership, CAPTCHA выполняет человек. | Подключение, кандидаты и браузерный адаптер. Реальный claim не проверен. |
| Alienware Arena | [Remaster][r-gamerpower]: обнаружение и уведомление, ручное получение. | Кандидаты и ссылка, автоматическое получение не обещается. |

GamerPower — источник кандидатов, а не магазин. SWCH требует type Game, обычную
цену выше нуля и действующий конечный срок, исключая early access, DLC, beta,
demo, trial, F2P и weekend. Затем проверяет URL, магазин и условия на его странице.
HTTP-переадресация кандидата может привести в Fanatical/Alienware вместо указанного
Steam. Добавлены literal meta/JavaScript redirects без исполнения чужого кода. Неоднозначные/вычисляемые перенаправления и неизвестный магазин пропускаются.
Сбой дополнительного источника не отменяет уже найденные native предложения.

## Общие функции

| Возможность источников | Адаптация в SWCH |
| --- | --- |
| [Расписание Remaster][r-main] | Интервал, ежедневное время, часовой пояс, оба режима, старт, реакция на новые аккаунты, единая блокировка. Учтены переходы летнего времени и сохранённые уже выполненные daily slots. |
| [Dry run][v-config] / [Remaster config][r-config] | Поиск без checkout, активации ключей и записи подтверждённой истории. |
| Ограниченные повторы | Network, 429 и 5xx, настройка попыток/паузы. CAPTCHA и 403 не повторяются агрессивно. |
| [История Remaster][r-db] / [vogler util][v-util] | Store + stable account ID + offer ID, реальная транзакционная дедупликация. Старое владение, новый claim и ключ различаются. |
| Browser state machine | Ожидаемый аккаунт, точное название/продукт, paid → free, конечный срок, Get → подтверждение → ownership. Проверяются trusted iframe и собственные popup. Неизвестный/ненулевой итог и лишняя игра в заказе блокируют действие. |
| Ключи и target mappings | Секрет в хранилище ОС, метаданные отдельно. Пользователь выбирает один Steam/GOG/Microsoft аккаунт. После ошибки нельзя молча переназначить одноразовый ключ другому аккаунту. Получение ключа не записывается как Steam/GOG лицензия. |
| [Уведомления][r-notifier] | Локальные уведомления требуют открытого UI. Добавлены Discord, Telegram, HTTPS webhook и email с TLS/STARTTLS SMTP. Внешние каналы по умолчанию выключены, секреты в OS vault, повтор одинакового результата подавляется, dry run ничего не отправляет. |
| Диагностика | Очищенные статусы, предупреждения источников, экспорт результатов без ключей и cookies. Автоматические screenshots страниц входа не сохраняются. |
| [Обновления Remaster][r-updates] | Отдельный runtime Remaster не устанавливается; SWCH использует собственное обновление. |

Обсуждение [современного Epic flow][v-epic-issue] не является уже влитой
реализацией основного `main` vogler. Статусы reference тоже не всегда достаточны:
Steam может считать отсутствие ошибки успехом, GOG — пустой JSON, Prime —
экспериментальный `redeemed?`. SWCH требует подтверждение ожидаемой лицензии.

## Автоматический вход и разделение аккаунтов

Steam ConnectCache → проверенное отдельное Valve CM соединение → Store session.
Epic DeviceAuth/refresh → официальный web exchange → redirect/set-sid → cookies,
проверенные по Epic account API. Обе связи реализованы. Вживую подтверждены
семь Steam входов и Epic SSO; после обновления все сохранённые Steam входы действуют.

GOG OAuth подтверждает библиотеку, но не создаёт магазинные cookies. Ubisoft
snapshot не раскрывает проверяемый Store SSO. Amazon, Microsoft, Fanatical,
Itch.io и IndieGala требуют первого официального подключения своего аккаунта.
После него SWCH восстанавливает его собственную web-сессию из хранилища ОС.
Новые чужие аккаунты из Steam/Epic профиля автоматически создать нельзя.

Браузер принадлежит SWCH, запускается отдельно и использует только cookies
выбранного магазина/аккаунта. Основной Chrome профиль пользователя не используется.
Пароли, токены и ключи не хранятся в SQLite, JSON экспорта, localStorage или логах.
Отключение и удаление аккаунта согласованы с блокировкой выполняемой операции.

## Что не переносится

- F2P, demo/weekend/DLC, подписки и скачивание без лицензии — вне исходного запроса.
- Stealth/fingerprint spoofing, искусственные age cookies, CAPTCHA solving,
  отключение 2FA, расход recovery codes и вымышленные billing данные.
- Один пароль `.env` на магазин, общий браузерный профиль и глобальный dedup названий.
- Неограниченные клики Place order и успех по одному исчезновению кнопки.
- [Fab][r-fab] и [Unity][r-unity] — ассеты; [AliExpress][r-aliexpress] — монеты.
  Они не создают игровые лицензии и не включены в клеймер игр.
- EA, Battle.net, Riot, Roblox и Minecraft не имеют игровых claim-модулей
  в изученных репозиториях.

## Проверка и оставшаяся работа

Автоматические тесты проверяют фильтры, расписание, идентичность, отсутствие
платного checkout, блокировки, ключи, историю и интерфейс. Они не заменяют
новое получение в реальном магазине. Реальный Epic браузер сейчас показывает
security check; её нужно пройти человеку через «Проверить вход в окне».

Ранее отсутствовавшие Prime/Ubisoft discovery, Microsoft StoreEdge/redeem,
Fanatical orders/reveal, SteamDB и внешние уведомления реализованы на Rust.
Тесты включают строгие parser/identity/price/key контракты и браузерные страницы.
Для проверки на реальных раздачах требуются подключённые магазинные аккаунты:
GOG и Ubisoft на устройстве ещё не подключены, Prime/Microsoft/Fanatical не добавлены.
Неизвестные данные, ограничения и проверки магазинов продолжают требовать действия
пользователя; абсолютная безотказность не заявляется. Новые linked-account связи
Prime автоматически не создаются без выбора и подтверждения нужного получателя.

Пользовательская проверка и ограничения: [auto-claim.md](auto-claim.md).
Основные модули: `src/auto_claim/{mod,providers,catalog,platforms,web,web_rules,
browser_claim,gog_claim,store_discovery,fanatical,microsoft,notifications,keys}.rs`, `src/server/auto_claim.rs`, `mods/autoclaimer/ui.mjs`.

[v-repo]: https://github.com/vogler/free-games-claimer/tree/99c1f05302aeece21a628797cfdffb561ee38956
[r-repo]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/tree/89f510ef4dcf809bd5410997aa8fe1a7f14574f3
[v-license]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/LICENSE
[r-license]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/LICENSE
[v-package]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/package.json
[v-readme]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/README.md#L126-L135
[v-config]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/src/config.js
[v-util]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/src/util.js
[v-steam]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/steam-games.js
[v-epic]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/epic-games.js
[v-epic-issue]: https://github.com/vogler/free-games-claimer/issues/590#issuecomment-4569361974
[v-prime]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/prime-gaming.js
[v-gog]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/gog.js
[v-unreal]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/unrealengine.js
[v-aliexpress]: https://github.com/vogler/free-games-claimer/blob/99c1f05302aeece21a628797cfdffb561ee38956/aliexpress.js
[r-main]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/main.py
[r-config]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/core/config.py
[r-db]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/core/database.py
[r-claimer]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/core/claimer.py
[r-run-state]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/core/run_state.py
[r-notifier]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/core/notifier.py
[r-updates]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/core/updates.py
[r-steam]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/steam.py
[r-epic]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/epic.py
[r-mobile]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/epic_mobile.py
[r-prime]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/prime.py
[r-gog]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/gog.py
[r-microsoft]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/microsoft.py
[r-ubisoft]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/ubisoft.py
[r-gamerpower]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/gamerpower.py
[r-fab]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/epic_fab.py
[r-unity]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/unity.py
[r-aliexpress]: https://github.com/P-Adamiec/Free-Games-Claimer-Remaster/blob/89f510ef4dcf809bd5410997aa8fe1a7f14574f3/src/stores/aliexpress.py
