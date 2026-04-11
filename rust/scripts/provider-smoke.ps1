param(
    [string]$EnvFile,
    [switch]$StructuralOnly,
    [switch]$LiveOnly,
    [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-DefaultEnvFile {
    $candidates = @(
        (Join-Path $PSScriptRoot "..\.env"),
        (Join-Path $PSScriptRoot "..\..\.env"),
        (Join-Path $PSScriptRoot "..\..\..\.env")
    )

    foreach ($candidate in $candidates) {
        $full = [System.IO.Path]::GetFullPath($candidate)
        if (Test-Path -LiteralPath $full) {
            return $full
        }
    }

    return $null
}

function Import-DotEnv {
    param([string]$Path)

    if (-not $Path -or -not (Test-Path -LiteralPath $Path)) {
        return
    }

    foreach ($rawLine in Get-Content -LiteralPath $Path) {
        $line = $rawLine.Trim()
        if (-not $line -or $line.StartsWith("#") -or -not $line.Contains("=")) {
            continue
        }

        $parts = $line.Split("=", 2)
        $key = $parts[0].Trim()
        if ($key.StartsWith("export ")) {
            $key = $key.Substring(7).Trim()
        }
        if (-not $key) {
            continue
        }

        $value = $parts[1].Trim()
        if ($value.Length -ge 2) {
            $doubleQuoted = $value.StartsWith('"') -and $value.EndsWith('"')
            $singleQuoted = $value.StartsWith("'") -and $value.EndsWith("'")
            if ($doubleQuoted -or $singleQuoted) {
                $value = $value.Substring(1, $value.Length - 2)
            }
        }

        [System.Environment]::SetEnvironmentVariable($key, $value)
    }
}

function Invoke-Step {
    param(
        [string]$Name,
        [scriptblock]$Action
    )

    $start = Get-Date
    try {
        $null = & $Action
        $duration = ((Get-Date) - $start).TotalSeconds
        return [pscustomobject]@{
            Name = $Name
            Status = "PASS"
            Seconds = [Math]::Round($duration, 2)
            Detail = ""
        }
    } catch {
        $duration = ((Get-Date) - $start).TotalSeconds
        return [pscustomobject]@{
            Name = $Name
            Status = "FAIL"
            Seconds = [Math]::Round($duration, 2)
            Detail = $_.Exception.Message
        }
    }
}

function Invoke-CommandChecked {
    param(
        [string]$FilePath,
        [string[]]$ArgumentList
    )

    & $FilePath @ArgumentList
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($ArgumentList -join ' ')"
    }
}

function Invoke-ClawPrompt {
    param(
        [string]$ClawPath,
        [string]$Model,
        [hashtable]$EnvMap = @{}
    )

    $previousValues = @{}
    foreach ($key in $EnvMap.Keys) {
        $previousValues[$key] = [System.Environment]::GetEnvironmentVariable($key)
        [System.Environment]::SetEnvironmentVariable($key, $EnvMap[$key])
    }

    try {
        $output = & $ClawPath --model $Model --compact "Reply with exactly OK"
        if ($LASTEXITCODE -ne 0) {
            throw "claw prompt failed for model $Model with exit code ${LASTEXITCODE}"
        }

        $text = ($output | Out-String).Trim()
        if ($text -ne "OK") {
            throw "expected exact OK response for $Model, got: $text"
        }
    } finally {
        foreach ($key in $EnvMap.Keys) {
            $previousValue = $previousValues[$key]
            if ($null -ne $previousValue) {
                [System.Environment]::SetEnvironmentVariable($key, $previousValue)
            } else {
                [System.Environment]::SetEnvironmentVariable($key, $null)
            }
        }
    }
}

$repoRustDir = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$clawExe = Join-Path $repoRustDir "target\debug\claw.exe"

if (-not $EnvFile) {
    $EnvFile = Resolve-DefaultEnvFile
}

if ($EnvFile) {
    Import-DotEnv -Path $EnvFile
}

$results = New-Object System.Collections.Generic.List[object]

if (-not $LiveOnly) {
    $structuralSteps = @(
        @{
            Name = "api-routing-openai-prefix"
            Action = { Invoke-CommandChecked -FilePath "cargo" -ArgumentList @("test", "-p", "api", "--lib", "openai_namespaced_model_routes_to_openai_not_anthropic") }
        },
        @{
            Name = "api-routing-gemini-prefix"
            Action = { Invoke-CommandChecked -FilePath "cargo" -ArgumentList @("test", "-p", "api", "--lib", "gemini_prefix_routes_to_openai_not_anthropic") }
        },
        @{
            Name = "api-routing-qwen-prefix"
            Action = { Invoke-CommandChecked -FilePath "cargo" -ArgumentList @("test", "-p", "api", "--lib", "qwen_prefix_routes_to_dashscope_not_anthropic") }
        },
        @{
            Name = "api-openai-compat-config-mapping"
            Action = { Invoke-CommandChecked -FilePath "cargo" -ArgumentList @("test", "-p", "api", "--lib", "metadata_builds_matching_openai_compatible_transport_configs") }
        },
        @{
            Name = "cli-model-precedence"
            Action = { Invoke-CommandChecked -FilePath "cargo" -ArgumentList @("test", "-p", "rusty-claude-cli", "--bin", "claw", "resolve_repl_model_") }
        },
        @{
            Name = "cli-provider-labels"
            Action = { Invoke-CommandChecked -FilePath "cargo" -ArgumentList @("test", "-p", "rusty-claude-cli", "--bin", "claw", "format_connected_line_") }
        },
        @{
            Name = "cli-gemini-auth-health"
            Action = { Invoke-CommandChecked -FilePath "cargo" -ArgumentList @("test", "-p", "rusty-claude-cli", "--bin", "claw", "check_auth_health_uses_openai_compatible_provider_for_gemini_config") }
        }
    )

    foreach ($step in $structuralSteps) {
        $results.Add((Invoke-Step -Name $step.Name -Action $step.Action))
    }
}

if (-not $StructuralOnly) {
    if (-not $SkipBuild) {
        $results.Add((Invoke-Step -Name "build-claw" -Action {
            Invoke-CommandChecked -FilePath "cargo" -ArgumentList @("build", "-p", "rusty-claude-cli", "--bin", "claw")
        }))
    }

    $liveMatrix = @(
        @{
            Name = "live-anthropic-sonnet"
            Model = "claude-sonnet-4-6"
            RequiredEnv = @("ANTHROPIC_API_KEY")
        },
        @{
            Name = "live-openai-gpt54"
            Model = "gpt-5.4"
            RequiredEnv = @("OPENAI_API_KEY")
        },
        @{
            Name = "live-xai-grok3"
            Model = "grok-3"
            RequiredEnv = @("XAI_API_KEY")
        },
        @{
            Name = "live-openrouter-qwen35-27b"
            Model = "openai/qwen/qwen3.5-27b"
            RequiredEnv = @("OPENROUTER_API_KEY")
            EnvMap = @{
                "OPENAI_API_KEY" = [System.Environment]::GetEnvironmentVariable("OPENROUTER_API_KEY")
                "OPENAI_BASE_URL" = "https://openrouter.ai/api/v1"
            }
        },
        @{
            Name = "live-gemini-structural-only"
            Model = "gemini-2.5-flash"
            RequiredEnv = @("OPENAI_API_KEY", "OPENAI_BASE_URL")
            StructuralOnlyReason = "gemini-* routes through the OpenAI-compatible path here and needs an explicit OPENAI_BASE_URL for a live backend"
        },
        @{
            Name = "live-qwen-structural-only"
            Model = "qwen-plus"
            RequiredEnv = @("DASHSCOPE_API_KEY")
            StructuralOnlyReason = "qwen-* needs DASHSCOPE_API_KEY for live validation"
        }
    )

    foreach ($entry in $liveMatrix) {
        $missing = @($entry.RequiredEnv | Where-Object {
            $value = [System.Environment]::GetEnvironmentVariable($_)
            [string]::IsNullOrWhiteSpace($value)
        })

        if ($missing.Count -gt 0) {
            $reason = if ($entry.ContainsKey("StructuralOnlyReason")) {
                $entry.StructuralOnlyReason
            } else {
                "missing required env vars: $($missing -join ', ')"
            }

            $results.Add([pscustomobject]@{
                Name = $entry.Name
                Status = "SKIP"
                Seconds = 0
                Detail = $reason
            })
            continue
        }

        $results.Add((Invoke-Step -Name $entry.Name -Action {
            $envMap = if ($entry.ContainsKey("EnvMap")) { $entry.EnvMap } else { @{} }
            Invoke-ClawPrompt -ClawPath $clawExe -Model $entry.Model -EnvMap $envMap
        }))
    }
}

$passed = @($results | Where-Object Status -eq "PASS").Count
$failed = @($results | Where-Object Status -eq "FAIL").Count
$skipped = @($results | Where-Object Status -eq "SKIP").Count

Write-Host ""
Write-Host "Provider Smoke Summary"
Write-Host "  PASS    $passed"
Write-Host "  FAIL    $failed"
Write-Host "  SKIP    $skipped"
Write-Host ""

foreach ($result in $results) {
    $detailSuffix = if ($result.Detail) { " :: $($result.Detail)" } else { "" }
    Write-Host ("[{0}] {1} ({2}s){3}" -f $result.Status, $result.Name, $result.Seconds, $detailSuffix)
}

if ($failed -gt 0) {
    exit 1
}
