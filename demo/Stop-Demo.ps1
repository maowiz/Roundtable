# Stops the Roundtable demo window and its local server.
Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" |
  Where-Object { $_.CommandLine -like '*RoundtableDemoProfile*' } |
  ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Get-NetTCPConnection -LocalPort 4173 -State Listen -ErrorAction SilentlyContinue |
  Select-Object -ExpandProperty OwningProcess -Unique |
  ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }
Write-Host 'Roundtable demo stopped.'
