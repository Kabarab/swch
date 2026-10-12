# SWCH interface maintenance

The canonical interface is the root `index.html` and `ui/` directory. Installed
copies (including Desktop and `output/windows`) are build artifacts, never an
editing source. Release packaging copies the canonical directory after the
`ui-regressions` job succeeds.

## Restored design

On 2026-10-11, compact accounts, CS2/Dota inventory, Riot collections and the
Ubisoft/GOG handlers were recovered selectively from
`output/auto-claim-desktop-backup-20261008-184134`. The following snapshot,
`auto-claim-desktop-backup-20261008-184459`, lacks those features. This establishes
which snapshots contain the regression; it does not identify the action that
overwrote the files. Later Minecraft/HoYoverse changes, Steam launch behavior and
the long giveaway authentication timeout were retained.

`ui/platforms.mjs` holds the eight platform names, local original logos and
navigation rules. Library navigation shows platforms with games; Accounts also
shows platforms with saved accounts and no games. `ui/ux.js` handles progressive
disclosure, and `ui/ux.css` applies common density and focus styling. Logo sources
are documented in `ui/platforms/SOURCES.md`.

## Verify and install

```powershell
Set-Location tests/ui
npm ci
npx playwright install chromium
$env:PLAYWRIGHT_CHANNEL = 'chromium'
npm test
Set-Location ../..
./scripts/update-ui.ps1 -InstalledDirectory 'C:\Users\NAZAR\Desktop' -RestartServer
```

`update-ui.ps1` backs up the installed UI to `output/ui-backup-*`, copies only
interface assets, and verifies every copied file by SHA-256. An optional restart
uses the existing installed server binary and refuses to interrupt an active
giveaway run. It restores the previous interface on failure. No account storage
or credentials are copied.

When changing interface imports, update their version suffix alongside the
entrypoint assets so an existing WebView loads the new modules. Run the UI
regressions before updating the installed copy; the screenshots in
`output/ux-redesign` show the directory, library and inventory at desktop/mobile
sizes.
