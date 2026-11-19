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
    function Open-FixtureKeyboardMenu([string]$Name, [ValidateSet('archived','active','refreshed')][string]$Phase) {
        $r.stage="rename-$Phase-find-row"
        $target=Wait-Observed { Find-Control @($Name) ($ct::TreeItem) } 'unique fixture history row'
        if (!$target.Current.IsKeyboardFocusable -or !$target.Current.IsEnabled -or $target.Current.IsOffscreen -or !(In-OwnedTree $target $window)) {
            throw 'Fixture row is not an eligible keyboard target'
        }
        $r.stage="rename-$Phase-owned-focus"
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
        $r.stage="rename-$Phase-shift-f10"
        Send-OwnedKeys '+{F10}'
        $menu=Wait-Observed { Find-Control @('Session actions') ($ct::Menu) } 'owned visible English session actions menu'
        if (!(In-OwnedTree $menu $window)) { throw 'Fixture menu escaped owned window' }
        return $menu
    }
    $r.stage='rename-confirm-english-baseline'
    $null=Wait-Observed { Find-Control @('Search sessions...') ($ct::Edit) } 'English workspace surface'
    $r.menuLanguage='en'
    $r.chineseUiAccepted=$false

    $r.stage='rename-negative-archived-keyboard'
    if (!(Test-Path -LiteralPath $fixture.ArchiveRollout -PathType Leaf)) { throw 'Synthetic archived fixture leaf missing' }
    Invoke-Button @('Archived sessions')
    $menu=Open-FixtureKeyboardMenu $fixture.ArchivedCanary 'archived'
    $r.stage='rename-archived-menu-eligibility'
    $members=@($menu.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) |
      Where-Object { $_.Current.ControlType -eq $ct::MenuItem })
    if (@($members | Where-Object { $_.Current.Name -ceq 'Copy session ID' -and !$_.Current.IsOffscreen }).Count -ne 1 -or
        @($members | Where-Object { $_.Current.Name -ceq 'Rename' }).Count -ne 0) { throw 'Archived menu eligibility observation failed' }
    $r.archivedRenameAbsent=$true
    Send-OwnedKeys '{ESC}'
    $null=Wait-Observed { !(Find-Control @('Session actions') ($ct::Menu)) } 'archive menu dismissed'
    Invoke-Button @('Close')
    $null=Wait-Observed { !(Find-Control @($fixture.ArchivedCanary) ($ct::TreeItem)) } 'archived fixture drawer closed'

    $r.stage='rename-positive-active-keyboard'
    Set-FixtureEdit 'Search sessions...' $fixture.ActiveCanary
    $search=Wait-Observed { Find-Control @('Search sessions...') ($ct::Edit) } 'English session search field'
    $search.SetFocus()
    $menu=Open-FixtureKeyboardMenu $fixture.ActiveCanary 'active'
    $r.stage='rename-active-menu-invoke'
    if (!(Invoke-FixtureTarget $menu 'Rename' ($ct::MenuItem))) { throw 'Eligible fixture does not expose English rename entry' }
    $r.activeRenameExposed=$true

    $r.stage='rename-submit-once'
    Set-FixtureEdit 'Session name' 'Pr101RenamedCanary'
    $row=Wait-Observed { Find-Control @($fixture.ActiveCanary) ($ct::TreeItem) } 'exact synthetic rename row before saving'
    if (!(Invoke-FixtureTarget $row 'Save name' ($ct::Button))) { throw 'Fixture rename save unavailable' }
    $r.rename='sent'
    $r.stage='rename-refresh-unique-active'
    Set-FixtureEdit 'Search sessions...' 'Pr101RenamedCanary'
    $null=Wait-Observed { Find-Control @('Pr101RenamedCanary') ($ct::TreeItem) } 'unique fixture rename becomes visible'
    Invoke-Button @('Refresh sessions')
    $null=Wait-Observed { Find-Control @('Refresh sessions') ($ct::Button) | Where-Object { $_.Current.IsEnabled } } 'formal refresh control settles'
    $null=Wait-Observed { Find-Control @('Pr101RenamedCanary') ($ct::TreeItem) } 'unique fixture rename persists after refresh'
    $search=Wait-Observed { Find-Control @('Search sessions...') ($ct::Edit) } 'English session search field after refresh'
    $search.SetFocus()
    $menu=Open-FixtureKeyboardMenu 'Pr101RenamedCanary' 'refreshed'
    $r.stage='rename-refreshed-id-copy'
    if (!(Invoke-FixtureTarget $menu 'Copy session ID' ($ct::MenuItem))) { throw 'Native ID unavailable in renamed row' }
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
}
