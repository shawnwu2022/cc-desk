# Called only in the temporary installed-package scenario, after fixture/project ownership validation.
function Test-CodexFixtureRename {
    $r=$result.codexHistory
    $editType=[System.Windows.Automation.ControlType]::Edit
    $rowType=[System.Windows.Automation.ControlType]::TreeItem
    $button=[System.Windows.Automation.ControlType]::Button
    $r.stage='rename-find-active-fixture'
    Set-FixtureEdit 'Search sessions...' $fixture.ActiveCanary
    $row=Wait-Observed { Find-Control @($fixture.ActiveCanary) $rowType } 'unique synthetic rename row'
    $r.stage='rename-open-fixture-actions'
    if (!(Invoke-FixtureTarget $row 'Session actions' $button)) { throw 'Fixture session actions are unavailable' }
    $menuType=[System.Windows.Automation.ControlType]::Menu
    $itemType=[System.Windows.Automation.ControlType]::MenuItem
    $menu=Wait-Observed { Find-Control @('Session actions') $menuType } 'unique synthetic session menu'
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
