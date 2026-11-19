# Hosted test-only; no coordinate fallback or session activation.
function Test-CodexFixtureRename {
    $r = $result.codexHistory
    $ct = [System.Windows.Automation.ControlType]
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type 'using System; using System.Runtime.InteropServices; public static class FixtureForeground { [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow(); }'
    function In-OwnedTree($Element, $Root) {
        for ($depth=0; $Element -and $depth -lt 40; $depth++) {
            if ([System.Windows.Automation.AutomationElement]::Compare($Element,$Root)) { return $true }
            $Element=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($Element)
        }
        return $false
    }
    function Assert-OwnedForeground {
        $hwnd=[IntPtr]::new($window.Current.NativeWindowHandle)
        if ($window.Current.ProcessId -ne $app.Id -or $hwnd -eq [IntPtr]::Zero -or [FixtureForeground]::GetForegroundWindow() -ne $hwnd) {
            throw 'Fixture window is not the active owned window'
        }
    }
    function Send-OwnedKeys([string]$Keys) {
        Assert-OwnedForeground
        $focus=[System.Windows.Automation.AutomationElement]::FocusedElement
        if (!$focus -or !(In-OwnedTree $focus $window)) { throw 'Fixture keyboard focus escaped owned window' }
        [System.Windows.Forms.SendKeys]::SendWait($Keys)
    }
    function Choose-FixtureLanguage([bool]$Chinese) {
        $settings=if ($Chinese) {'Settings'} else {'设置'}
        $general=if ($Chinese) {'General'} else {'通用'}
        $label=if ($Chinese) {'Language'} else {'语言'}
        $target=if ($Chinese) {'中文'} else {'English'}
        $workspace=if ($Chinese) {'工作区'} else {'Workspace'}
        Invoke-Button @($settings)
        Invoke-Button @($general)
        $combo=Wait-Observed { Find-Control @($label) ($ct::ComboBox) } 'application language selector'
        $expanded=$null
        if (!$combo.Current.IsEnabled -or !$combo.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$expanded)) { throw 'App language selector cannot expand' }
        ([System.Windows.Automation.ExpandCollapsePattern]$expanded).Expand()
        $option=Wait-Observed {
            $candidates=@($combo.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) |
                Where-Object { $_.Current.Name -ceq $target -and $_.Current.IsEnabled -and !$_.Current.IsOffscreen })
            if ($candidates.Count -gt 1) { throw 'Ambiguous application language option' }
            if ($candidates.Count -eq 1) { return $candidates[0] }
        } 'owned application language option'
        $select=$null
        if (!$option.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern,[ref]$select)) { throw 'App language option cannot be selected' }
        ([System.Windows.Automation.SelectionItemPattern]$select).Select()
        Invoke-Button @($workspace)
    }
    function Open-FixtureKeyboardMenu([string]$Name) {
        $target=Wait-Observed { Find-Control @($Name) ($ct::TreeItem) } 'unique fixture history row'
        if (!$target.Current.IsKeyboardFocusable -or !$target.Current.IsEnabled -or $target.Current.IsOffscreen -or !(In-OwnedTree $target $window)) {
            throw 'Fixture row is not an eligible keyboard target'
        }
        $match=$false
        for ($i=0; $i -lt 48; $i++) {
            Assert-OwnedForeground
            $focus=[System.Windows.Automation.AutomationElement]::FocusedElement
            if ($focus -and (In-OwnedTree $focus $target) -and $focus.Current.ControlType -ne $ct::Edit) { $match=$true; break }
            Send-OwnedKeys '{TAB}'
            Start-Sleep -Milliseconds 65
        }
        if (!$match) { throw 'Keyboard traversal did not reach the unique fixture row' }
        Assert-OwnedForeground
        $focus=[System.Windows.Automation.AutomationElement]::FocusedElement
        if (!(In-OwnedTree $focus $target) -or !(In-OwnedTree $target $window)) { throw 'Fixture row focus changed before context-menu key' }
        Send-OwnedKeys '+{F10}'
        $menu=Wait-Observed { Find-Control @('会话操作') ($ct::Menu) } 'owned visible Chinese session actions menu'
        if (!(In-OwnedTree $menu $window)) { throw 'Fixture menu escaped owned window' }
        return $menu
    }
    $r.stage='rename-formal-language-zh'
    Choose-FixtureLanguage $true
    $null=Wait-Observed { Find-Control @('搜索会话...') ($ct::Edit) } 'Chinese workspace surface'
    $r.languageUiZh=$true

    $r.stage='rename-negative-archived-keyboard'
    if (!(Test-Path -LiteralPath $fixture.ArchiveRollout -PathType Leaf)) { throw 'Synthetic archived fixture leaf missing' }
    Invoke-Button @('已存档会话')
    $menu=Open-FixtureKeyboardMenu $fixture.ArchivedCanary
    $members=@($menu.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) |
      Where-Object { $_.Current.ControlType -eq $ct::MenuItem })
    if (@($members | Where-Object { $_.Current.Name -ceq '复制会话 ID' -and !$_.Current.IsOffscreen }).Count -ne 1 -or
        @($members | Where-Object { $_.Current.Name -ceq '重命名' }).Count -ne 0) { throw 'Archived menu eligibility observation failed' }
    $r.archivedRenameAbsent=$true
    Send-OwnedKeys '{ESC}'
    $null=Wait-Observed { !(Find-Control @('会话操作') ($ct::Menu)) } 'archive menu dismissed'
    Invoke-Button @('关闭')
    $null=Wait-Observed { !(Find-Control @($fixture.ArchivedCanary) ($ct::TreeItem)) } 'archived fixture drawer closed'

    $r.stage='rename-positive-active-keyboard'
    Set-FixtureEdit '搜索会话...' $fixture.ActiveCanary
    $search=Wait-Observed { Find-Control @('搜索会话...') ($ct::Edit) } 'Chinese session search field'
    $search.SetFocus()
    $menu=Open-FixtureKeyboardMenu $fixture.ActiveCanary
    if (!(Invoke-FixtureTarget $menu '重命名' ($ct::MenuItem))) { throw 'Eligible fixture does not expose Chinese rename entry' }
    $r.activeRenameZh=$true

    $r.stage='rename-submit-once'
    Set-FixtureEdit '会话名称' 'Pr101RenamedCanary'
    $row=Wait-Observed { Find-Control @($fixture.ActiveCanary) ($ct::TreeItem) } 'exact synthetic rename row before saving'
    if (!(Invoke-FixtureTarget $row '保存名称' ($ct::Button))) { throw 'Fixture rename save unavailable' }
    $r.rename='sent'
    $r.stage='rename-refresh-unique-active'
    Set-FixtureEdit '搜索会话...' 'Pr101RenamedCanary'
    $null=Wait-Observed { Find-Control @('Pr101RenamedCanary') ($ct::TreeItem) } 'unique fixture rename becomes visible'
    Invoke-Button @('刷新会话')
    $null=Wait-Observed { Find-Control @('刷新会话') ($ct::Button) | Where-Object { $_.Current.IsEnabled } } 'formal refresh control settles'
    $null=Wait-Observed { Find-Control @('Pr101RenamedCanary') ($ct::TreeItem) } 'unique fixture rename persists after refresh'
    $search=Wait-Observed { Find-Control @('搜索会话...') ($ct::Edit) } 'Chinese session search field after refresh'
    $search.SetFocus()
    $menu=Open-FixtureKeyboardMenu 'Pr101RenamedCanary'
    if (!(Invoke-FixtureTarget $menu '复制会话 ID' ($ct::MenuItem))) { throw 'Native ID unavailable in renamed row' }
    $null=Wait-Observed {
        $actual=Get-Clipboard -Raw
        $actual -and $actual.Length -le 128 -and $actual.Trim() -ceq $fixture.SessionId
    } 'same native UUID in exact unique refreshed row'
    $r.renamedTitleObserved=$true
    $r.refreshedNativeIdMatch=$true

    $r.stage='rename-readonly-provider'
    $proof=& (Join-Path $PSScriptRoot 'pr101-codex-history-fixture.ps1') -Codex $FixtureCodex -ReadOnlyExisting $fixture -ObserveRename
    $r.readonlyPostRename=[ordered]@{observed=$true; sameId=$proof.sameId; sameCwd=$proof.sameCwd; expectedName=$proof.expectedName}
    if (!$proof.sameId -or !$proof.sameCwd -or !$proof.expectedName) { throw 'Independent fixture identity/name did not match' }
    $r.rename='native-name-observed'
    $r.stage='rename-formal-language-en'
    Choose-FixtureLanguage $false
}
