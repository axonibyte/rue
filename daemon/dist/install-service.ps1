# Register rued with the Windows service-control manager.
#
# docs/ROADMAP.md 7.9 and 12. The service runs as LocalSystem, keeps its
# store under %ProgramData%\rue, and serves the control channel on the
# named pipe \\.\pipe\rue, whose access-control list names the group the
# -Group argument gives (docs/control-protocol.md).
#
# Not run on any machine this phase: Windows is proven under wine only
# (the Phase 3 acceptance amendment), and this file is the shape a real
# installation takes, checked by eye and by shellcheck's PowerShell
# equivalent nowhere. Phase 3W runs it.
#
#   powershell -ExecutionPolicy Bypass -File install-service.ps1 `
#       -Site C:\ProgramData\rue\site.rue -Group rue-operators

param(
    [Parameter(Mandatory = $true)][string]$Site,
    [string]$Store = "$env:ProgramData\rue\store",
    [string]$Pipe = "\\.\pipe\rue",
    [string]$Group = "rue",
    [string]$Exe = "$env:ProgramFiles\rue\rued.exe",
    [string]$Name = "rue"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Exe)) {
    throw "rued.exe is not at $Exe; install it there or pass -Exe"
}
if (-not (Test-Path $Site)) {
    throw "the site file is not at $Site"
}
New-Item -ItemType Directory -Force -Path $Store | Out-Null

# One string, because sc.exe takes the whole command line as binPath.
$bin = '"{0}" service --site "{1}" --store "{2}" --socket "{3}" --group "{4}"' -f `
    $Exe, $Site, $Store, $Pipe, $Group

& sc.exe create $Name binPath= $bin start= auto obj= LocalSystem DisplayName= "rue engine"
if ($LASTEXITCODE -ne 0) { throw "sc.exe create failed with $LASTEXITCODE" }
& sc.exe description $Name "Applies and reverts rue plans; serves the control channel on $Pipe"
& sc.exe start $Name
if ($LASTEXITCODE -ne 0) { throw "sc.exe start failed with $LASTEXITCODE" }

Write-Output "rue is registered and running; rue.exe --socket $Pipe status"
