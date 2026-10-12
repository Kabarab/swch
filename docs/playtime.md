# Game time and Riot names

The library, lists and game dialogs use `ui/playtime.mjs` to distinguish unknown
time from a confirmed zero and isolate the selected account. Imported platform
totals take precedence over manual entries. Duplicate snapshots are not added.

Sources available without switching native launcher sessions:

- Steam: existing owned-game and local history records (minutes).
- Riot: saved profile match durations, explicitly labeled as available match
  history rather than lifetime time. Use **Обновить профиль** to load new history.
- Epic: authenticated read-only library playtime API (`totalTime` in seconds),
  cached for ten minutes. **Обновить** bypasses the cache. A failed request keeps
  saved records and marks them as cached.
- Minecraft Java: per-player statistics in existing local worlds (20 ticks per
  second). Deleted worlds and remote servers are outside this total. Duplicate
  roots and UUID aliases do not add the same statistics twice.
- GOG GALAXY: `GameTimes.minutesInGame` in its local database, opened read-only.
  GOG, Epic and Ubisoft entries are shown when their platform/game identifiers
  match. This local total is labeled GALAXY and is not assigned to another SWCH
  account. Native Epic totals take precedence.

Libraries may also supply explicit `playtime_seconds` or `playtime_minutes`.
There is no assumed automatic total for Roblox or HoYoverse; absent source data
is left unknown. Existing manually entered values remain a labeled fallback.

Primary implementation references:

- [Playnite Epic client](https://github.com/JosefNemec/PlayniteExtensions/blob/master/source/Libraries/EpicLibrary/Services/EpicAccountClient.cs)
- [Epic playtime response](https://github.com/JosefNemec/PlayniteExtensions/blob/master/source/Libraries/EpicLibrary/Models/PlaytimesResponse.cs)
- [GALAXY database export implementation](https://github.com/AB1908/GOG-Galaxy-Export-Script/blob/master/galaxy_library_export.py)

For Riot names, `/api/riot/accounts` chooses the latest valid Riot ID across saved
game profiles, rather than the original connection/session label. If Riot Client
is already running, a bounded read obtains its active alias; identity is checked
before and after the read. It never starts or switches the client. The public
name is saved for subsequent offline reads without changing session flags,
credentials, game data or statistics timestamps. Closed clients retain their
last confirmed name until a refresh or profile synchronization succeeds.

Regression checks: `tests/ui/playtime.test.mjs`,
`tests/ui/playtime-browser.cjs`, `tests/playtime_tests.rs` and
`tests/riot_profiles_tests.rs`. Browser checks cover cards, list, modal, selected
profiles, renamed accounts, refresh, zero/unknown values and five window widths.
