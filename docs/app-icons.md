# SWCH application icons

The selected logo is concept 03: two bold orange interlocking forms with a
diagonal transparent seam. The same approved artwork is used for Windows and
macOS at every size; the broad shapes do not need a separate taskbar variant.

- Approved master: `src/assets/swch-logo.png`.
- Windows resource: `src/assets/swch.ico`, embedded by `src/windows_app.rc`.
- macOS resource: `swch.app/Contents/Resources/iconfile.icns`, selected by `CFBundleIconFile`.
- Master SHA-256: `c44a4c2d8e3c0f53ceba9ade58c78c0885af61b34f3125e6e667fcce353416a0`.

The master is the exact PNG chosen by the user, with its transparency intact.
Run `./scripts/build-app-icons.ps1` on Windows to package ICO frames at
16/20/24/32/40/48/64/128/256px and ICNS frames at 16–1024px, including Retina.
The script only resamples and packages the approved image; it does not redraw
or restyle it. Inspect actual 16/20/24/32px frames when replacing the master.

The previous S-and-arrows PNGs remain as legacy artwork; they are no longer
inputs to the icon build.

Windows packages also ship the ICO as `swch-logo-link.ico`. Installer shortcuts
and the Installed Apps entry refer to that file explicitly, rather than an
icon cached under the unchanged `swch.exe` path. Rename the standalone file
when changing the logo design, and update packaging, installer and smoke tests
together. The native window still uses its embedded ICO resource. Its window
property store explicitly supplies the same AppUserModelID, relaunch command,
display-name resource and standalone taskbar icon before the window is shown;
these properties are cleared on destruction. A portable executable without
the sidecar falls back to embedded icon 101.

Windows documents the separate group icon in
[RelaunchIconResource](https://learn.microsoft.com/en-us/windows/win32/properties/props-system-appusermodel-relaunchiconresource)
and property lifetime in
[SHGetPropertyStoreForWindow](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shgetpropertystoreforwindow).

## Selected concept prompt

Created with the built-in Imagegen tool, without image references:

Use case: logo-brand. Design one original logo concept for SWCH, a vibrant gaming launcher linking several game platforms and switching accounts. Direction 03: a distinctive compact LINK / PORTAL glyph formed from exactly TWO chunky interlocking geometric hook shapes, one entering from upper left and the other from lower right, arranged along a rising diagonal. Each hook is an open squared C with slightly rounded corners; together they create a connected dynamic symbol and a large simple lightning-shaped negative-space seam through the center. Visual idea: two profiles / two platforms united into one hub. Strong bright gaming character, balanced compact almost-square silhouette, clean premium geometry. Solid vivid orange #FF671D only with genuine transparent background and clean transparent cutouts. Keep very thick strokes and generous clear openings so it reads at 16px. Distinct from an ordinary S letter or basic horizontal exchange arrows. No container circle, no separate letter, no small lines, no circular arrows, no recognizable existing brand logos, no Nintendo Switch controller shape, no wordmark or labels, no mascots or shields. Flat vector-like art, no gradients, glow, shadows, volume or texture. One standalone glyph centered upright on square canvas with even 10% transparent margins. Create a professional coherent symbol, not two unrelated rings or a realistic metal chain.
