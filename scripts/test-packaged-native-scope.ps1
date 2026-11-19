# Verification-branch helper. Called only inside test-desktop-webview.ps1 hosted guard.
# Uses real UIA controls. No WebView script, IPC injection, terminal input or account.
function Set-ObservedValue([string[]]$Names, [string]$Value) {
    $control = Wait-Observed { Find-Control $Names ([System.Windows.Automation.ControlType]::Edit) } ($Names -join '/')
    $pattern = $null
    if (!$control.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) { throw 'BLOCKED_UI_VALUE' }
    ([System.Windows.Automation.ValuePattern]$pattern).SetValue($Value)
}
function Invoke-ObservedControl([string[]]$Names) {
    $control = Wait-Observed { Find-Control $Names $null } ($Names -join '/')
    $pattern = $null
    if ($control.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        ([System.Windows.Automation.InvokePattern]$pattern).Invoke(); return
    }
    if ($control.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$pattern)) {
        ([System.Windows.Automation.ExpandCollapsePattern]$pattern).Expand(); return
    }
    throw 'BLOCKED_UI_ACTION'
}

function Select-ObservedOption([string[]]$Names, [string[]]$Options) {
    $combo = Wait-Observed { Find-Control $Names ([System.Windows.Automation.ControlType]::ComboBox) } ($Names -join '/')
    $expand = $null
    if (!$combo.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$expand)) { throw 'BLOCKED_UI_SELECTOR' }
    ([System.Windows.Automation.ExpandCollapsePattern]$expand).Expand()
    $option = Wait-Observed { Find-Control $Options ([System.Windows.Automation.ControlType]::ListItem) } ($Options -join '/')
    $select = $null
    if (!$option.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$select)) { throw 'BLOCKED_UI_SELECTION' }
    ([System.Windows.Automation.SelectionItemPattern]$select).Select()
}
function Read-ObservedValue([string[]]$Names) {
    $control = Wait-Observed { Find-Control $Names ([System.Windows.Automation.ControlType]::Edit) } ($Names -join '/')
    $pattern = $null
    if (!$control.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) { throw 'BLOCKED_UI_VALUE_READ' }
    return ([System.Windows.Automation.ValuePattern]$pattern).Current.Value
}


function Invoke-WorkspaceHeader([string[]]$Names) {
    # Source header has the unique collapse-sessions sibling; excludes empty-state/sidebar duplicates.
    $control = Wait-Observed {
        $all = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition))
        $found = @($all | Where-Object {
            if ($_.Current.IsOffscreen -or $_.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button -or $_.Current.Name -cnotin $Names) { return $false }
            $parent = [System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($_)
            if (!$parent) { return $false }
            $siblings = @($parent.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition))
            return @($siblings | Where-Object { $_.Current.Name -cin @('Collapse sessions panel','收起会话栏') }).Count -eq 1
        })
        if ($found.Count -gt 1) { throw 'Ambiguous workspace header action' }
        if ($found.Count -eq 1) { return $found[0] }
        return $null
    } 'workspace header action'
    $pattern = $null
    if (!$control.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)) { throw 'BLOCKED_WORKSPACE_HEADER_INVOKE' }
    ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
}

