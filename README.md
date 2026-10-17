# SWCH

A desktop game library and account manager for Windows and macOS. Organize games from multiple platforms, switch between your own accounts, and extend the app with optional mods.

[Download](https://github.com/Kabarab/swch/releases/latest) · [Русская инструкция](README.ru.md) · [Documentation](#documentation) · [Report an issue](https://github.com/Kabarab/swch/issues) · [Legal notice](docs/LEGAL.md)

> **Independent project:** SWCH is not affiliated with or endorsed by the platforms it integrates with. Integration support varies by platform and operating system. Read the [legal notice and conditions of use](docs/LEGAL.md) before connecting accounts or installing mods.

## Features

- **One game library:** browse installed games and available account libraries across supported platforms.
- **Account management:** save and switch between your own supported accounts and sessions. Platform authentication and ownership requirements still apply.
- **Game details:** view supported inventories, collections, statistics, and playtime. Available data varies by integration.
- **Mods:** add themes, tabs, actions, and launcher integrations through ZIP packages or the mod catalog.
- **Languages and updates:** select interface language packs and check for application updates from the settings screen.

English is the app's base and fallback language. By default, SWCH selects your system language on startup; if a translation is unavailable, it uses English. You can choose a different language in settings, and that choice is preserved.

## Platform integrations

| Platform | Main capabilities | Notes |
| --- | --- | --- |
| Steam | Library, account switching, inventory, account status, and supported game statistics | Requires the relevant account and Steam client/API access. |
| Epic Games | Account switching, library synchronization, and Fortnite collection data | Requires an authenticated Epic account; service changes can affect support. |
| Roblox | Account management, game launching, and inventory information | Use only accounts you are authorized to access; individual features vary by OS. |
| Minecraft | Local Java Edition accounts, profiles, servers, and worlds | Requires an appropriate game license and compatible installed launcher. |
| Riot Games | League of Legends and VALORANT collections, supported statistics, and saved sessions | Requires the relevant official client and account. |
| Ubisoft Connect | Installed-game discovery, official-client launching, and saved sessions | Windows integration; game ownership is checked by Ubisoft Connect. |
| GOG GALAXY | Account libraries, installed games, and account switching | GALAXY launching is supported on Windows. |
| HoYoverse / HoYoPlay | Installed-game discovery, launching, and saved account sessions | Availability depends on the installed client and OS; registry-based integration is Windows-specific. |

These are community-maintained integrations, not platform approvals. Some depend on interfaces that may change without notice. SWCH cannot guarantee compatibility or protection from platform enforcement.

## Download and installation

Get a build from [GitHub Releases](https://github.com/Kabarab/swch/releases/latest).

| System | Installer | Portable archive |
| --- | --- | --- |
| Windows x64 | `SWCH-Setup-<version>.exe` | `swch-windows-x86_64.zip` |
| macOS 13.5+ on Apple Silicon | `SWCH-Setup-<version>-arm64.pkg` | `swch-macos-arm64.zip` |

1. Download the package for your system and follow its installation instructions.
2. Start SWCH. The desktop interface and local backend start together.
3. Connect your own accounts and follow the sign-in flow for each platform.
4. Open the library to browse games. Install optional mods from the **Mods** page if needed.

For portable Windows builds, extract the ZIP and run `swch.exe`. For the macOS archive, move the app bundle to `/Applications`.

For installation details, signing limitations, and troubleshooting, see the [installation guide](docs/installation.md) (Russian). Packaged desktop builds target Windows x64 and macOS Apple Silicon; support for an individual integration may be narrower.

## Mods and account safety

Mods extend SWCH separately from its core library and account-management features. The giveaways/autoclaimer functionality belongs to a mod with its own behavior and platform requirements; it is not a guarantee that a store permits automated claiming.

Backend mods can access files, the network, and programs with your user's permissions. Install only mods you trust. See the [mod guide and SDK](docs/mods.md) for capabilities and lifecycle details.

Treat session files, cookies, tokens, local databases, and account exports as sensitive. Do not share them or attach them to public issues. Platform sign-in can expire or be revoked, requiring you to connect the account again. Before changing local session files or troubleshooting an account, close the relevant game and follow the platform's sign-in instructions.

## Documentation

Most detailed guides are currently in Russian.

- [Installation and updates](docs/installation.md)
- [Account sign-in and switching](README.ru.md#-вход-после-перезагрузки)
- [GOG setup](docs/gog.md)
- [Mods and SDK](docs/mods.md)
- [Language packs](docs/localization.md)
- [Game statistics](docs/game-statistics.md)
- [Playtime tracking](docs/playtime.md)
- [Giveaways/autoclaimer mod](docs/auto-claim.md)
- [Legal notice and conditions of use](docs/LEGAL.md)

## Support and contributions

Maintained by [Kabarab](https://github.com/Kabarab). Use [GitHub Issues](https://github.com/Kabarab/swch/issues) for bug reports and feature requests. Include the SWCH version, operating system, affected integration, and steps to reproduce the problem. Remove credentials and personal data from logs and screenshots before posting.

For a substantial contribution, open an issue to discuss the scope first. Keep changes focused and describe how they were verified. Never include real account credentials, cookies, tokens, or private session data in a contribution.

## License and third-party rights

A project-wide software license has not yet been specified in this repository. Public availability of the source does not by itself grant unrestricted rights to use, modify, or redistribute it. Third-party components retain their own licenses, and platform names, logos, games, and other content remain the property of their respective owners.

See the [legal notice](docs/LEGAL.md) for conditions of use, platform requirements, and the limits of responsibility.
