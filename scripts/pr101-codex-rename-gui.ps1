# Called only in the temporary installed-package scenario, after fixture/project ownership validation.
function Test-CodexFixtureRename {
    $r=$result.codexHistory
    $editType=[System.Windows.Automation.ControlType]::Edit
    $rowType=[System.Windows.Automation.ControlType]::TreeItem
    $button=[System.Windows.Automation.ControlType]::Button
    $r.stage='rename-find-active-fixture'
    Set-FixtureEdit 'Search sessions...' $fixture.ActiveCanary
    $row=Wait-Observed { Find-Control @($fixture.ActiveCanary) $rowType } 'unique synthetic rename row'
    $r.stage='rename-verify-owned-hover-target'
    if (!$row.Current.IsEnabled -or $row.Current.IsOffscreen -or $window.Current.ProcessId -ne $app.Id) { throw 'Synthetic row/window is not eligible for hover' }
    $rowRect=$row.Current.BoundingRectangle
    $winRect=$window.Current.BoundingRectangle
    $overlap=[System.Windows.Rect]::Intersect($rowRect,$winRect)
    if ($rowRect.IsEmpty -or $winRect.IsEmpty -or $overlap.IsEmpty -or $overlap.Width -lt 2 -or $overlap.Height -lt 2) {
        throw 'No visible intersection for synthetic row and owned app'
    }
    $point=[System.Windows.Point]::new(0,0)
    if (!$row.TryGetClickablePoint([ref]$point)) { throw 'The synthetic row exposes no verified UIA point' }
    $point=[System.Windows.Point]::new([Math]::Round($point.X),[Math]::Round($point.Y))
    if (!$overlap.Contains($point)) { throw 'Verified UIA point is outside the row/window intersection' }
    function Is-OwnedFixtureHit($Point) {
        $hit=[System.Windows.Automation.AutomationElement]::FromPoint($Point)
        $rowSeen=$false
        for($depth=0;$hit -and $depth -lt 32;$depth++) {
            if ([System.Windows.Automation.AutomationElement]::Compare($hit,$row)) { $rowSeen=$true }
            if ([System.Windows.Automation.AutomationElement]::Compare($hit,$window)) { return $rowSeen -and $hit.Current.ProcessId -eq $app.Id }
            $hit=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($hit)
        }
        return $false
    }
    Add-Type -AssemblyName System.Windows.Forms
    if (!(Is-OwnedFixtureHit $point)) { throw 'Hovered point was not covered by the exact synthetic row and app' }
    $r.ownedHoverHitVerified=$true
    # The production CSS uses :hover; never click the row or invoke session launch.
    [System.Windows.Forms.Cursor]::Position=[Drawing.Point]::new([int]$point.X,[int]$point.Y)
    if (!(Is-OwnedFixtureHit $point)) { throw 'Synthetic row is no longer the visible hover target' }
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
    } 'exact synthetic session actions after verified row hover'
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
