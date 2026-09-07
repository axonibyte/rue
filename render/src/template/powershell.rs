//! The PowerShell template: Windows. The scheduler entry (Phase 3) invokes
//! it by `-EncodedCommand` or on stdin so execution policy never applies;
//! a host that cannot run PowerShell at all declares `artifact: python`.

use rue_core::model::Drift;

use super::banner;
use crate::actions::{Action, Kind};
use crate::quote::powershell;
use crate::{Context, RenderError};

fn q(step: u32, s: &str) -> Result<String, RenderError> {
    powershell(s).map_err(|inner| RenderError::Unquotable { step, inner })
}

pub fn render(ctx: &Context<'_>) -> Result<String, RenderError> {
    let mut o = String::new();
    o.push_str(&format!("# {}\n", banner(ctx)));
    o.push_str("Set-StrictMode -Version Latest\n$ErrorActionPreference = 'Continue'\n");
    o.push_str(&format!("$Root = {}\n", q(0, &ctx.root)?));
    o.push_str(&format!(
        "$Inst = Join-Path (Join-Path $Root 'instances') {}\n",
        q(0, &ctx.instance.id)?
    ));
    o.push_str("if (Test-Path (Join-Path $Inst 'fired')) { exit 0 }\n");
    o.push_str(
        "$now = [int64][System.DateTimeOffset]::UtcNow.ToUnixTimeSeconds()\n$due = $false\n",
    );
    if ctx.deadline {
        o.push_str("$dl = Join-Path $Inst 'deadline'\nif (Test-Path $dl) { if ($now -ge [int64](Get-Content $dl -Raw).Trim()) { $due = $true } }\n");
    }
    if let Some(hb) = ctx.heartbeat_s {
        o.push_str(&format!("$hb = Join-Path $Inst 'heartbeat'\nif (Test-Path $hb) {{ if (($now - [int64](Get-Content $hb -Raw).Trim()) -gt {hb}) {{ $due = $true }} }} else {{ $due = $true }}\n"));
    }
    o.push_str("if (-not $due) { exit 0 }\n");
    o.push_str(HELPERS);
    for s in &ctx.steps {
        let n = s.n;
        o.push_str(&format!("# step {}: {}\n", n, s.id));
        o.push_str(&format!(
            "$M = Join-Path (Join-Path $Inst 'markers') '{n}'\n"
        ));
        o.push_str("if (Test-Path $M) {\n  $skip = $false\n");
        for f in &s.files {
            if f.kind == Kind::Region {
                continue;
            }
            let p = q(n, &f.path)?;
            match s.drift {
                Drift::Defer => o.push_str(&format!(
                    "  if ((Sha {p}) -ne (Recorded $M {p})) {{ $skip = $true }}\n"
                )),
                Drift::Clobber => o.push_str(&format!(
                    "  if ((Sha {p}) -ne (Recorded $M {p})) {{ Clobbered {n} }}\n"
                )),
            }
        }
        o.push_str(&format!("  if ($skip) {{ Defer {n} }} else {{\n"));
        for a in &s.actions {
            o.push_str("    ");
            match a {
                Action::Remove { path } => o.push_str(&format!(
                    "Remove-Item -Force -ErrorAction SilentlyContinue {}\n",
                    q(n, path)?
                )),
                Action::StripRegion { path, anchor, k } => {
                    let p = q(n, path)?;
                    let snap = format!("(Join-Path $Inst 'snapshots\\{n}\\{k}')");
                    let fallback = match s.drift {
                        Drift::Clobber => format!(
                            "if (ForeignRegion {p}) {{ Defer {n} }} else {{ Restore {snap} {p}; Clobbered {n} }}"
                        ),
                        Drift::Defer => format!("Defer {n}"),
                    };
                    o.push_str(&format!(
                        "if (-not (StripRegion {p} {})) {{ {fallback} }}\n",
                        q(n, anchor)?
                    ));
                }
                Action::RestoreSnapshot { path, k } => o.push_str(&format!(
                    "Restore (Join-Path $Inst 'snapshots\\{n}\\{k}') {}\n",
                    q(n, path)?
                )),
                Action::Run { command } => o.push_str(&format!(
                    "& powershell.exe -NoProfile -NonInteractive -Command {}\n",
                    q(n, command)?
                )),
                Action::Write { path, content } => o.push_str(&format!(
                    "Set-Content -NoNewline -Path {} -Value {}\n",
                    q(n, path)?,
                    q(n, content)?
                )),
                Action::Append { path, line } => o.push_str(&format!(
                    "Add-Content -Path {} -Value {}\n",
                    q(n, path)?,
                    q(n, line)?
                )),
                Action::RegionSet {
                    path,
                    anchor,
                    content,
                } => o.push_str(&format!(
                    "RegionSet {} {} {}\n",
                    q(n, path)?,
                    q(n, anchor)?,
                    q(n, content)?
                )),
                Action::RegionClear { path, anchor } => o.push_str(&format!(
                    "StripRegion {} {} | Out-Null\n",
                    q(n, path)?,
                    q(n, anchor)?
                )),
            }
        }
        o.push_str("    Remove-Item -Force $M\n  }\n}\n");
    }
    o.push_str(
        "New-Item -ItemType File -Path (Join-Path $Inst 'fired') -Force | Out-Null\nexit 0\n",
    );
    Ok(o)
}

const HELPERS: &str = r##"function Sha($p) { if (Test-Path $p) { (Get-FileHash -Algorithm SHA256 -Path $p).Hash.ToLower() } else { 'missing' } }
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
"##;
