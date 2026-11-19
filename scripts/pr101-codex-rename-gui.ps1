# Called only in the temporary installed-package scenario, after fixture/project ownership validation.
function Test-CodexFixtureRename {
    $r=$result.codexHistory
    $editType=[System.Windows.Automation.ControlType]::Edit
    $rowType=[System.Windows.Automation.ControlType]::TreeItem
    $button=[System.Windows.Automation.ControlType]::Button
    $r.stage='rename-find-active-fixture'
    Set-FixtureEdit 'Search sessions...' $fixture.ActiveCanary
    $row=Wait-Observed { Find-Control @($fixture.ActiveCanary) $rowType } 'unique synthetic rename row'
    $r.stage='rename-focus-fixture-row'
    if (!$row.Current.IsEnabled -or !$row.Current.IsKeyboardFocusable -or $window.Current.ProcessId -ne $app.Id) { throw 'The exact synthetic row is not focusable' }
    # CSS only exposes this particular action on hover/focus-within. Focus does not select or resume the row.
    $row.SetFocus()
    $null=Wait-Observed {
        $focus=[System.Windows.Automation.AutomationElement]::FocusedElement
        if ($focus -and [System.Windows.Automation.AutomationElement]::Compare($focus,$row)) { return $true }
    } 'focus remains on the exact synthetic history row'
    $r.stage='rename-open-fixture-actions'
    $trigger=Wait-Observed {
        $targets=@($row.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition) | Where-Object {
            $_.Current.Name -ceq 'Session actions' -and $_.Current.ControlType -eq $button
        })
        if ($targets.Count -gt 1) { throw 'Ambiguous synthetic action control' }
        if ($targets.Count -eq 1) {
            $candidate=$targets[0]
            $r.renameActionControl=[ordered]@{ enabled=$candidate.Current.IsEnabled; offscreen=$candidate.Current.IsOffscreen; patterns=@($candidate.GetSupportedPatterns() | ForEach-Object ProgrammaticName) }
            if ($candidate.Current.IsEnabled -and !$candidate.Current.IsOffscreen) { return $candidate }
        }
    } 'exact synthetic session actions after verified row focus'
    $pattern=$null
    if ($trigger.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)) {
        ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
    } elseif ($trigger.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$pattern)) {
        $pattern=[System.Windows.Automation.ExpandCollapsePattern]$pattern
        if ($pattern.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Collapsed) { $pattern.Expand() }
    } else { throw 'No supported synthetic menu action observed' }
    $r.stage='rename-observe-fixture-menu'
    $menuType=[System.Windows.Automation.ControlType]::Menu
    $itemType=[System.Windows.Automation.ControlType]::MenuItem
    $menu=Wait-Observed { Find-Control @('Session actions') $menuType } 'unique synthetic session menu'
    $r.stage='rename-select-entry'
    if (!(Invoke-FixtureTarget $menu 'Rename' $itemType)) { throw 'Fixture rename menu is unavailable' }

    # The edited value is synthetic, never used to start a process or issue commands.
    $r.stage='rename-set-synthetic-title'
    Set-FixtureEdit 'Session name' 'Pr101RenamedCanary'
    $row=Wait-Observed { Find-Control @($fixture.ActiveCanary) $rowType } 'unchanged synthetic session identity before saving'
    $r.stage='rename-submit-once'
    if (!(Invoke-FixtureTarget $row 'Save name' $button)) { throw 'Fixture rename save unavailable' }
    $r.rename='sent'
    $r.stage='rename-project-and-source'
    $titleObserved=$false
    try {
        Set-FixtureEdit 'Search sessions...' 'Pr101RenamedCanary'
        $null=Wait-Observed { Find-Control @('Pr101RenamedCanary') $rowType } 'synthetic renamed title in session list'
        $titleObserved=$true
    } catch { }
    $r.renamedTitleObserved=$titleObserved
    $r.readonlyPostRename=[ordered]@{observed=$false}
    try {
        $proof=& (Join-Path $PSScriptRoot 'pr101-codex-history-fixture.ps1') -Codex $FixtureCodex -ReadOnlyExisting $fixture -ObserveRename
        $r.readonlyPostRename=[ordered]@{observed=$true; sameId=$proof.sameId; sameCwd=$proof.sameCwd; expectedName=$proof.expectedName}
        if ($proof.sameId -and $proof.sameCwd -and $proof.expectedName) { $r.rename='native-name-observed' }
    } catch { }
}
