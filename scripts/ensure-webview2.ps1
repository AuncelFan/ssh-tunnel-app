$ErrorActionPreference = 'Stop'
Write-Host "Windows session: $((Get-Process -Id $PID).SessionId); user-interactive: $([Environment]::UserInteractive)"

function Find-WebViewRuntime {
    $roots = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Microsoft\EdgeWebView\Application'),
        (Join-Path $env:LOCALAPPDATA 'Microsoft\EdgeWebView\Application')
    )
    foreach ($root in $roots) {
        if (Test-Path -LiteralPath $root) {
            $runtime = Get-ChildItem -LiteralPath $root -Directory |
                Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
                Sort-Object { [version]$_.Name } -Descending |
                Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'msedgewebview2.exe') } |
                Select-Object -First 1
            if ($runtime) { return $runtime.FullName }
        }
    }
    return $null
}

$runtime = Find-WebViewRuntime
if (-not $runtime) {
    $installer = Join-Path $env:TEMP ('webview2-bootstrapper-' + [guid]::NewGuid().ToString('N') + '.exe')
    Invoke-WebRequest -Uri 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $installer
    $signature = Get-AuthenticodeSignature -LiteralPath $installer
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
        throw 'WebView2 bootstrapper does not have a valid Microsoft signature'
    }
    $process = Start-Process -FilePath $installer -ArgumentList '/silent', '/install' -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "WebView2 installer failed: $($process.ExitCode)" }
    $runtime = Find-WebViewRuntime
    if (-not $runtime) { throw 'WebView2 runtime is absent after installation' }
}

Write-Host "Verified WebView2 runtime: $runtime"
if ($env:GITHUB_ENV) {
    "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER=$runtime" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
}
