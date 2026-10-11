# Read-only check of every enforcement layer on the real machine. Changes nothing.
# Run it while Porteiro is locked (and again while unlocked: every line should flip to ABSENT).
#   powershell -NoProfile -File scripts\verify-enforcement.ps1
$ErrorActionPreference = 'Continue'
$results = @()
function Check($name, [bool]$ok, $detail) { $script:results += [pscustomobject]@{ Layer = $name; State = $(if ($ok) { 'PRESENT' } else { 'ABSENT' }); Detail = $detail } }

$hostsPath = Join-Path $env:SystemRoot 'System32\drivers\etc\hosts'
$hosts = Get-Content $hostsPath -ErrorAction SilentlyContinue
$entries = @($hosts | Where-Object { $_ -match '^0\.0\.0\.0\s+\S+' }).Count
Check 'hosts block' (($hosts -contains '# >>> porteiro >>>') -and ($hosts -contains '# <<< porteiro <<<')) "$entries IPv4 entries between the markers"

foreach ($site in 'www.instagram.com', 'web.whatsapp.com', 'x.com', 'www.facebook.com') {
  $ips = try { [System.Net.Dns]::GetHostAddresses($site) | ForEach-Object { $_.ToString() } } catch { @('(lookup failed)') }
  $blockedLookup = ($ips | Where-Object { $_ -notin '0.0.0.0', '::', '(lookup failed)' }).Count -eq 0
  Check "system resolver: $site" $blockedLookup ($ips -join ', ')
}

$policyKeys = @(
  'HKLM:\SOFTWARE\Policies\Google\Chrome\URLBlocklist',
  'HKLM:\SOFTWARE\Policies\Microsoft\Edge\URLBlocklist',
  'HKLM:\SOFTWARE\Policies\BraveSoftware\Brave\URLBlocklist',
  'HKLM:\SOFTWARE\Policies\Mozilla\Firefox\WebsiteFilter\Block')
foreach ($key in $policyKeys) {
  $count = if (Test-Path $key) { (Get-Item $key).ValueCount } else { 0 }
  Check "browser policy $($key -replace '^HKLM:\\SOFTWARE\\Policies\\', '')" ($count -gt 0) "$count entries"
}
$doh = (Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Mozilla\Firefox\DNSOverHTTPS' -ErrorAction SilentlyContinue)
Check 'Firefox DoH disabled+locked' ($doh -and $doh.Enabled -eq 0 -and $doh.Locked -eq 1) "Enabled=$($doh.Enabled) Locked=$($doh.Locked)"

$rules = @(Get-NetFirewallRule -DisplayName 'Porteiro-*' -ErrorAction SilentlyContinue)
Check 'firewall rules' ($rules.Count -gt 0) "$($rules.Count) outbound block rules (Porteiro-N)"

$task = Get-ScheduledTask -TaskName 'Porteiro' -ErrorAction SilentlyContinue
Check 'restart task' ($null -ne $task) $(if ($task) { "state $($task.State), run level $($task.Principal.RunLevel)" } else { 'not registered (debug builds never register it)' })

$proc = Get-Process porteiro -ErrorAction SilentlyContinue
Check 'process running' ($null -ne $proc) $(if ($proc) { "pid $($proc.Id -join ',')" } else { 'not running' })

$results | Format-Table -AutoSize -Wrap
