# CC Desk — stacked terminal icon

Standalone, hand-authored SVG reconstruction of the approved refined stacked-terminal concept. No application repository, release, system setting or original raster reference was changed.

## Deliverables
- `svg/ccdesk-master.svg`: 256 × 256 vector master, intended for 48 px and larger exports
- `svg/ccdesk-16.svg`, `ccdesk-24.svg`, `ccdesk-32.svg`: separately tuned vector variants for small display sizes
- `png/ccdesk-{size}.png`: transparent 8-bit RGBA PNGs at 16, 24, 32, 48, 64, 128, 256, 512 and 1024 px
- `windows/ccdesk.ico`: 16, 24, 32, 48, 64, 128 and 256 px frames, using the corresponding PNG geometry at every size
- `previews/small-size-preview.png`: labeled light/dark board, with native-size icons and nearest-neighbor enlargements
- `previews/small-size-strip.png`: compact native-size / nearest-neighbor 4× pixel-inspection strip
- `qa.json`: renderer details and automated export checks
- `tools/build_assets.py`: local, repeatable export script; requires Python 3, Pillow and Inkscape

## Design specification
- Orange: `#FA8939`
- Charcoal: `#35404B`, lifted slightly to retain an edge on dark backgrounds
- Prompt: `#FFFFFF`
- Transparent canvas; flat fills only. No gradients, shadows, embedded bitmap, external font or linked asset dependencies in the SVGs
- Frontal rounded terminal card; orange rear card offset toward the top-right
- White greater-than and underscore are vector geometry, not text glyphs
- Master viewBox: `0 0 256 256`
- Master silhouette: x = 20–236, y = 34–218; front card 196 × 161, rear card 182 × 152
- Master front radius: 20; rear radius: 22; prompt thickness: 17
- At 16/24/32 px the silhouette is slightly fuller, prompt weight/placement and offsets are tuned to those pixel grids. Use the matching small SVG/PNG instead of downscaling the master for those exact sizes

## Verification and limitations
- All 9 PNGs are RGBA, have fully transparent canvas pixels, and have empty space at every canvas edge; no clipping
- All 7 ICO frames were decoded and compared pixel-for-pixel against the matching PNG exports
- ICO uses 32-bit BGRA DIB data plus 1-bit AND transparency masks for 16–128 px, with a PNG-compressed 256 px frame
- Preview boards composite the exported pixels onto light `#F4F5F7` and dark `#1D242C` backgrounds. The marked 1× views are native pixels in the image file. View the board at 100% zoom to inspect true image-pixel size
- No installed Windows app, taskbar, tray, Start menu or executable resource test was performed. Windows integration and release packaging are separate steps
- SVG rasterization and fractional OS scaling may vary by renderer. The included PNGs and ICO fix the intended pixels for the listed sizes

## Rebuild
Run `python tools/build_assets.py` from any working directory. The script rewrites this standalone asset folder only. No package installation or network access is performed.
