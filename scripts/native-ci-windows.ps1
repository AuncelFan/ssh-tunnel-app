$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    & cargo build -p ssh-tunnel-core --example host_key_fixture --locked
    if ($LASTEXITCODE -ne 0) { throw 'Cannot build native SSH fixture' }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    $elevated = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    Write-Host "CI launcher elevated: $elevated"
    if (-not $elevated) {
        & node frontend/scripts/native-smoke.mjs --release
        if ($LASTEXITCODE -ne 0) { throw 'Native smoke failed' }
        & node frontend/scripts/native-smoke.mjs --release --implicit-config
        if ($LASTEXITCODE -ne 0) { throw 'Native cutover smoke failed' }
        return
    }

    if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
        throw 'Elevated smoke account isolation is restricted to disposable GitHub-hosted runners'
    }
    # A dedicated standard account also works when the runner has no filtered admin token.
    $name = 'stci' + [guid]::NewGuid().ToString('N').Substring(0, 12)
    $output = Join-Path $root "target\native-ci\$name"
    [void](New-Item -ItemType Directory -Path $output)
    $node = (Get-Command node.exe).Source
    $worker = Join-Path $root 'scripts\native-ci-worker.mjs'
    $password = ConvertTo-SecureString ('Aa1!' + [guid]::NewGuid().ToString('N')) -AsPlainText -Force
    $testUser = New-LocalUser -Name $name -Password $password -Description 'Disposable ssh-tunnel native CI fixture'
    $grants = @()
    $process = $null
    try {
        Add-LocalGroupMember -SID ([Security.Principal.SecurityIdentifier]'S-1-5-32-545') -Member $testUser
        foreach ($grant in @(@($root, 'ReadAndExecute'), @((Join-Path $root 'target'), 'Modify'))) {
            $acl = Get-Acl -LiteralPath $grant[0]
            $rule = New-Object Security.AccessControl.FileSystemAccessRule($testUser.SID, $grant[1], 'ContainerInherit,ObjectInherit', 'None', 'Allow')
            [void]$acl.AddAccessRule($rule)
            Set-Acl -LiteralPath $grant[0] -AclObject $acl
            $grants += @{ path = $grant[0]; rule = $rule }
        }
        $credential = New-Object Management.Automation.PSCredential("$env:COMPUTERNAME\$name", $password)
        $process = Start-Process -FilePath $node -ArgumentList "`"$worker`"", "`"$output`"" -WorkingDirectory $root -Credential $credential -LoadUserProfile -WindowStyle Hidden -PassThru
        if (-not $process.WaitForExit(240000)) { throw 'Standard-user native smoke timed out' }
        $resultPath = Join-Path $output 'result.json'
        if (-not (Test-Path -LiteralPath $resultPath)) { throw 'Standard-user worker did not produce a result' }
        $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
        Get-Content -LiteralPath (Join-Path $output 'worker.log')
        if ($result.exitCode -ne 0) { throw "Standard-user native smoke failed: $($result.error)" }
        if ($result.elevated -ne $false) { throw 'Missing non-elevated worker verification' }
    } finally {
        if ($process -and -not $process.HasExited) { $process.Kill() }
        foreach ($candidate in Get-CimInstance Win32_Process) {
            $owner = Invoke-CimMethod -InputObject $candidate -MethodName GetOwnerSid -ErrorAction SilentlyContinue
            if ($owner.Sid -eq $testUser.SID.Value) { Stop-Process -Id $candidate.ProcessId -Force -ErrorAction SilentlyContinue }
        }
        foreach ($grant in $grants) {
            try {
                $acl = Get-Acl -LiteralPath $grant.path
                $acl.RemoveAccessRuleSpecific($grant.rule)
                Set-Acl -LiteralPath $grant.path -AclObject $acl
            } catch { Write-Warning 'Could not remove a disposable-account workspace ACL' }
        }
        Remove-LocalUser -SID $testUser.SID
    }
} finally {
    Pop-Location
}