function Test-PackagedNativeScope {
    $result.nativeStage = 'add-empty-project'
    Invoke-Button @('Workspace', '工作区')
    Invoke-WorkspaceHeader @('Add Project', '添加项目')
    $mainWindow = $window
    try {
        $window = Wait-Observed {
            $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $app.Id)
            $all = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
            $dialogs = @($all | Where-Object { !$_.Current.IsOffscreen -and $_.Current.NativeWindowHandle -ne $mainWindow.Current.NativeWindowHandle })
            if ($dialogs.Count -gt 1) { throw 'Ambiguous project picker' }
            if ($dialogs.Count -eq 1) { return $dialogs[0] }
            return $null
        } 'owned project directory picker'
        Set-ObservedValue @('Folder:', '文件夹:') $nativeProject
        Invoke-Button @('Select Folder', 'Select folder', '选择文件夹')
    } finally { $window = $mainWindow }
    $result.nativeStage = 'safe-new-session-dialog'
    Invoke-WorkspaceHeader @('New Session', '新会话')
    Invoke-ObservedControl @('More options…', '更多选项…')
    if ((Read-ObservedValue @('Project', '项目')) -ine $nativeProject) { throw 'GUI project path mismatch' }
    Select-ObservedOption @('Tool', '工具') @('Codex CLI')
    Select-ObservedOption @('Start mode', '启动方式') @('New Session', '新会话')
    Invoke-ObservedControl @('Developer options', '开发者选项')
    $raw = Wait-Observed { Find-Control @('Use raw arguments', '使用原始参数') ([System.Windows.Automation.ControlType]::CheckBox) } 'raw argument opt-in'
    $toggle = $null
    if (!$raw.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$toggle)) { throw 'BLOCKED_UI_RAW_TOGGLE' }
    $toggle = [System.Windows.Automation.TogglePattern]$toggle
    if ($toggle.Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off) { throw 'Raw mode was not default off' }
    $toggle.Toggle()
    Select-ObservedOption @('Argument format', '编辑方式') @('Raw JSON array', '原始 JSON 数组')
    $safeArgv = '["--sandbox","read-only","--ask-for-approval","on-request"]'
    Set-ObservedValue @('Raw arguments', '原始参数') $safeArgv
    if ((Read-ObservedValue @('Raw arguments', '原始参数')) -cne $safeArgv) { throw 'GUI safe argument readback mismatch' }
    $result.nativeStage = 'create-actual-run'
    Invoke-Button @('Create session', '创建会话')
    $ready = Wait-Observed {
        $program = Find-Control @('Detected program', '检测到的程序') ([System.Windows.Automation.ControlType]::ComboBox)
        if ($program) { return 'discovery' }
        $hostButton = Find-Control @('单次远程连接') ([System.Windows.Automation.ControlType]::Button)
        if ($hostButton -and $hostButton.Current.IsEnabled) { return 'native' }
        return $null
    } 'real program discovery or Native run'
    if ($ready -eq 'discovery') {
        Select-ObservedOption @('Detected program', '检测到的程序') @($NativeCodex)
        Invoke-Button @('Use and start session', '使用并启动会话')
    }

    # Wait for the ordinary Native run; never type a prompt/status/login into its terminal.
    Invoke-Button @('单次远程连接')
    $result.nativeStage = 'read-public-scope'
    Invoke-Button @('读取公开会话范围')
    $scope = Wait-Observed {
        foreach ($code in @('REMOTE_HOST_UNAVAILABLE','NATIVE_CANONICAL_UNAVAILABLE','RUN_NOT_READY','FORBIDDEN','DOCUMENT_BRIDGE_UNAVAILABLE')) {
            $fixed = '未能读取公开会话范围（' + $code + '），不会自动重试。'
            if (Find-Control @($fixed) ([System.Windows.Automation.ControlType]::Text)) {
                $result.nativeScopeError = $code
                throw 'PACKAGED_SCOPE_REJECTED'
            }
        }

        $output = Find-Control @('公开会话范围') $null
        if (!$output) { return $null }
        $pattern = $null
        $text = $null
        if ($output.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern, [ref]$pattern)) {
            $text = ([System.Windows.Automation.TextPattern]$pattern).DocumentRange.GetText(4096)
        } else {
            $leaves = @($output.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Text)))
            if ($leaves.Count -gt 10) { throw 'Public scope output exceeds bound' }
            $text = ($leaves | ForEach-Object { $_.Current.Name }) -join ''
        }
        if (!$text) { return $null }
        if ($text.Length -gt 4096) { throw 'Public scope output exceeds bound' }
        $candidate = $text | ConvertFrom-Json
        $keys = @($candidate.PSObject.Properties.Name | Sort-Object)
        if (($keys -join ',') -cne 'generation,run,session' -or !$candidate.session -or !$candidate.run -or !$candidate.generation) { throw 'Invalid public scope shape' }
        if ($candidate.session -isnot [string] -or $candidate.session -cnotmatch '^[a-f0-9]{32}$' -or
            $candidate.run -isnot [string] -or !$candidate.run.Trim() -or
            $candidate.generation -isnot [string] -or $candidate.generation -cnotmatch '^[1-9][0-9]*$') { throw 'Invalid public scope values' }
        return $candidate
    } 'actual labelled public scope'

    $null = Wait-Observed { Find-Control @('尚无批准') ([System.Windows.Automation.ControlType]::Text) } 'Host no approval'
    foreach ($name in @('请求本机批准', '启动一次')) {
        $button = Wait-Observed { Find-Control @($name) ([System.Windows.Automation.ControlType]::Button) } $name
        if ($button.Current.IsEnabled) { throw 'Host authority unexpectedly available' }
    }
    Observe-Children
    $codexChildren = @($children.Values | Where-Object { $_.ProcessName -eq 'codex' })
    if ($codexChildren.Count -ne 1 -or $codexChildren[0].MainModule.FileName -ine $NativeCodex) { throw 'Exact real Codex child not observed' }
    if ((Get-FileHash -LiteralPath $codexChildren[0].MainModule.FileName -Algorithm SHA256).Hash -ine $codexSha) { throw 'Running Codex digest differs' }
    $result.nativeStage = 'scope-and-locked-observed'
    $result.native = [ordered]@{
        scope = $scope; hostNoApproval = $true; requestDisabled = $true; startDisabled = $true
        codexPid = $codexChildren[0].Id; codexSha256 = $codexSha; account = 'fresh empty CODEX_HOME'
        taskSent = $false; terminalInput = $false; publicRelayTested = $false; allDescendantsRetired = 'not proven'
    }
}
