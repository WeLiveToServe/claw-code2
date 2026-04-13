$envFile = Join-Path $PSScriptRoot "..\.env"
if (-not (Test-Path $envFile)) { $envFile = Join-Path $PSScriptRoot ".env" }
if (Test-Path $envFile) {
    Get-Content $envFile | ForEach-Object {
        if ($_ -match '^([A-Za-z_][A-Za-z0-9_]*)=(.*)$') {
            $val = $Matches[2] -replace '^"(.*)"$','$1'
            [Environment]::SetEnvironmentVariable($Matches[1], $val, "Process")
        }
    }
}
& (Join-Path $PSScriptRoot "rust\target\debug\claw.exe") @args
