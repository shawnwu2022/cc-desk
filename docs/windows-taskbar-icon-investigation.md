# Windows taskbar icon investigation — 2026-10-19

## Current conclusion

The user reported at 10:10 UTC that restarting Explorer restored the correct
taskbar icon. Stop expanding this investigation. This recovery is consistent
with Shell cache/refresh association, but does not prove its exact internal
cause. No additional window
icon, AppUserModelID, pinning or cache change is made on `fix/v0.23.3` from this
report. The independently confirmed missing NSIS `installerIcon` is fixed on the
same branch; that installer-shell change does not prove the taskbar issue fixed.

The affected machine reports a correct upper-left app icon and Start menu icon,
but an old taskbar icon. Its running version/path and pinned shortcut properties
are still unknown. CC Desk removes native Windows decorations and renders its
upper-left icon in Vue `TitleBar.vue`; that image alone does not validate the
HWND's native small/big icons. WSK-MSI is an investigation host, not evidence of
the affected machine's state.

## Source and release-package evidence

The actual [v0.23.2 Windows installer](https://github.com/shawnwu2022/cc-desk/releases/download/v0.23.2/CC.Desk_0.23.2_x64-setup.exe)
was downloaded and its SHA-256 matched GitHub release asset **655801636**:
`4c880b0c1d0c374448832a03120b49aa42c9f9168f8a14e1ce3a0c7e06fc7ca0`.
The release target is `316279818c57671e4f82eb41e7d051dc946f97ae`.
7-Zip only extracted `cc-desk.exe`; neither installer nor application was run.

The extracted app SHA-256 is
`5727809c30a4330e202db78a8bffcddf14fbb76c0629763ddb688e0d5b74d46d`.
Its outer PE icon group contains all seven approved frames, byte-exact. The
approved 16px RGBA used by the locked Tauri default-window-icon decoder is also
present at byte offset 23690096; the v0.23.1 16px RGBA sequence is absent. This
checks a separate compiled runtime source, beyond the PE icon group, but is not
a live HWND/taskbar measurement. The released installer's outer 32px icon matches
neither the old nor approved app's 32px pixels; it is a separate installer icon.

Local evidence is retained under `.cache/v0233-verification/taskbar/` and the
download/extraction under `.cache/v0233-verification/release-v0.23.2/`.

## Icon and identity paths

| Surface | Locked source / behavior | Evidence boundary |
| --- | --- | --- |
| Custom upper-left icon | `TitleBar.vue` imports approved PNG/srcset | Frontend image, not native taskbar proof |
| Windows executable | Tauri build embeds `icons/icon.ico` as resource 32512 | Released app's seven frames verified |
| Default native window icon | `tauri-codegen 2.5.5` selects first configured ICO, decodes its first frame; `tauri 2.10.3` supplies it when a window has no custom icon | Compiled RGBA verified; live handle unmeasured |
| Small/big window icons | `tao 0.34.8` sets the window icon as `ICON_SMALL`; separate `taskbar_icon` defaults to None / unset `ICON_BIG` | Framework behavior, not by itself proof of an old taskbar icon |
| Runtime overrides | No project `setIcon`, tray icon, taskbar icon, explicit process/window AppUserModelID or relaunch-icon override found | No speculative override added |
| New NSIS shortcuts | Locked CLI 2.10.1 generated template targets `$INSTDIR\cc-desk.exe`, no separate IconLocation argument; sets shortcut AppUserModelID `io.github.shawnwu2022.ccdesk` | Template/source checked; affected installed `.lnk` not read |
| Upgrade | UpdateMode / NoShortcutMode skips shortcut recreation; old executable-name migration changes the target only | Existing shortcut/pinned state can survive; affected state unknown |
| Installer hooks | Only Explorer context-menu entries reference `$INSTDIR\cc-desk.exe` | No taskbar/pinned-shortcut rewrite |

The project has no custom pinned-item refresh. The absence of an explicit runtime
AppUserModelID is not sufficient to prove a mismatch: Shell launch association
can participate, and the affected process/window/shortcut properties are unknown.

Microsoft documents that taskbar identity associates processes/windows with
shortcuts, and that a matching shortcut can supply taskbar relaunch icon/text.
An explicit window identity can also use a relaunch-icon property. These are
separate sources from a titlebar image or executable resource. See
[Application User Model IDs](https://learn.microsoft.com/en-us/windows/win32/shell/appids)
and [RelaunchIconResource](https://learn.microsoft.com/en-us/windows/win32/properties/props-system-appusermodel-relaunchiconresource).

## If the issue recurs: minimum affected-machine evidence

No further field collection is currently required after the user's recovery.
If it recurs, collect these without closing sessions or changing the pinned item:

1. Is the old taskbar button pinned, and is it a running/active button or only an
   idle shortcut? Does opening from Start join it or create a second button?
   Observe the next normal launch; do not start another instance just for this.
2. About version, plus the running `cc-desk.exe` path from Task Manager's
   **Open file location** / process properties. This distinguishes an older live
   process or different installation from the released package checked here.
3. Read the taskbar shortcut's **Properties** (right-click its taskbar menu's
   app name): Target and current icon file/index. Compare with the correct Start
   menu shortcut. Do not save, repin or change the icon.

If these point to the same current application, a read-only technical follow-up
can compare shortcut `System.AppUserModel.ID`, relaunch icon/command properties,
and the running HWND's `WM_GETICON` small/small2/big and class icons. Only after
those sources agree should stale Shell icon state become the leading diagnosis.
No cache deletion, Explorer restart, ACL change, installation replacement,
pin/unpin operation or user-session termination is part of this investigation.
