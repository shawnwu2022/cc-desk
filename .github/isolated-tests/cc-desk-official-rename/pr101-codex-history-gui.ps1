# Sourced only by installed hosted acceptance with a verified, synthetic fixture.
function Invoke-FixtureTarget($Root, [string]$Name, $Type, [switch]$WorkspaceHeader) {
    $specific = @($Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object {
        if ($_.Current.Name -cne $Name -or ($Type -and $_.Current.ControlType -ne $Type)) { return $false }
        if (!$WorkspaceHeader) { return $true }
        $parent = [System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($_)
        if (!$parent) { return $false }
        return @($parent.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.Condition]::TrueCondition) |
            Where-Object { $_.Current.Name -ceq 'Collapse sessions panel' }).Count -eq 1
    })
    if ($specific.Count -gt 1) { throw 'Ambiguous synthetic fixture control' }
    if ($specific.Count -eq 0) { return $false }
    $ctl=$specific[0]
    if ($ctl.Current.IsOffscreen) {
        $scroll=$null
        if (!$ctl.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)) { return $false }
        ([System.Windows.Automation.ScrollItemPattern]$scroll).ScrollIntoView()
    }
    $pattern=$null
    if ($ctl.Current.IsOffscreen -or !$ctl.Current.IsEnabled -or !$ctl.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)) { return $false }
    ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
    return $true
}

function Set-FixtureEdit([string]$Name,[string]$Value) {
    $editType=[System.Windows.Automation.ControlType]::Edit
    Reveal-FixtureControl @($Name) $editType
    $ctl=Wait-Observed { Find-Control @($Name) $editType } "synthetic configuration: $Name"
    $pattern=$null
    if (!$ctl.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)) { throw "Synthetic edit not writable: $Name" }
    $pattern=[System.Windows.Automation.ValuePattern]$pattern
    $pattern.SetValue($Value)
    $null=Wait-Observed {
        $observed=Find-Control @($Name) $editType
        $read=$null
        if ($observed -and $observed.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$read) -and
            ([System.Windows.Automation.ValuePattern]$read).Current.Value -ceq $Value) { return $observed }
    } "exact synthetic edit readback: $Name"
}

