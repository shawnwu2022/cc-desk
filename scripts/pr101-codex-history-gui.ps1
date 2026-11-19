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
    $r.stage='project-expand'
    $projectLabel=Split-Path $fixture.Project -Leaf
    $projectNode=Wait-Observed { Find-Control @($projectLabel) ([System.Windows.Automation.ControlType]::TreeItem) } 'synthetic registered project node'
    $r.projectNodeObserved=$true
    $r.projectControl=[ordered]@{
        patterns=@($projectNode.GetSupportedPatterns() | ForEach-Object ProgrammaticName)
        enabled=$projectNode.Current.IsEnabled
        offscreen=$projectNode.Current.IsOffscreen
        disclosureState='not-supported'
        knownChildren=@($projectNode.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.Condition]::TrueCondition) |
            Where-Object { $_.Current.Name -cin @($projectLabel,'Expand','Collapse') } |
            Select-Object -First 8 | ForEach-Object { [ordered]@{
                expectedName=if($_.Current.Name -ceq $projectLabel){'fixture-project'}else{$_.Current.Name}
                type=$_.Current.ControlType.ProgrammaticName
                patterns=@($_.GetSupportedPatterns() | ForEach-Object ProgrammaticName)
            } })
    }
    $disclosure=$null
    if ($projectNode.Current.IsEnabled -and
        $projectNode.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$disclosure)) {
        $disclosure=[System.Windows.Automation.ExpandCollapsePattern]$disclosure
        $r.projectControl.disclosureState=$disclosure.Current.ExpandCollapseState.ToString()
        if ($disclosure.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Collapsed) {
            $disclosure.Expand()
            $r.projectExpandInvoked=$true
        }
    }
    $hasCollapse=@($projectNode.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object {
        $_.Current.ControlType -eq $fixtureButton -and $_.Current.Name -ceq 'Collapse'
    }).Count
    if ($hasCollapse -gt 1) { throw 'Ambiguous synthetic project disclosure' }
    if ($hasCollapse -eq 0 -and !$r.Contains('projectExpandInvoked') -and $r.projectControl.disclosureState -ne 'Expanded') {
        if (!(Invoke-FixtureTarget $projectNode 'Expand' $fixtureButton)) { throw 'Synthetic project expand unavailable' }
        $r.projectExpandInvoked=$true
    }
    $r.stage='active-history'
    $row=Wait-Observed { Find-Control @($fixture.ActiveCanary) ([System.Windows.Automation.ControlType]::TreeItem) } 'synthetic active history row'
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
