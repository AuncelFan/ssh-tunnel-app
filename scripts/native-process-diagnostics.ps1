param([Parameter(Mandatory = $true)][int]$RootProcessId)
$ErrorActionPreference = 'Stop'
$processes = @(Get-CimInstance Win32_Process)
$ids = [System.Collections.Generic.HashSet[int]]::new()
[void]$ids.Add($RootProcessId)
do {
    $added = $false
    foreach ($process in $processes) {
        if ($ids.Contains([int]$process.ParentProcessId)) {
            if ($ids.Add([int]$process.ProcessId)) { $added = $true }
        }
    }
} while ($added)
# Emit only this synthetic fixture application's process tree, not unrelated processes.
$selected = @($processes | Where-Object { $ids.Contains([int]$_.ProcessId) } |
    Select-Object ProcessId, ParentProcessId, Name, SessionId, ExecutablePath, CommandLine)
$networkErrors = @()
$listeners = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue -ErrorVariable networkErrors |
    Where-Object { $ids.Contains([int]$_.OwningProcess) } |
    Select-Object OwningProcess, LocalAddress, LocalPort)
$settings = @()
foreach ($hive in @('HKLM:', 'HKCU:')) {
    foreach ($prefix in @('SOFTWARE', 'SOFTWARE\WOW6432Node')) {
        foreach ($suffix in @('Microsoft\Edge\WebView2', 'Policies\Microsoft\Edge\WebView2')) {
            $base = "$hive\$prefix\$suffix"
            foreach ($name in @('AdditionalBrowserArguments', 'BrowserExecutableFolder', 'UserDataFolder')) {
                $key = Get-Item -LiteralPath "$base\$name" -ErrorAction SilentlyContinue
                if ($key) {
                    foreach ($app in @('*', 'ssh-tunnel.exe', 'app.ssh-tunnel.desktop')) {
                        $value = $key.GetValue($app)
                        if ($null -ne $value) {
                            $settings += @{ key = "$base\$name"; app = $app; empty = ([string]$value).Length -eq 0; containsDebugPort = ([string]$value).Contains('--remote-debugging-port') }
                        }
                    }
                }
            }
        }
    }
    $edge = Get-Item -LiteralPath "$hive\SOFTWARE\Policies\Microsoft\Edge" -ErrorAction SilentlyContinue
    if ($edge) {
        foreach ($name in @('RemoteDebuggingAllowed', 'DeveloperToolsAvailability')) {
            $value = $edge.GetValue($name)
            if ($value -is [int]) { $settings += @{ key = "$hive\SOFTWARE\Policies\Microsoft\Edge"; name = $name; value = $value } }
        }
    }
}
$netstat = @(& netstat.exe -ano -p tcp | Where-Object {
    $fields = $_.Trim() -split '\s+'
    $owner = 0
    $fields.Length -eq 5 -and [int]::TryParse($fields[-1], [ref]$owner) -and $ids.Contains($owner)
})
@{ processes = $selected; listeners = $listeners; networkQueryErrors = @($networkErrors | ForEach-Object { $_.FullyQualifiedErrorId }); netstat = $netstat; webviewSettings = $settings } | ConvertTo-Json -Depth 5
