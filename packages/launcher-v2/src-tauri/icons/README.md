# launcher-v2 icons

The icons committed here are generated from the canonical brand SVG at
[`site/src/assets/psycheros-logo.svg`](../../../../site/src/assets/psycheros-logo.svg)
— heart silhouette with the cyan→purple chip gradient. Transparent background;
designed to render cleanly on macOS's dock + dark Finder.

## Regenerating

When the canonical SVG changes, regenerate from the repo root:

```bash
cd packages/launcher-v2/src-tauri/icons

# 1. Render the source SVG to a 1024×1024 RGBA PNG.
sips -s format png --resampleHeightWidth 1024 1024 \
  ../../../../site/src/assets/psycheros-logo.svg \
  --out icon.png

# 2. Have Tauri's CLI generate every platform variant.
cd ../..
npx --yes @tauri-apps/cli@^2.0 icon src-tauri/icons/icon.png \
  --output src-tauri/icons/
```

`tauri icon` produces the full mobile + Windows-tile suite alongside the desktop
set. The mobile + tile variants are ignored via
[`packages/launcher-v2/.gitignore`](../../.gitignore) so they don't clutter PRs
— the launcher only ships the desktop variants referenced from
[`tauri.conf.json`](../tauri.conf.json):

- `32x32.png`, `128x128.png`, `128x128@2x.png` (256×256)
- `icon.icns` (macOS multi-resolution)
- `icon.ico` (Windows multi-resolution)
- `icon.png` (canonical 1024 — used as the source for future regenerations)

## Tauri's RGBA requirement

Tauri's `generate_context!` macro validates icon files at compile time and
rejects anything that isn't RGBA (PNG color type 6). The `sips` pipeline above
produces the right format. If a future hand-edited icon causes `cargo tauri dev`
to panic with `icon is not RGBA`, the most likely culprit is the input PNG using
color type 2 (RGB) — re-export with an alpha channel.
