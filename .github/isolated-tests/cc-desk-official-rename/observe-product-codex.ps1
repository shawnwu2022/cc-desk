# Bound observation of a directly spawned process, not the separately started fixture.
function Assert-ExactFixtureConfig {
    $config=Join-Path $fixture.Home 'config.toml'
    if ((Get-FileHash -LiteralPath $config).Hash -ine '77716d232bafa5ba15aaec3838433b05c540e96b1fa53d81934eab5ab7ce9f94' -or
        (Test-Path -LiteralPath (Join-Path $fixture.Home 'auth.json')) -or
        @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object LocalPort -eq 1).Count) { throw 'SYNTHETIC_CONFIG_DRIFT' }
}
function Invoke-OneSaveAndObserveProductCodex($Save) {
    Assert-ExactFixtureConfig
    if (!$result.codexHistory.profilePinMatches -or !$result.codexHistory.productCodeHomeInputBound -or
        $env:CODEX_HOME -cne $fixture.Home -or $app.HasExited -or $result.codexHistory.saveInvocationCount -ne 0) { throw 'PRODUCT_LAUNCH_CONTEXT_UNBOUND' }
    $ready=[Threading.ManualResetEventSlim]::new($false)
    $clock=[hashtable]::Synchronized(@{ save=$null })
    $worker=[PowerShell]::Create()
    $async=$null
    try {
        $observer={
            param($Owner,$Expected,$Ready,$Clock)
            $ErrorActionPreference='Stop'
            $sid='ccdesk-rename-codex-'+[guid]::NewGuid().ToString('N')
            $registered=$false
            $candidate=$null
            try {
                $query="SELECT * FROM Win32_ProcessStartTrace WHERE ParentProcessID = $($Owner.Id) AND ProcessName = 'codex.exe'"
                $null=Register-CimIndicationEvent -Query $query -SourceIdentifier $sid
                $registered=$true
                $Ready.Set()
                $event=Wait-Event -SourceIdentifier $sid -Timeout 8
                if (!$event) { return $false }
                $at=$Clock.save
                if ($null -eq $at) { return $false }
                $data=$event.SourceEventArgs.NewEvent
                $eventTime=[DateTime]::FromFileTimeUtc([long]$data.TIME_CREATED)
                if ($eventTime -lt $at -or [int]$data.ParentProcessID -ne $Owner.Id -or [int]$data.SessionID -ne $Owner.SessionId -or $Owner.HasExited) { return $false }
                $candidate=[Diagnostics.Process]::GetProcessById([int]$data.ProcessID)
                $null=$candidate.Handle
                if ($candidate.HasExited -or $candidate.SessionId -ne $Owner.SessionId -or
                    $candidate.StartTime.ToUniversalTime() -lt $at -or $candidate.StartTime.ToUniversalTime() -gt $eventTime -or
                    $candidate.MainModule.FileName -ine $Expected) { return $false }
                # The candidate path is held and digest/length verified by the wrapper.
                # The actual product can terminate the metadata child after reading.
                Remove-Event -EventIdentifier $event.EventIdentifier
                if (!$candidate.WaitForExit(5000)) { return $false }
                if (Wait-Event -SourceIdentifier $sid -Timeout 1) { return $false }
                return $true
            } catch {
                return $false
            } finally {
                if ($candidate) { $candidate.Dispose() }
                if ($registered) {
                    Unregister-Event -SourceIdentifier $sid -ErrorAction SilentlyContinue
                    Get-Event -SourceIdentifier $sid -ErrorAction SilentlyContinue | Remove-Event -ErrorAction SilentlyContinue
                }
            }
        }
        $null=$worker.AddScript($observer.ToString()).AddArgument($app).AddArgument($FixtureCodex).AddArgument($ready).AddArgument($clock)
        $async=$worker.BeginInvoke()
        if (!$ready.Wait(3000) -or $async.IsCompleted) { throw 'PRODUCT_OBSERVER_NOT_READY' }
        $result.codexHistory.productObserverArmed=$true
        $clock.save=[DateTime]::UtcNow
        $result.codexHistory.saveInvocationCount++
        $result.codexHistory.rename='save-attempted'
        Invoke-ExactVisible $Save
        $observerCompleted=$async.AsyncWaitHandle.WaitOne(14000)
        if ($observerCompleted) {
            $proof=@($worker.EndInvoke($async))
            if ($proof.Count -eq 1 -and $proof[0].PSObject.BaseObject -is [bool] -and $proof[0].PSObject.BaseObject) {
                $result.codexHistory.productCodexChildImageObserved=$true
                $result.codexHistory.productCodexPinnedVersion='0.169.1'
            }
        }
        Assert-ExactFixtureConfig
        # Missing process evidence stays unknown. GUI/native reads continue, but
        # the enclosing gate will not pass without observed product process evidence.
    } finally {
        if ($async -and !$async.IsCompleted) { $worker.Stop() }
        $worker.Dispose()
        $ready.Dispose()
    }
}