function Configure-CodexFixtureProfile {
    $result.codexHistory.stage='create-official-codex-profile'
    $result.codexHistory.stage='settings-open'
    Invoke-Button @('Settings')
    $result.codexHistory.stage='settings-launch-configurations'
    Invoke-Button @('Launch configurations')
    $name='Pr101SyntheticCodexProfile'
    $button=[System.Windows.Automation.ControlType]::Button
    $result.codexHistory.stage='codex-config-create-control'
    $target=Wait-Observed {
        $tools=@($window.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) | Where-Object {
            $_.Current.Name -ceq 'New configuration' -and $_.Current.ControlType -eq $button -and
            $null -ne [System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($_) -and
            @([System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($_).FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) | Where-Object {
                $_.Current.Name -ceq 'Codex CLI' -and $_.Current.ControlType -ne $button
            }).Count -eq 1
        })
        if ($tools.Count -gt 1) { throw 'Ambiguous Codex profile group' }
        if ($tools.Count -eq 1) { $tools[0] }
    } 'new configuration in exact Codex CLI group'
    if ($target.Current.IsOffscreen) {
        $scroll=$null
        if (!$target.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)) { throw 'Codex configuration group cannot reveal its action' }
        ([System.Windows.Automation.ScrollItemPattern]$scroll).ScrollIntoView()
    }
    $click=$null
    if (!$target.Current.IsEnabled -or !$target.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$click)) { throw 'Codex configuration action unavailable' }
    ([System.Windows.Automation.InvokePattern]$click).Invoke()
    $result.codexHistory.stage='codex-config-name'
    Set-FixtureEdit 'Configuration name' $name
    $result.codexHistory.stage='codex-config-options'
    $details=Wait-Observed {
        $label="More options$([char]0x2026)"
        $same = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) |
            Where-Object { $_.Current.Name -ceq $label })
        if ($same.Count -gt 8) { throw 'Too many identically labelled profile controls' }
        $result.codexHistory.optionsControls=@($same | ForEach-Object { [ordered]@{
            type=$_.Current.ControlType.ProgrammaticName
            patterns=@($_.GetSupportedPatterns() | ForEach-Object ProgrammaticName)
        } })
        $action=@($same | Where-Object {
            $pattern=$null
            $_.Current.IsEnabled -and !$_.Current.IsOffscreen -and
                $_.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$pattern)
        })
        if ($action.Count -gt 1) { throw 'Ambiguous actionable disclosure in Codex configuration' }
        if ($action.Count -eq 1) { return $action[0] }
    } 'unique actionable Codex configuration options'
    $expand=$null
    if (!$details.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$expand)) { throw 'Missing profile options disclosure' }
    ([System.Windows.Automation.ExpandCollapsePattern]$expand).Expand()
    $result.codexHistory.stage='codex-config-program-mode'
    $combo=Wait-Observed { Find-Control @('CLI program override') ([System.Windows.Automation.ControlType]::ComboBox) } 'Codex executable override'
    $expand=$null
    if (!$combo.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$expand)) { throw 'Missing executable override disclosure' }
    ([System.Windows.Automation.ExpandCollapsePattern]$expand).Expand()
    $choice=Wait-Observed { Find-Control @('Set') ([System.Windows.Automation.ControlType]::ListItem) } 'exact program override setting'
    $selection=$null
    if (!$choice.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern,[ref]$selection)) { throw 'Missing executable override selection' }
    ([System.Windows.Automation.SelectionItemPattern]$selection).Select()
    $result.codexHistory.stage='codex-config-program-readback'
    Set-FixtureEdit 'Program path' $FixtureCodex
    $result.codexHistory.stage='codex-config-save'
    Reveal-FixtureControl @('Save') $button
    Invoke-Button @('Save')
    $null=Wait-Observed {
        if (Find-Control @('Configuration name') ([System.Windows.Automation.ControlType]::Edit)) { return $null }
        Find-Control @($name) ([System.Windows.Automation.ControlType]::Text)
    } 'closed editor and confirmed saved official Codex profile in GUI'
    $result.codexHistory.stage='codex-config-saved'
    $result.codexHistory.profileSaved=$true
    Invoke-Button @('Workspace')
}
function Test-CodexHistoryGui {
    $r = $result.codexHistory
    $fixtureButton=[System.Windows.Automation.ControlType]::Button
    Invoke-Button @('Workspace')
    $r.stage='project-select'
    $null=Wait-Observed { Invoke-FixtureTarget $window 'Add Project' $fixtureButton -WorkspaceHeader } 'synthetic project Add Project'
    $main=$window
    try {
        $window=Wait-Observed {
            $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$app.Id)
            $dialogs=@($main.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition) | Where-Object {
                !$_.Current.IsOffscreen -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Window -and
                $_.Current.NativeWindowHandle -ne $main.Current.NativeWindowHandle -and $_.Current.Name -ceq 'Select Project Directory'
            })
            if ($dialogs.Count -gt 1) { throw 'Ambiguous synthetic project picker' }
            if ($dialogs.Count -eq 1) { return $dialogs[0] }
        } 'synthetic owned project picker'
        $folder=Wait-Observed { Find-Control @('Folder:') ([System.Windows.Automation.ControlType]::Edit) } 'synthetic project folder field'
        $set=$null
        if (!$folder.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$set)) { throw 'Synthetic folder field is read-only' }
        ([System.Windows.Automation.ValuePattern]$set).SetValue($fixture.Project)
        Invoke-Button @('Select Folder', 'Select folder')
    } finally { $window=$main }
    Configure-CodexFixtureProfile
    $r.stage='refresh-bound-fixture-history'
    $null=Wait-Observed { Invoke-FixtureTarget $window 'Refresh sessions' $fixtureButton } 'refresh formally bound synthetic history'
    $r.stage='verify-profile-project-cwd'
    $ownedSettings=Get-Item -LiteralPath $homeRoot
    if (!$ownedSettings.PSIsContainer -or ($ownedSettings.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unsafe synthetic workspace settings root' }
    $statePath=Join-Path $homeRoot 'cli-workspace.v1.json'
    $stateFile=Get-Item -LiteralPath $statePath
    if ($stateFile.Length -gt 1048576 -or ($stateFile.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Untrusted synthetic workspace receipt' }
    $manifest=Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json -AsHashtable
    $matchingProfile=@($manifest.profiles.Values | Where-Object {
        $_.name -ceq 'Pr101SyntheticCodexProfile' -and $_.cli -ceq 'codex' -and $_.launcher.kind -ceq 'native' -and
        $_.programPath.mode -ceq 'set' -and $_.programPath.value -ceq $FixtureCodex -and
        ($_.defaultArgs.mode -ceq 'inherit' -or ($_.defaultArgs.mode -ceq 'set' -and @($_.defaultArgs.value).Count -eq 0)) -and
        @($_.env.Keys).Count -eq 0
    })
    $projects=@($manifest.registeredProjects.Values)
    $meta=Get-Content -LiteralPath $fixture.Rollout -TotalCount 1 | ConvertFrom-Json -AsHashtable
    $r.selectedProjectExact=@($projects | Where-Object selectedPath -CEQ $fixture.Project).Count -eq 1
    $r.recordedCanonicalExact=@($projects | Where-Object canonicalPath -CEQ $fixture.Project).Count -eq 1
    $r.metaCwdExact=$meta.payload.cwd -ceq $fixture.Project
    $r.profilePinMatches=$matchingProfile.Count -eq 1
    if (!$r.selectedProjectExact -or !$r.metaCwdExact -or !$r.profilePinMatches) { throw 'Stored synthetic project or bound profile does not match exact fixture inputs' }
    Test-ExactActiveRename
}

function Find-Scoped($Root, [string]$Name, $Type) {
    $items=@($Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) |
        Where-Object { $_.Current.Name -ceq $Name -and $_.Current.ControlType -eq $Type -and !$_.Current.IsOffscreen })
    if ($items.Count -gt 1) { throw 'Ambiguous visible scoped control' }
    if ($items.Count -eq 1) { return $items[0] }
}
function Invoke-ExactVisible($Control) {
    $invoke=$null
    if (!$Control -or $Control.Current.IsOffscreen -or !$Control.Current.IsEnabled -or
        !$Control.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$invoke)) { throw 'Unique visible control is not invokable' }
    ([System.Windows.Automation.InvokePattern]$invoke).Invoke()
}
function Get-ActiveFixtureRow([string]$Title) {
    $tree=Find-Control @('Sessions') ([System.Windows.Automation.ControlType]::Tree)
    if (!$tree) { return }
    $projectNode=Find-Scoped $tree (Split-Path -Leaf $fixture.Project) ([System.Windows.Automation.ControlType]::TreeItem)
    if (!$projectNode) { return }
    $result.codexHistory.projectTreeObserved=$true
    $row=Find-Scoped $projectNode $Title ([System.Windows.Automation.ControlType]::TreeItem)
    if ($row -and $row.Current.IsEnabled -and $row.Current.IsKeyboardFocusable) {
        $screenName=Find-Control @($Title) ([System.Windows.Automation.ControlType]::TreeItem)
        if (!$screenName -or ![System.Windows.Automation.AutomationElement]::Compare($row,$screenName)) { throw 'Title is not globally unique in the owned UI' }
        return $row
    }
}
function Open-ExactActiveRowMenu([string]$Title) {
    $row=Wait-Observed { Get-ActiveFixtureRow $Title } 'unique visible non-archived fixture row and exact name'
    if (Find-Control @('Session actions') ([System.Windows.Automation.ControlType]::Menu)) { throw 'A session menu is already open' }
    $row.SetFocus() # Reveals the scoped overflow through the real focus-within style. Does not open a CLI session.
    $more=Wait-Observed { Find-Scoped $row 'Session actions' ([System.Windows.Automation.ControlType]::Button) } 'exact active row overflow action'
    Invoke-ExactVisible $more
    Wait-Observed { Find-Control @('Session actions') ([System.Windows.Automation.ControlType]::Menu) } 'exact visible owned row menu'
}
function Verify-ExactClipboardId($Menu) {
    $copy=Find-Scoped $Menu 'Copy session ID' ([System.Windows.Automation.ControlType]::MenuItem)
    Set-Clipboard -Value 'SyntheticClipboardGuard'
    Invoke-ExactVisible $copy
    $null=Wait-Observed {
        $actual=Get-Clipboard -Raw
        $actual -and $actual.Length -le 128 -and $actual.Trim() -ceq $fixture.SessionId
    } 'same native UUID from the exact visible row'
    $null=Wait-Observed { -not (Find-Control @('Session actions') ([System.Windows.Automation.ControlType]::Menu)) } 'row menu closed after copy'
}

function Assert-OwnedEditorFocus($Editor) {
    $hwnd=[IntPtr]::new($window.Current.NativeWindowHandle)
    if ($window.Current.ProcessId -ne $app.Id -or $hwnd -eq [IntPtr]::Zero -or [OfficialRenameForeground]::GetForegroundWindow() -ne $hwnd) {
        throw 'Original application window does not own keyboard foreground'
    }
    $actual=[System.Windows.Automation.AutomationElement]::FocusedElement
    if (!$actual -or ![System.Windows.Automation.AutomationElement]::Compare($Editor,$actual)) {
        throw 'Unique synthetic title editor lost keyboard focus'
    }
}
function Read-OwnedEditor($Editor) {
    $value=$null
    if (!$Editor.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$value)) { throw 'No exact title readback pattern' }
    return ([System.Windows.Automation.ValuePattern]$value).Current.Value
}
function Test-ExactActiveRename {
    $r=$result.codexHistory
    $r.stage='native-old-name'
    $baseline=& (Join-Path $PSScriptRoot 'pr101-codex-history-fixture.ps1') -Codex $FixtureCodex -ReadOnlyExisting $fixture -ObserveRename -ExpectedTitle $fixture.ActiveCanary
    if (!$baseline.sameId -or !$baseline.sameCwd -or !$baseline.expectedName -or !$baseline.listsExact -or !$baseline.executedPinnedAppServer) {
        throw 'Independent old title, native identity or non-archived membership not established'
    }
    $r.appServerExecutableProven=$true
    $r.nonArchivedNativeRecord=$true
    $r.nativePreflight=$true
    $r.stage='locate-exact-sessions-tree'
    $tree=Wait-Observed { Find-Control @('Sessions') ([System.Windows.Automation.ControlType]::Tree) } 'unique visible workspace Sessions tree'
    $r.sessionsTreeObserved=$true
    $r.stage='scoped-expand'
    $projectName=Split-Path -Leaf $fixture.Project
    $node=Wait-Observed { Find-Scoped $tree $projectName ([System.Windows.Automation.ControlType]::TreeItem) } 'exact fixture project inside the Sessions tree'
    $r.projectTreeObserved=$true
    $expand=Find-Scoped $node 'Expand' ([System.Windows.Automation.ControlType]::Button)
    $collapse=Find-Scoped $node 'Collapse' ([System.Windows.Automation.ControlType]::Button)
    if ($expand -and $collapse) { throw 'Ambiguous fixture project expansion state' }
    if ($expand) {
        Invoke-ExactVisible $expand
        $r.expansionInvoked=$true
    } elseif (!$collapse) { throw 'Unique fixture project has neither exact Expand nor Collapse' }
    $null=Wait-Observed {
        $same=Find-Scoped $tree $projectName ([System.Windows.Automation.ControlType]::TreeItem)
        if ($same) { Find-Scoped $same 'Collapse' ([System.Windows.Automation.ControlType]::Button) }
    } 'expanded fixture project state'
    $r.projectExpanded=$true
    $r.stage='unique-visible-old-row'
    $null=Wait-Observed { Get-ActiveFixtureRow $fixture.ActiveCanary } 'one visible non-archived fixture row under exact project'
    $r.activeRowObserved=$true
    $r.stage='verify-pre-save-uuid'
    Verify-ExactClipboardId (Open-ExactActiveRowMenu $fixture.ActiveCanary)
    $r.activeRowUuidMatched=$true
    $r.stage='invoke-scoped-rename-entry'
    $menu=Open-ExactActiveRowMenu $fixture.ActiveCanary
    Invoke-ExactVisible (Find-Scoped $menu 'Rename' ([System.Windows.Automation.ControlType]::MenuItem))
    $r.stage='verify-unique-old-editor'
    $row=Wait-Observed { Get-ActiveFixtureRow $fixture.ActiveCanary } 'old titled row in edit mode'
    $editType=[System.Windows.Automation.ControlType]::Edit
    $edit=Wait-Observed { Find-Scoped $row 'Session name' $editType } 'visible scoped title editor'
    $whole=Find-Control @('Session name') $editType
    if (!$whole -or ![System.Windows.Automation.AutomationElement]::Compare($edit,$whole) -or
        (Read-OwnedEditor $edit) -cne $fixture.ActiveCanary) { throw 'Old name or unique scoped editor differs' }
    $r.oldEditorMatched=$true
    $r.stage='read-back-edit'
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class OfficialRenameForeground {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
'@
    $newTitle='Pr101RenamedCanary'
    if ($newTitle -cnotmatch '^[A-Za-z0-9]{5,30}$') { throw 'Unsafe synthetic title' }
    $edit.SetFocus()
    Assert-OwnedEditorFocus $edit
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    Assert-OwnedEditorFocus $edit
    [System.Windows.Forms.SendKeys]::SendWait($newTitle)
    $null=Wait-Observed {
        Assert-OwnedEditorFocus $edit
        (Read-OwnedEditor $edit) -ceq $newTitle
    } 'new exact title readback before save'
    $r.inputReadbackMatched=$true
    $r.stage='save-once'
    $save=Find-Scoped $row 'Save name' ([System.Windows.Automation.ControlType]::Button)
    if (!$save -or !$save.Current.IsEnabled -or $r.saveInvocationCount -ne 0) { throw 'Unique unused save action unavailable' }
    Invoke-OneSaveAndObserveProductCodex $save
    $r.stage='refresh-exact-row'
    $null=Wait-Observed { Get-ActiveFixtureRow $newTitle } 'unique new title in same project'
    Invoke-Button @('Refresh sessions')
    $null=Wait-Observed { Find-Control @('Refresh sessions') ([System.Windows.Automation.ControlType]::Button) | Where-Object { $_.Current.IsEnabled } } 'ordinary refresh finished'
    $null=Wait-Observed { Get-ActiveFixtureRow $newTitle } 'unique new title persisted after refresh'
    $r.refreshedTitleMatched=$true
    $r.stage='verify-post-save-uuid'
    Verify-ExactClipboardId (Open-ExactActiveRowMenu $newTitle)
    $r.postRenameUuidMatched=$true
    $r.stage='native-readback'
    $proof=& (Join-Path $PSScriptRoot 'pr101-codex-history-fixture.ps1') -Codex $FixtureCodex -ReadOnlyExisting $fixture -ObserveRename -ExpectedTitle $newTitle
    $r.nativeReadbackMatched=$proof.sameId -and $proof.sameCwd -and $proof.expectedName -and $proof.listsExact -and $proof.executedPinnedAppServer
    if (!$r.nativeReadbackMatched) { throw 'Separate real app-server did not confirm exact ID, CWD and new title' }
    $r.rename='native-name-persisted'
    $r.stage='rename-proof-complete'
}
