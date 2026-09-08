# rue backstop artifact: plan open_mgmt_port on fw-win-01 (os windows), instance golden, language powershell. Rendered by rue-render; do not edit.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Continue'
$Root = 'C:\ProgramData\rue'
$Inst = Join-Path (Join-Path $Root 'instances') 'golden'
if (Test-Path (Join-Path $Inst 'fired')) { exit 0 }
$now = [int64][System.DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$due = $false
$dl = Join-Path $Inst 'deadline'
if (Test-Path $dl) { if ($now -ge [int64](Get-Content $dl -Raw).Trim()) { $due = $true } }
if (-not $due) { exit 0 }
$HostLock = $null
for ($i = 0; $i -lt 300 -and -not $HostLock; $i++) { try { $HostLock = [System.IO.File]::Open((Join-Path $Root 'lock'), 'OpenOrCreate', 'ReadWrite', 'None') } catch { Start-Sleep -Seconds 1 } }
if (-not $HostLock) { exit 1 }
function Sha($p) { if (Test-Path $p) { (Get-FileHash -Algorithm SHA256 -Path $p).Hash.ToLower() } else { 'missing' } }
function Recorded($m, $p) { foreach ($l in Get-Content $m) { $f = $l -split ' ', 3; if ($f.Count -eq 3 -and $f[1] -eq $p) { return $f[2] } }; return '' }
function ForeignRegion($p) {
  foreach ($d in Get-ChildItem -Path (Join-Path $Root 'instances') -Directory) {
    $mf = Join-Path $d.FullName 'manifest'
    if ($mf -eq (Join-Path $Inst 'manifest')) { continue }
    if ((Test-Path $mf) -and (Select-String -Path $mf -SimpleMatch -Pattern "region $p " -Quiet)) { return $true }
  }
  return $false
}
function StripRegion($p, $a) {
  if (-not (Test-Path $p)) { return $false }
  $lines = @(Get-Content $p)
  $b = @($lines | Where-Object { $_ -eq "# rue-region $a begin" }).Count
  $e = @($lines | Where-Object { $_ -eq "# rue-region $a end" }).Count
  if ($b -ne 1 -or $e -ne 1) { return $false }
  $out = @(); $skip = $false
  foreach ($l in $lines) {
    if ($l -eq "# rue-region $a begin") { $skip = $true; continue }
    if ($l -eq "# rue-region $a end") { $skip = $false; continue }
    if (-not $skip) { $out += $l }
  }
  Set-Content -Path $p -Value $out
  return $true
}
function RegionSet($p, $a, $c) {
  StripRegion $p $a | Out-Null
  Add-Content -Path $p -Value @("# rue-region $a begin", $c, "# rue-region $a end")
}
function Restore($s, $p) { Copy-Item -Path $s -Destination $p -Force }
function Defer($n) { Add-Content -Path (Join-Path $Inst 'drift') -Value $n }
function Clobbered($n) { Add-Content -Path (Join-Path $Inst 'clobbered') -Value $n }
# step 1: winfw_allow
$M = Join-Path (Join-Path $Inst 'markers') '1'
if (Test-Path $M) {
  $skip = $false
  if ($skip) { Defer 1 } else {
    & powershell.exe -NoProfile -NonInteractive -Command 'Remove-NetFirewallRule -Name rue-mgmt'
    Remove-Item -Force $M
  }
}
New-Item -ItemType File -Path (Join-Path $Inst 'fired') -Force | Out-Null
exit 0
