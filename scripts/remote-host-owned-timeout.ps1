# 只终止本运行器创建的确切进程；不猜测或终止其他进程树。
function Stop-OwnedTimedOut([System.Diagnostics.Process]$Process) {
    try {
        if (-not $Process.HasExited) { $Process.Kill() }
        return $Process.WaitForExit(5000)
    } catch {
        return $false
    }
}

# 进程尚未终结时不得访问 ExitCode，先保存未知回收结果再中止后续实验。
function Get-OwnedExitCode($Process) {
    if ($Process.HasExited) { return $Process.ExitCode }
    return $null
}
