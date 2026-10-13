# SWCH application icons

The logo is a separate S with two clockwise arrows in a minimalist orange and
near-black design. Windows uses the approved circular icon; macOS uses the
approved rounded-square tile. Both have transparent outer backgrounds.
The masters were created with the built-in Imagegen tool.

- Windows master: `src/assets/swch-minimal.png`.
- macOS master: `src/assets/swch-macos-minimal.png`.
- Windows resource: `src/assets/swch.ico`, embedded by `src/windows_app.rc`.
- macOS resource: `swch.app/Contents/Resources/iconfile.icns`, selected by `CFBundleIconFile`.

On Windows, run `./scripts/build-app-icons.ps1` after replacing a master to
regenerate the ICO and ICNS sizes. This only packages the finished artwork and
preserves transparency; it does not generate or redesign the logo.

## Final Windows prompt

Use case: style-transfer. Edit the attached SWCH logo into a MUCH MORE MINIMALIST flat graphic. Keep its identity: one orange circle, one normal black uppercase S centered, and exactly two clockwise curved arrows forming the very outer edge of the orange circle. Use ONLY two solid colors: orange #FF641E and near-black #191919, plus genuine transparency outside. Absolutely no gradients, no highlights, no shine, no bevel, no 3D volume, no shadow, no textures. A crisp geometric vector-like design. Make the arrow outline noticeably thinner and simpler than the reference, uniform stroke roughly 4% of the circle diameter. Two small simple opposed arrowheads at roughly 2 and 8 o'clock, two neat small breaks. The arrow strokes follow the circumference exactly: their outside edges coincide with the orange disk edge. Keep arrowheads compact and inscribed in the circular silhouette, no bulging corner or pointed projection beyond the circle, no outer orange rim. The S is a clean bold modern sans-serif letter without any tail or attached arrow, centered, with ample clear orange space around it. Perfect front-facing round geometry, equal width and height, no tilt or perspective. One standalone icon with generous transparent margins, clean smooth antialiased edges and true alpha outside. No other elements, no text labels, no mockup, no checkerboard. The result should feel like a simple professional app symbol, NOT a rendered 3D badge.

## Final macOS prompt

Use case: style-transfer. Asset: macOS app icon for SWCH based on the approved minimalist Windows logo in the reference. Keep the same recognizable clean black S and two clockwise circular switching arrows, their proportions, the orange and near-black palette, and the flat minimalist style. Change the outer orange background silhouette into ONE upright, front-facing macOS rounded square / superellipse tile with smooth generously rounded corners, equal width and height, no tilt or perspective. Center the S and the two-arrow circular mark on this orange tile, with even comfortable padding. The arrows remain a circular refresh ring around the separate S, EXACTLY TWO arrowheads and two small opposite breaks. No arrow attached to the S. The orange tile is solid warm orange close to #FF641E and the glyph near-black #191919. Flat restrained finish like the reference: no 3D volume, no bevel, no shiny highlights, no shadows, no metallic look. Simple crisp smooth shapes readable at 32px. ONE tile only, no nested backing squares, no white backing plate, no Apple logo, no words, no labels, no mockup, no checkerboard. True alpha transparency outside the rounded square, with generous transparent canvas margins. The result must look like the same approved logo adapted to a native macOS rounded-square app icon, not a new symbol.
