// Deliberately English. This is developer input, not a translatable UI label.
export const MOD_PROMPT = `Build a complete, working mod for SWCH using its existing Mod SDK v1. My idea: [DESCRIBE THE MOD HERE]. My personal GitHub username: [USERNAME]. Mod ID: [username.mod-name]. Choose theme, tab, launcher, or a combination to match the idea.

First read the authoritative sources (do not invent APIs):
- https://github.com/Kabarab/swch/blob/main/docs/mods.md
- https://github.com/Kabarab/swch/blob/main/mod-sdk/index.d.ts
- https://github.com/Kabarab/swch/tree/main/mod-sdk/templates
If you cannot access them, ask me to provide these files before implementing SDK-dependent behavior.

Deliver all runnable files, their paths, and exact setup/test/release commands. No Rust changes or SWCH rebuild should be necessary. Developer runtime is Node.js 24 LTS; SWCH bundles Node.js for users. Server entry point is an ES module exporting default async function activate(swch). SWCH supplies the SDK argument; no SDK npm install is needed for execution. Register tabs, actions and launchers during activation. Treat stdout as SWCH's JSON-RPC transport and use swch.log for logging.

Put manifest.json at the ZIP root. Include id, name, semver version, api: "^1.0.0", platforms: ["windows-x64", "macos-arm64"], description, and relative backend/ui/theme entry points as needed; at least one entry point is required. IDs are lowercase ASCII letters, digits, dots, hyphens and underscores, starting with a letter or digit, at most 80 characters. Do not ship .env, credentials, symlinks, native .node addons, or dependencies on other mods. Bundle ordinary JavaScript dependencies in advance; users must not run npm install. Respect package limits: 50 MiB ZIP, 200 MiB unpacked, 10,000 entries.

Available server SDK APIs: library.list(), accounts.list() (public account fields only); storage.get/set/delete(key, value); settings.get/set(value); secrets.get/set/delete(key, value); events.on(event, listener), events.emit(event, data); registerTab({id,name}); registerAction({id,name}, handler); registerLauncher({id,name,capabilities}, handlers). Library and account events are library.changed and accounts.changed. Keep mod data in SDK storage, settings and secrets; never access SWCH's database directly or hard-code another mod's IDs. Use new URL('./resource', import.meta.url) for packaged resources because the process working directory is the persistent mod data directory.

For a launcher, declare only supported capabilities: library, sync, accounts, connect, disconnect, switch, launch. Supply each declared handler. library/sync return Game[]; accounts returns Account[]; connect receives a parameter object; disconnect/switch receive {account_id}; launch receives {game_id, account_id?}. Game needs local id, name, is_installed and optional owner_ids with local account IDs; Account needs local id, username, is_active and optional display_name. Return at most one active account per launcher. SWCH assigns global platform/game/account namespaces; handlers use local IDs. Implement real functionality when feasible; label any demo or stub honestly. Handle cancellation, timeouts and errors, and never pretend a launch or login succeeded.

For a game-specific mod, declare optional manifest games: [{launcher:"hoyoverse",app_id:"genshin",name:"Genshin Impact"}] using the exact launcher/app_id pair from library.list(). Users can also assign installed mods to library games in Mods > Games and applications; bindings persist across updates. When opened from a game, window.swch.game and window.swch.account contain the selected game and public account (or null); both are null for general navigation. Game-scoped card actions receive {game,account} as input. Binding only adds navigation: implement inventory, characters or other game data using real authorized APIs in the mod itself.

For tabs, ui is a standalone HTML document in a sandboxed iframe. Inline/bundle its styles, scripts and resources; do not assume relative files are served. Use window.swch.call(method, params) and window.swch.on(event, listener). Examples: call('library.list'), call('action', {id:'local-action', input:{}}), call('settings.get'), call('settings.set', {value:{}}), call('storage.get', {key:'counter'}). No direct access to the main window DOM, internal endpoints, credentials or database. Keep user content as textContent, not unsafe HTML. Include accessible labels and responsive layouts that support long translations and RTL.

For themes, use only documented JSON tokens: bg, panel, panel-hover, line, text, muted, accent as #RRGGBB; radius from 0px to 32px. Main-window CSS injection is not supported. Mod code is trusted user-level code; process/iframe separation does not make it safe. Do not add hidden telemetry, credential collection or unrelated filesystem/network operations.

Create the project with:
node mod-sdk/cli.mjs init my-mod --template THEME_OR_TAB_OR_LAUNCHER --id username.mod-name
Validate and package with:
node mod-sdk/cli.mjs check my-mod
node mod-sdk/cli.mjs pack my-mod dist/mod.zip
Explain local testing: SWCH > Mods > ZIP installation and development > connect the absolute project folder, then Reload after edits. Test enable/disable/restart, persistence, failed operations, and every declared launcher capability on supported platforms; clearly state any OS you could not test.

Include the release workflow from mod-sdk/templates/release.yml. Publish in a public repository owned by my personal GitHub account (organizations are not supported). Set manifest version X.Y.Z, push tag vX.Y.Z, and produce a stable GitHub Release with the asset named mod.zip. As the repository owner, submit the "Publish a SWCH mod" issue form in Kabarab/swch with owner/repository. The automated catalog checks repository ownership and package data without executing the mod; later stable releases are checked every six hours. Never replace an already published version's ZIP; increase the version. Explain that catalog validation is not a security audit.

Finish with a short README covering installation, permissions, settings, local testing, publishing and updating. Provide the complete files and verify the package with the actual SWCH CLI before claiming success.`;
