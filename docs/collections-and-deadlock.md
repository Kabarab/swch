# Collections and Deadlock statistics

Inventory platform tabs use `inventoryPlatforms` in the shared platform metadata.
Only platforms with connected accounts and an implemented collection are shown.
If the selected platform loses its last account, selection moves to an available
platform. A late Roblox response cannot restore an obsolete inventory render.

Riot inventory opens on **Все предметы**, grouped into labeled category sections
with names sorted naturally within each section. Category selection separates skins,
buddies and agents/champions. Shared heroes are grouped by their product and item
identity; the card lists all owners in the selected account scope. Skin and buddy
ownership is still kept per account.

VALORANT weapon categories and exact-weapon selection use local skin/level UUID
metadata, including melee weapons whose skin names do not contain “Melee”. Old
snapshots can fall back to a known English weapon suffix; unrecognized skins are
retained in **Прочее оружие**. Filters never classify buddies or agents as weapons.
The metadata source is [Valorant-API weapons](https://valorant-api.com/v1/weapons?language=en-US),
captured on 2026-10-11 in `ui/valorant-weapons.mjs`. It contains identifiers only;
account storage and credential formats are unchanged.

Deadlock statistics add a per-account character selector. The existing overall
match totals remain available. Per-character totals use public hero statistics
filtered to normal Standard/Ranked games; bot, private and Brawl matches are
excluded. Standard wins are requested separately, and ranked participation is
confirmed from deduplicated match records for the current season and account.
Missing values stay unknown, and cached older responses remain deserializable.

Requirements come from the active public ranked-season asset, rather than fixed
thresholds. The current source reports 60 Standard wins and 3 heroes with 15
Standard wins each. The official [Valve matchmaking update](https://steamcommunity.com/games/1422450/announcements/detail/680756685198854911)
explains these requirements. Future season changes are read again on refresh.

Public history is incomplete for some players and does not supply Valve's
authoritative current permission flag. The interface distinguishes **Есть
рейтинговые матчи**, **Условия выполнены по статистике** and **Доступ не
подтверждён**. A low observed win count never proves the player is locked out.
Account restrictions, penalties and exact eligibility are decided by the game.
No game launches, native session switches, paid API calls or bot friend requests
are needed to view these statistics.

Primary sources: [Deadlock API schema](https://api.deadlock-api.com/openapi.json),
[hero assets](https://api.deadlock-api.com/v1/assets/heroes),
[season assets](https://api.deadlock-api.com/v1/assets/ranked-seasons),
[rank assets](https://api.deadlock-api.com/v1/assets/ranks),
[Valve protocol definitions](https://github.com/SteamDatabase/Protobufs/blob/master/deadlock/citadel_gcmessages_common.proto).

Tests cover account additions/removals and delayed responses, mixed Riot
collections, shared heroes, weapon UUIDs and filters, partial sections, Deadlock
account and mode isolation, current/expired seasons, character selection and
five window widths. Browser screenshots are in `output/riot-inventory` and
`output/deadlock-stats`.
