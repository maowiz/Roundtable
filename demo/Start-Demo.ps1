# Roundtable demo launcher
# Starts the built prototype on a local port and opens it as a standalone desktop window (Edge app mode).
# Closing the window shuts the local server down again.

$ErrorActionPreference = 'Stop'
$root   = Split-Path -Parent $PSScriptRoot
$app    = Join-Path $root 'ui-prototype'
$port   = 4173
$url    = "http://127.0.0.1:$port/?screen=welcome"
$edge   = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
            "${env:ProgramFiles}\Microsoft\Edge\Application\msedge.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
$profile = Join-Path $env:LOCALAPPDATA 'RoundtableDemoProfile'

if (-not $edge) { Write-Host 'Microsoft Edge was not found. Install Edge or open the URL below in Chrome.'; Write-Host $url; Read-Host 'Press Enter to exit'; exit 1 }
if (-not (Test-Path (Join-Path $app 'dist\index.html'))) {
  Write-Host 'Building the prototype (first run only)...'
  Push-Location $app; npm run build | Out-Null; Pop-Location
}

# Reuse a server that is already running, otherwise start one minimised.
$listening = Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue
$server = $null
if (-not $listening) {
  $server = Start-Process -FilePath 'cmd.exe' -ArgumentList "/c npx vite preview --host 127.0.0.1 --port $port --strictPort" -WorkingDirectory $app -WindowStyle Hidden -PassThru
  $ok = $false
  for ($i = 0; $i -lt 40; $i++) {
    try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:$port/" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}
    Start-Sleep -Milliseconds 500
  }
  if (-not $ok) { Write-Host 'The local server did not start. Run "npm install" inside ui-prototype and try again.'; Read-Host 'Press Enter to exit'; exit 1 }
}

# Open as a desktop window: no tabs, no address bar, its own clean profile.
$args = @("--app=$url", "--user-data-dir=$profile", '--start-maximized',
          '--no-first-run', '--no-default-browser-check', '--disable-sync',
          '--disable-features=msImplicitSignIn,msEdgeShoppingAssistant,msHubApps,msEdgeSidebarV2')
$window = Start-Process -FilePath $edge -ArgumentList $args -PassThru
Write-Host "Roundtable demo is open. Close the window to stop."

# Wait for the demo window to close, then stop the server we started.
Start-Sleep -Seconds 2
while (Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like "*RoundtableDemoProfile*" }) { Start-Sleep -Seconds 1 }
if ($server) {
  Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue |
    Select-Object -ExpandProperty OwningProcess -Unique | ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }
  Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
}
