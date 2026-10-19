# CC Desk application icon

## Approved source

The application mark is the approved refined stacked terminal: charcoal `#35404B` front card, orange `#FA8939` rear card offset up/right, and white `#FFFFFF` prompt on a transparent canvas. Keep this artwork unchanged in light and dark themes. Claude Code / Codex CLI marks and session-state artwork remain separate.

`src-tauri/icons/source/` retains every file from the approved `ccdesk-icon-refined.zip`, unchanged, including the hand-authored master SVG, 16/24/32 optical SVGs, nine transparent PNGs, seven-frame Windows ICO, previews, renderer QA, original export script and `SHA256SUMS.txt`.

Original ZIP SHA-256: `bed9870abdd80fea2cd3178c95b8ce8e3374c60c1d0a05386121044e232819f2` (125,271 bytes). The delivered preview was inspected on light and dark backgrounds. These are CC Desk-owned product assets; third-party CLI mark attribution remains in `src/assets/icons/cli/README.md`.

The 19-file manifest is byte-exact across platforms; `.gitattributes` disables newline conversion for this source tree. Do not regenerate or edit the approved source merely to update the application outputs.

## Application and packaging references

| Consumer | Checked-in asset | Approved source |
| --- | --- | --- |
| TitleBar, 16 CSS px | `src/assets/icons/app-icon-{16,24,32,48,64}.png` | Exact PNGs at 1x/1.5x/2x/3x/4x |
| Settings About, 56 CSS px | `src/assets/icons/app-icon.png` | 256 PNG |
| Browser favicon | `src/assets/icons/app-icon.ico` via `index.html` | Original seven-frame ICO |
| Tauri Linux PNGs | `src-tauri/icons/{32x32,128x128,128x128@2x}.png` | 32 optical / 128 / 256 PNGs |
| Tauri Windows icon | `src-tauri/icons/icon.ico` | Original ICO: 16/24/32/48/64/128/256 |
| Windows NSIS installer shell | `bundle.windows.nsis.installerIcon`: `icons/icon.ico` | Same original seven-frame ICO |
| Tauri macOS icon | `src-tauri/icons/icon.icns` | Tauri-encoded family using approved size-specific pixels |
| High-resolution source | `src-tauri/icons/cc-desk-icon-source.png` | 1024 PNG |

The explicit `app-icon` CSS class exempts these images from the dark-theme filter, including when Vite inlines small PNGs into data URLs. Vite imports the titlebar `srcset` and favicon so production HTML/chunks reference emitted assets. Existing Tauri bundle icon paths stay stable; `tauri.windows.conf.json` does not override them. Normal release builds consume these checked-in files through the unchanged Windows NSIS, macOS App/DMG and Linux AppImage build paths.

NSIS requires its own explicit `installerIcon`; `bundle.icon` alone updates the
application executable but does not select the installer shell icon. Keep both
references on the approved ICO. The [Tauri config reference](https://v2.tauri.app/reference/config/#installericon)
defines this separate field. It is a compile-time installer resource, not an
extra installed file requiring a `bundle.resources` entry.

## Reproduce application outputs

From the repository root, after `npm ci`:

```sh
npm run icons:generate
npm run icons:check
```

Both commands verify all 19 source hashes. Generation copies the exact PNG/ICO bytes and invokes the lockfile's Tauri CLI (2.10.1 at integration) in private temporary directories. No network, Python, Inkscape or system installation is required for this step. `icons:check` regenerates the expected bytes in temporary directories and rejects any stale output without changing the repository.

ICNS keeps the standard Tauri family types. The legacy 16px (`is32/s8mk`) and 32px (`il32/l8mk`) frames use uncompressed RGB plus alpha decoded directly from the approved ICO DIBs, independently verified against the optical PNGs. This avoids Tauri's resize premultiplication/rounding of edge colors. PNG-backed standard/Retina entries use exact approved PNG bytes: `ic11` = 32, `ic12` = 64, `ic07` = 128, `ic08/ic13` = 256, `ic09/ic14` = 512, `ic10` = 1024. A fixed entry order removes Tauri's nondeterministic collection order. A changed type inventory fails closed for review. Other temporary Tauri outputs are discarded.

The original `source/tools/build_assets.py` is retained as export provenance. A full SVG rerender has additional Inkscape, Pillow and Linux DejaVu font requirements documented in the original README/script. Run it only in a separate copy of the original delivery tree and review pixel/hash differences before intentionally replacing any approved source. It is not part of normal application output generation.

## Verification and boundaries

With Python 3 and Pillow already available:

```sh
python tests/scripts/appIcons.py
```

This independently decodes all nine PNGs, seven ICO frames and ten standard/Retina ICNS representations. It verifies dimensions, alpha extrema, transparent canvas borders, ICO DIB/PNG framing and exact RGBA pixels against the approved exports. It also checks generated frontend/native copies and packaging paths. It performs no writes or downloads.

After an authorized local NSIS test build, verify the actual installer shell
without running or installing it (Python standard library only):

```sh
python tests/scripts/nsisInstallerIcon.py "path/to/CC Desk_VERSION_x64-setup.exe"
```

The script reads the outer PE's RT_GROUP_ICON and RT_ICON resources and requires
one complete icon group to match all seven approved ICO frames byte-for-byte.
It does not mistake the bundled application's icon for the installer shell.

Integration validation includes source/manifest checks, repeated byte-stable generation, image decoding, session-icon regressions, typecheck, production build and browser verification of emitted icon references and light/dark colors. These checks do not certify installed taskbar/Start menu rendering, macOS Finder/Dock behavior, Linux desktop icon caches or OS fractional scaling. Signing and publication are verified separately through the standard protected-main release process. The icon integration does not install the app or alter release gates; authorized version metadata changes follow the normal release policy.

The taskbar-only v0.23.2 field report has a separate
[source/package investigation and minimal read-only field checklist](windows-taskbar-icon-investigation.md).
The user reported recovery after restarting Explorer; the exact internal cause
remains unproven. No taskbar code change was added, and the installer-shell icon
fix must not be presented as its confirmed remedy.
