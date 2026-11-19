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
    Reveal-DiagnosticsControl @($Name) $editType
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
    Invoke-Button @('Settings')
    Invoke-Button @('Launch configurations')
    $name='Pr101SyntheticCodexProfile'
    $button=[System.Windows.Automation.ControlType]::Button
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
    Set-FixtureEdit 'Configuration name' $name
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
    $combo=Wait-Observed { Find-Control @('CLI program override') ([System.Windows.Automation.ControlType]::ComboBox) } 'Codex executable override'
    $expand=$null
    if (!$combo.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$expand)) { throw 'Missing executable override disclosure' }
    ([System.Windows.Automation.ExpandCollapsePattern]$expand).Expand()
    $choice=Wait-Observed { Find-Control @('Set') ([System.Windows.Automation.ControlType]::ListItem) } 'exact program override setting'
    $selection=$null
    if (!$choice.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern,[ref]$selection)) { throw 'Missing executable override selection' }
    ([System.Windows.Automation.SelectionItemPattern]$selection).Select()
    Set-FixtureEdit 'Program path' $FixtureCodex
    Reveal-DiagnosticsControl @('Save') $button
    Invoke-Button @('Save')
    $null=Wait-Observed {
        if (Find-Control @('Configuration name') ([System.Windows.Automation.ControlType]::Edit)) { return $null }
        Find-Control @($name) ([System.Windows.Automation.ControlType]::Text)
    } 'closed editor and confirmed saved official Codex profile in GUI'
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
        @($_.env.Keys | Where-Object { $_ -ieq 'CODEX_HOME' }).Count -eq 0
    })
    $projects=@($manifest.registeredProjects.Values)
    $meta=Get-Content -LiteralPath $fixture.Rollout -TotalCount 1 | ConvertFrom-Json -AsHashtable
    $r.selectedProjectExact=@($projects | Where-Object selectedPath -CEQ $fixture.Project).Count -eq 1
    $r.recordedCanonicalExact=@($projects | Where-Object canonicalPath -CEQ $fixture.Project).Count -eq 1
    $r.metaCwdExact=$meta.payload.cwd -ceq $fixture.Project
    $r.profilePinMatches=$matchingProfile.Count -eq 1
    if (!$r.selectedProjectExact -or !$r.metaCwdExact -or !$r.profilePinMatches) { throw 'Stored synthetic project or bound profile does not match exact fixture inputs' }
    $r.stage='active-history-search'
    Set-FixtureEdit 'Search sessions...' $fixture.ActiveCanary
    $row=$null
    try {
        $row=Wait-Observed { Find-Control @($fixture.ActiveCanary) ([System.Windows.Automation.ControlType]::TreeItem) } 'searched synthetic active history row'
    } catch {
        if ($_.Exception.Message -cne 'BLOCKED_UI_OBSERVATION: searched synthetic active history row') { throw }
        $r.archive='active-canary-not-observed'
        $visible=@($window.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) |
            Where-Object { !$_.Current.IsOffscreen -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::TreeItem } |
            Select-Object -First 33)
        $r.activeSearch=[ordered]@{
            visibleTreeItemCountWithinBudget=$visible.Count
            activeCanaryObserved=@($visible | Where-Object { $_.Current.Name -ceq $fixture.ActiveCanary }).Count -eq 1
            registeredProjectVisible=@($visible | Where-Object { $_.Current.Name -ceq (Split-Path $fixture.Project -Leaf) }).Count -eq 1
        }
    }
    if ($row) {
        $r.activeRowObserved=$true
        $basename=Split-Path $fixture.Rollout -Leaf
        $physicalArchive=Join-Path $fixture.Home ('archived_sessions/' + $basename)
        if (!(Test-Path -LiteralPath $fixture.Rollout -PathType Leaf) -or (Test-Path -LiteralPath $physicalArchive)) { throw 'Unexpected fixture state before archive' }
        $r.stage='archive'
        if (Invoke-FixtureTarget $row 'Native archive (including child sessions)' $fixtureButton) {
            $r.archive='sent'
            $null=Wait-Observed {
                !(Test-Path -LiteralPath $fixture.Rollout) -and (Test-Path -LiteralPath $physicalArchive -PathType Leaf)
            } 'Codex native archive changed physical fixture path'
            $r.archive='physical-archive'
        } else {
            $r.archive='visible-entry-unavailable'
        }
    }
    Set-FixtureEdit 'Search sessions...' ''
    $r.stage='restore'
    if (!(Invoke-FixtureTarget $window 'Archived sessions' $fixtureButton)) { throw 'Archived sessions entry unavailable' }
    $archivedRow=Wait-Observed { Find-Control @($fixture.ArchivedCanary) ([System.Windows.Automation.ControlType]::TreeItem) } 'separate synthetic archived row'
    if (!(Test-Path -LiteralPath $fixture.ArchiveRollout -PathType Leaf)) { throw 'Unexpected fixture state before restore' }
    if (!(Invoke-FixtureTarget $archivedRow 'Restore archive' $fixtureButton)) { throw 'Restore archive entry unavailable for fixture' }
    $r.restore='sent'
    $null=Wait-Observed {
        $restored=@(Get-ChildItem -LiteralPath (Join-Path $fixture.Home 'sessions') -Filter "*$($fixture.ArchivedId).jsonl" -File -Recurse)
        if ($restored.Count -gt 1) { throw 'Restored fixture path is ambiguous' }
        !(Test-Path -LiteralPath $fixture.ArchiveRollout) -and $restored.Count -eq 1
    } 'Codex native restore changed physical fixture path'
    $r.restore='physical-restore'
    $r.stage='read-diagnostics'
}
