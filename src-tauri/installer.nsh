; CC Desk NSIS Installer Hooks
; Windows 资源管理器右键菜单注册

!macro NSIS_HOOK_POSTINSTALL
  ; 清理旧版 CC-Box 右键菜单，避免升级后出现两个入口
  DeleteRegKey HKCU "Software\Classes\Directory\shell\cc-box"
  DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\cc-box"

  ; 右键文件夹："使用 CC Desk 打开"
  WriteRegStr HKCU "Software\Classes\Directory\shell\cc-desk" "" "使用 CC Desk 打开"
  WriteRegStr HKCU "Software\Classes\Directory\shell\cc-desk" "Icon" "$INSTDIR\cc-desk.exe"
  WriteRegStr HKCU "Software\Classes\Directory\shell\cc-desk\command" "" '"$INSTDIR\cc-desk.exe" "%1"'

  ; 右键空白处："在此处打开 CC Desk"
  WriteRegStr HKCU "Software\Classes\Directory\Background\shell\cc-desk" "" "在此处打开 CC Desk"
  WriteRegStr HKCU "Software\Classes\Directory\Background\shell\cc-desk" "Icon" "$INSTDIR\cc-desk.exe"
  WriteRegStr HKCU "Software\Classes\Directory\Background\shell\cc-desk\command" "" '"$INSTDIR\cc-desk.exe" "%V"'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; 移除右键菜单注册表项
  DeleteRegKey HKCU "Software\Classes\Directory\shell\cc-desk"
  DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\cc-desk"
  DeleteRegKey HKCU "Software\Classes\Directory\shell\cc-box"
  DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\cc-box"
!macroend
