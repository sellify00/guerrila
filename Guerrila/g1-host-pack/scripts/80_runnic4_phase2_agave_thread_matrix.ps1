param(
    [string]$SshPath = "C:\WINDOWS\System32\OpenSSH\ssh.exe",
    [string]$ScpPath = "C:\WINDOWS\System32\OpenSSH\scp.exe",
    [string]$KeyPath = "C:\Shadow Engine\Guerrila\g1-host-pack\ssh_file.txt",
    [string]$Remote = "ubuntu@109.94.99.15",
    [string]$ResultsRoot = "/home/ubuntu/shadow-host-audit/dpdk-runs",
    [string]$RemoteRepoRoot = "/home/ubuntu/Guerrila",
    [string]$HeliusUrl = "https://mainnet.helius-rpc.com/?api-key=REDACTED_API_KEY",
    [int]$DurationSeconds = 120,
    [string[]]$Profiles = @("32_28", "36_36", "32_32", "28_28")
)

$ErrorActionPreference = "Stop"

$script:RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$script:RunNic4LocalRoot = Join-Path $script:RepoRoot "tmp\runnic4_phase2"
New-Item -ItemType Directory -Force -Path $script:RunNic4LocalRoot | Out-Null

function Invoke-Remote {
    param([Parameter(Mandatory)][string]$Command)
    $output = & $SshPath -o StrictHostKeyChecking=no -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=3 -i $KeyPath $Remote $Command 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw ("remote_command_failed`n" + ($output -join "`n"))
    }
    return @($output)
}

function Copy-ToRemote {
    param(
        [Parameter(Mandatory)][string]$LocalPath,
        [Parameter(Mandatory)][string]$RemotePath
    )
    & $ScpPath -o StrictHostKeyChecking=no -o ConnectTimeout=10 -i $KeyPath $LocalPath "${Remote}:$RemotePath"
    if ($LASTEXITCODE -ne 0) {
        throw "scp_to_remote_failed: $LocalPath -> $RemotePath"
    }
}

function Copy-FromRemote {
    param(
        [Parameter(Mandatory)][string]$RemotePath,
        [Parameter(Mandatory)][string]$LocalPath
    )
    $parent = Split-Path -Parent $LocalPath
    if ($parent) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    & $ScpPath -o StrictHostKeyChecking=no -o ConnectTimeout=10 -i $KeyPath "${Remote}:$RemotePath" $LocalPath
    if ($LASTEXITCODE -ne 0) {
        throw "scp_from_remote_failed: $RemotePath -> $LocalPath"
    }
}

function Parse-KeyValueFile {
    param([Parameter(Mandatory)][string]$Path)
    $map = [ordered]@{}
    if (-not (Test-Path $Path)) {
        return $map
    }
    foreach ($line in Get-Content $Path) {
        if ($line -match "^[A-Za-z0-9_]+=") {
            $parts = $line -split "=", 2
            $map[$parts[0]] = $parts[1]
        }
    }
    return $map
}

function Get-ReferenceSlots {
    $processedBody = '{"jsonrpc":"2.0","id":1,"method":"getSlot","params":[{"commitment":"processed"}]}'
    $finalizedBody = '{"jsonrpc":"2.0","id":1,"method":"getSlot","params":[{"commitment":"finalized"}]}'
    $processed = (Invoke-RestMethod -Method Post -Uri $HeliusUrl -ContentType "application/json" -Body $processedBody -ConnectionTimeoutSeconds 10 -OperationTimeoutSeconds 10).result
    $finalized = (Invoke-RestMethod -Method Post -Uri $HeliusUrl -ContentType "application/json" -Body $finalizedBody -ConnectionTimeoutSeconds 10 -OperationTimeoutSeconds 10).result
    [pscustomobject]@{
        processed = [int64]$processed
        finalized = [int64]$finalized
    }
}

function Get-RemoteLocalState {
    $cmd = @'
python3 - <<PY
import json
import subprocess
import urllib.request

def rpc(method, params=None):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        payload["params"] = params
    req = urllib.request.Request(
        "http://127.0.0.1:8899",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=5) as response:
        return json.loads(response.read().decode())

def cmd_text(command):
    return subprocess.run(command, shell=True, capture_output=True, text=True).stdout.strip()

out = {}
try:
    out["health"] = rpc("getHealth").get("result", "error")
    out["processed"] = rpc("getSlot", [{"commitment": "processed"}]).get("result")
    out["finalized"] = rpc("getSlot", [{"commitment": "finalized"}]).get("result")
except Exception as exc:  # noqa: BLE001
    out["health"] = f"ERROR:{type(exc).__name__}:{exc}"
    out["processed"] = None
    out["finalized"] = None

out["udp_listener_count"] = int(cmd_text("ss -lun | grep -E ':(800[0-9]|801[0-9]|802[0-5])[[:space:]]' | wc -l") or "0")
out["combined"] = cmd_text("ethtool -l eno1 | awk '/Current hardware settings:/{current=1; next} current && $1 == \"Combined:\" {gsub(/[[:space:]]/, \"\", $2); print $2; exit}'")
eno1 = cmd_text("ip -details link show dev eno1 | head -n 1")
if "xdpgeneric" in eno1:
    out["xdp_mode"] = "xdpgeneric"
elif " xdp " in eno1:
    out["xdp_mode"] = "native"
else:
    out["xdp_mode"] = "none"
out["xsk_map"] = "present" if cmd_text("test -e /sys/fs/bpf/shadow_xsks_map && echo present || true") == "present" else "absent"
out["service_active"] = cmd_text("systemctl is-active sh1-agave-no-geyser-2.service 2>/dev/null || true")
print(json.dumps(out))
PY
'@
    $raw = Invoke-Remote $cmd
    $jsonLine = $raw | Where-Object { $_ -match '^\{' } | Select-Object -Last 1
    if (-not $jsonLine) {
        throw "remote_local_state_missing"
    }
    return $jsonLine | ConvertFrom-Json
}

function Set-RemoteThreadProfile {
    param(
        [Parameter(Mandatory)][int]$ReplayThreads,
        [Parameter(Mandatory)][int]$RayonThreads
    )
    $cmd = @'
sudo python3 - <<PY
from pathlib import Path

path = Path("/etc/shadow-engine/env.g1.local.sh")
text = path.read_text(encoding="utf-8") if path.exists() else ""
lines = text.splitlines()

def upsert(key: str, value: str):
    prefix = f"export {key}="
    for index, line in enumerate(lines):
        if line.startswith(prefix):
            lines[index] = f'export {key}="{value}"'
            return
    lines.append(f'export {key}="{value}"')

upsert("SH1_REPLAY_TRANSACTIONS_THREADS", "__REPLAY__")
upsert("SH1_RAYON_GLOBAL_THREADS", "__RAYON__")
path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print(path.read_text(encoding="utf-8"))
PY
'@
    $cmd = $cmd.Replace("__REPLAY__", [string]$ReplayThreads).Replace("__RAYON__", [string]$RayonThreads)
    $null = Invoke-Remote $cmd
}

function Restart-RemoteAgave {
    $cmd = @'
sudo pkill -x shadow-tier0 2>/dev/null || true
sudo pkill -x shadow-oracle 2>/dev/null || true
sudo pkill -x shadow-egress-tpu 2>/dev/null || true
sudo ip link set dev eno1 xdp off 2>/dev/null || true
sudo ip link set dev eno1 xdpgeneric off 2>/dev/null || true
sudo rm -f /sys/fs/bpf/shadow_xsks_map 2>/dev/null || true
sudo systemctl restart sh1-agave-no-geyser-2.service
'@
    $null = Invoke-Remote $cmd
}

function Wait-FlatGreenBaseline {
    param(
        [Parameter(Mandatory)][string]$ProfileName,
        [int]$SamplesRequired = 10,
        [int]$IntervalSeconds = 30,
        [string]$ProgressPath = ""
    )
    $samples = New-Object System.Collections.Generic.List[object]
    $prev = $null
    for ($i = 0; $i -lt $SamplesRequired; $i++) {
        if ($i -gt 0) {
            Start-Sleep -Seconds $IntervalSeconds
        }
        $local = Get-RemoteLocalState
        $reference = Get-ReferenceSlots
        $now = Get-Date
        $processedLag = if ($null -ne $local.processed) { [int64]$reference.processed - [int64]$local.processed } else { $null }
        $finalizedLag = if ($null -ne $local.finalized) { [int64]$reference.finalized - [int64]$local.finalized } else { $null }
        $processedSlope = 0.0
        $finalizedSlope = 0.0
        if ($null -ne $prev -and $null -ne $processedLag -and $null -ne $prev.processedLag) {
            $dt = [math]::Max(([double](New-TimeSpan -Start $prev.when -End $now).TotalSeconds), 0.001)
            $processedSlope = (($processedLag - $prev.processedLag) / $dt)
            $finalizedSlope = (($finalizedLag - $prev.finalizedLag) / $dt)
        }
        $samples.Add([pscustomobject]@{
            timestamp_utc = $now.ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
            health = [string]$local.health
            processed_lag = $processedLag
            finalized_lag = $finalizedLag
            processed_lag_slope = [double]([math]::Round($processedSlope, 6))
            finalized_lag_slope = [double]([math]::Round($finalizedSlope, 6))
            udp_listener_count = [int]$local.udp_listener_count
            combined = [string]$local.combined
            xdp_mode = [string]$local.xdp_mode
            xsk_map = [string]$local.xsk_map
            service_active = [string]$local.service_active
        }) | Out-Null
        if ($ProgressPath) {
            ($samples[$samples.Count - 1] | ConvertTo-Json -Compress) | Add-Content -Path $ProgressPath
        }
        $prev = [pscustomobject]@{
            when = $now
            processedLag = $processedLag
            finalizedLag = $finalizedLag
        }
        Write-Host "[$ProfileName] baseline sample $($i + 1)/$SamplesRequired health=$($local.health) processed_lag=$processedLag finalized_lag=$finalizedLag xdp=$($local.xdp_mode) combined=$($local.combined)"
    }

    $positiveSlopes = @($samples | ForEach-Object { [math]::Max(0.0, [double]$_.processed_lag_slope) } | Sort-Object)
    if ($positiveSlopes.Count -gt 0) {
        $idx = [math]::Min($positiveSlopes.Count - 1, [math]::Max(0, [math]::Ceiling(0.95 * $positiveSlopes.Count) - 1))
        $p95 = [math]::Round([double]$positiveSlopes[$idx], 6)
    } else {
        $p95 = 0.0
    }

    $allGreen = -not ($samples | Where-Object {
        $_.health -ne "ok" -or
        $_.service_active -ne "active" -or
        $_.xdp_mode -ne "none" -or
        $_.xsk_map -ne "absent" -or
        $_.combined -ne "16" -or
        $_.udp_listener_count -lt 30 -or
        $null -eq $_.processed_lag -or
        $null -eq $_.finalized_lag
    })

    [pscustomobject]@{
        samples = $samples
        processed_lag_start = if ($samples.Count) { [int64]$samples[0].processed_lag } else { $null }
        processed_lag_end = if ($samples.Count) { [int64]$samples[$samples.Count - 1].processed_lag } else { $null }
        finalized_lag_start = if ($samples.Count) { [int64]$samples[0].finalized_lag } else { $null }
        finalized_lag_end = if ($samples.Count) { [int64]$samples[$samples.Count - 1].finalized_lag } else { $null }
        lag_slope_p95 = $p95
        all_green = $allGreen
    }
}

function Start-RemoteLease {
    param(
        [Parameter(Mandatory)][string]$StepName,
        [Parameter(Mandatory)][int]$Duration
    )
    $logRemote = "$ResultsRoot/runnic3_logs/$StepName.log"
    $pidFileRemote = "$ResultsRoot/runnic3_logs/$StepName.pid"
    $cmd = @'
set -e
mkdir -p __RESULTS__/runnic3_logs
rm -f __LOG__ __PID__
cd __REPO__
nohup sudo -E env SHADOW_RESULTS_DIR=__RESULTS__ SHADOW_RUNNIC3_STEP=__STEP__ SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL=http://127.0.0.1:8899 bash g1-host-pack/scripts/79_shadow_eno1_lease_duration_probe.sh __DURATION__ >__LOG__ 2>&1 < /dev/null &
echo $! > __PID__
echo pid=$(cat __PID__)
'@
    $cmd = $cmd.Replace("__RESULTS__", $ResultsRoot).Replace("__LOG__", $logRemote).Replace("__PID__", $pidFileRemote).Replace("__REPO__", $RemoteRepoRoot).Replace("__STEP__", $StepName).Replace("__DURATION__", [string]$Duration)
    $output = Invoke-Remote $cmd
    $pidLine = $output | Where-Object { $_ -match '^pid=' } | Select-Object -Last 1
    if (-not $pidLine) {
        throw "remote_lease_pid_missing"
    }
    [pscustomobject]@{
        step = $StepName
        pid = [int]($pidLine -replace '^pid=', '')
        log_remote = $logRemote
        pid_file_remote = $pidFileRemote
    }
}

function Wait-RemoteProfileCompletion {
    param(
        [Parameter(Mandatory)][string]$StepName,
        [int]$TimeoutSeconds = 1200,
        [int]$IntervalSeconds = 15
    )
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $state = Invoke-Remote "if [ -f '$ResultsRoot/$StepName/summary.txt' ] && grep -q '^cleanup_verified=' '$ResultsRoot/$StepName/summary.txt'; then echo ready; fi"
            if ($state -contains "ready") {
                return
            }
        } catch {
        }
        Start-Sleep -Seconds $IntervalSeconds
    }
    throw "remote_profile_completion_timeout: $StepName"
}

function Parse-JsonLines {
    param([Parameter(Mandatory)][string]$Path)
    $items = New-Object System.Collections.Generic.List[object]
    if (-not (Test-Path $Path)) {
        return $items
    }
    foreach ($line in Get-Content $Path) {
        if ($line -match '^\{') {
            $items.Add(($line | ConvertFrom-Json)) | Out-Null
        }
    }
    return $items
}

function Merge-LeaseSamples {
    param(
        [Parameter(Mandatory)]$RemoteLagSamples,
        [Parameter(Mandatory)]$ReferenceSamples
    )
    $merged = New-Object System.Collections.Generic.List[object]
    foreach ($remoteSample in $RemoteLagSamples) {
        $nearest = $null
        $bestDelta = [double]::PositiveInfinity
        foreach ($referenceSample in $ReferenceSamples) {
            $delta = [math]::Abs([double]$referenceSample.timestamp_unix - [double]$remoteSample.timestamp_unix)
            if ($delta -lt $bestDelta) {
                $bestDelta = $delta
                $nearest = $referenceSample
            }
        }
        if ($null -eq $nearest) {
            continue
        }
        $merged.Add([pscustomobject]@{
            elapsed_seconds = [double]$remoteSample.elapsed_seconds
            timestamp_unix = [double]$remoteSample.timestamp_unix
            local_processed = [int64]$remoteSample.local_processed_slot
            local_finalized = [int64]$remoteSample.local_finalized_slot
            reference_processed = [int64]$nearest.reference_processed
            reference_finalized = [int64]$nearest.reference_finalized
            processed_lag = [int64]$nearest.reference_processed - [int64]$remoteSample.local_processed_slot
            finalized_lag = [int64]$nearest.reference_finalized - [int64]$remoteSample.local_finalized_slot
            match_delta_seconds = [double]([math]::Round($bestDelta, 3))
        }) | Out-Null
    }
    return $merged
}

function Compute-LagMetrics {
    param([Parameter(Mandatory)]$MergedSamples)
    $slopes = New-Object System.Collections.Generic.List[object]
    for ($i = 1; $i -lt $MergedSamples.Count; $i++) {
        $dt = [math]::Max(([double]$MergedSamples[$i].timestamp_unix - [double]$MergedSamples[$i - 1].timestamp_unix), 0.001)
        $slope = ([double]$MergedSamples[$i].processed_lag - [double]$MergedSamples[$i - 1].processed_lag) / $dt
        $slopes.Add([pscustomobject]@{
            elapsed_seconds = [double]$MergedSamples[$i].elapsed_seconds
            slope = [double]([math]::Round($slope, 6))
        }) | Out-Null
    }
    $positive = @($slopes | ForEach-Object { [math]::Max(0.0, [double]$_.slope) } | Sort-Object)
    if ($positive.Count -gt 0) {
        $p50Index = [math]::Min($positive.Count - 1, [math]::Max(0, [math]::Ceiling(0.50 * $positive.Count) - 1))
        $p95Index = [math]::Min($positive.Count - 1, [math]::Max(0, [math]::Ceiling(0.95 * $positive.Count) - 1))
        $p50 = [math]::Round([double]$positive[$p50Index], 6)
        $p95 = [math]::Round([double]$positive[$p95Index], 6)
        $smax = [math]::Round(([double]($positive | Measure-Object -Maximum).Maximum), 6)
    } else {
        $p50 = 0.0
        $p95 = 0.0
        $smax = 0.0
    }

    $firstLag50 = $MergedSamples | Where-Object { $_.processed_lag -ge 50 } | Select-Object -First 1
    $firstLag100 = $MergedSamples | Where-Object { $_.processed_lag -ge 100 } | Select-Object -First 1
    $firstSlopeGt1 = $slopes | Where-Object { $_.slope -gt 1.0 } | Select-Object -First 1

    [pscustomobject]@{
        slopes = $slopes
        processed_lag_start = if ($MergedSamples.Count) { [int64]$MergedSamples[0].processed_lag } else { $null }
        processed_lag_max = if ($MergedSamples.Count) { [int64](($MergedSamples | Measure-Object -Property processed_lag -Maximum).Maximum) } else { $null }
        processed_lag_end = if ($MergedSamples.Count) { [int64]$MergedSamples[$MergedSamples.Count - 1].processed_lag } else { $null }
        finalized_lag_start = if ($MergedSamples.Count) { [int64]$MergedSamples[0].finalized_lag } else { $null }
        finalized_lag_max = if ($MergedSamples.Count) { [int64](($MergedSamples | Measure-Object -Property finalized_lag -Maximum).Maximum) } else { $null }
        finalized_lag_end = if ($MergedSamples.Count) { [int64]$MergedSamples[$MergedSamples.Count - 1].finalized_lag } else { $null }
        lag_slope_p50 = $p50
        lag_slope_p95 = $p95
        lag_slope_max = $smax
        time_to_lag_50 = if ($firstLag50) { [double]$firstLag50.elapsed_seconds } else { $null }
        time_to_lag_100 = if ($firstLag100) { [double]$firstLag100.elapsed_seconds } else { $null }
        time_to_slope_gt_1 = if ($firstSlopeGt1) { [double]$firstSlopeGt1.elapsed_seconds } else { $null }
        max_match_delta_seconds = if ($MergedSamples.Count) { [double](($MergedSamples | Measure-Object -Property match_delta_seconds -Maximum).Maximum) } else { $null }
    }
}

function Wait-RemoteGreenReady {
    param(
        [Parameter(Mandatory)][string]$ProfileName,
        [int]$TimeoutSeconds = 900,
        [int]$IntervalSeconds = 10,
        [string]$ProgressPath = ""
    )
    $remoteCommand = @'
deadline=$((SECONDS + __TIMEOUT__))
while true; do
  ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  service_active=$(systemctl is-active sh1-agave-no-geyser-2.service 2>/dev/null || true)
  health_json=$(curl -sS -m 3 -X POST -H "Content-Type: application/json" --data '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' http://127.0.0.1:8899 2>/dev/null || true)
  if [[ "$health_json" == *'"result":"ok"'* ]]; then
    health_state=ok
  else
    health_state=${health_json:-unavailable}
  fi
  udp_count=$(ss -lun | grep -E ':(800[0-9]|801[0-9]|802[0-5])[[:space:]]' | wc -l)
  combined=$(ethtool -l eno1 | awk '/Current hardware settings:/{current=1; next} current && $1 == "Combined:" {gsub(/[[:space:]]/, "", $2); print $2; exit}')
  first_line=$(ip -details link show dev eno1 2>/dev/null | head -n 1 || true)
  if [[ "$first_line" == *"xdpgeneric"* ]]; then
    xdp_mode=xdpgeneric
  elif [[ "$first_line" == *" xdp "* ]]; then
    xdp_mode=native
  else
    xdp_mode=none
  fi
  if test -e /sys/fs/bpf/shadow_xsks_map 2>/dev/null; then
    xsk_map=present
  else
    xsk_map=absent
  fi
  printf '%s|%s|%s|%s|%s|%s|%s\n' "$ts" "$service_active" "$health_state" "$udp_count" "$combined" "$xdp_mode" "$xsk_map"
  if [[ "$service_active" == "active" && "$health_state" == "ok" && "$udp_count" -ge 30 && "$combined" == "16" && "$xdp_mode" == "none" && "$xsk_map" == "absent" ]]; then
    exit 0
  fi
  if (( SECONDS >= deadline )); then
    exit 1
  fi
  sleep __INTERVAL__
done
'@
    $remoteCommand = $remoteCommand.Replace("__TIMEOUT__", [string]$TimeoutSeconds).Replace("__INTERVAL__", [string]$IntervalSeconds)
    $lines = Invoke-Remote $remoteCommand
    foreach ($line in $lines) {
        if ($line -match '^[0-9]{4}-[0-9]{2}-[0-9]{2}T') {
            $parts = $line -split '\|', 7
            $sample = [pscustomobject]@{
                timestamp_utc = $parts[0]
                service_active = $parts[1]
                health = $parts[2]
                udp_listener_count = [int]$parts[3]
                combined = $parts[4]
                xdp_mode = $parts[5]
                xsk_map = $parts[6]
            }
            Write-Host "[$ProfileName] green-wait health=$($sample.health) udp=$($sample.udp_listener_count) xdp=$($sample.xdp_mode) combined=$($sample.combined)"
            if ($ProgressPath) {
                ($sample | ConvertTo-Json -Compress) | Add-Content -Path $ProgressPath
            }
        }
    }
}

function Invoke-ProfileRun {
    param(
        [Parameter(Mandatory)][string]$ProfileName,
        [Parameter(Mandatory)][int]$ReplayThreads,
        [Parameter(Mandatory)][int]$RayonThreads
    )

    $profileLocalRoot = Join-Path $script:RunNic4LocalRoot $ProfileName
    New-Item -ItemType Directory -Force -Path $profileLocalRoot | Out-Null
    $baselinePath = Join-Path $profileLocalRoot "baseline_samples.json"
    $baselineProgressPath = Join-Path $profileLocalRoot "baseline_progress.jsonl"
    $greenWaitProgressPath = Join-Path $profileLocalRoot "green_wait_progress.jsonl"
    $referencePath = Join-Path $profileLocalRoot "reference_samples.jsonl"
    $reportPath = Join-Path $profileLocalRoot "profile_report.json"
    Remove-Item $baselineProgressPath,$greenWaitProgressPath -Force -ErrorAction SilentlyContinue

    Write-Host "[$ProfileName] applying thread profile replay=$ReplayThreads rayon=$RayonThreads"
    Set-RemoteThreadProfile -ReplayThreads $ReplayThreads -RayonThreads $RayonThreads
    Restart-RemoteAgave

    Write-Host "[$ProfileName] waiting for Agave green before baseline hold"
    Wait-RemoteGreenReady -ProfileName $ProfileName -ProgressPath $greenWaitProgressPath

    Write-Host "[$ProfileName] waiting for 5-minute flat-green baseline"
    $baseline = Wait-FlatGreenBaseline -ProfileName $ProfileName -ProgressPath $baselineProgressPath
    $baseline | ConvertTo-Json -Depth 6 | Set-Content -Path $baselinePath
    if (-not $baseline.all_green -or [double]$baseline.lag_slope_p95 -gt 0.25) {
        return [pscustomobject]@{
            profile_name = $ProfileName
            replay_threads = $ReplayThreads
            rayon_threads = $RayonThreads
            status = "blocked_pre_lease_baseline_not_green"
            baseline = $baseline
        }
    }

    $stepName = "epoch8_auto_strict_directarb_quote_identity_runnic4_${ProfileName}_120s_refmerge"
    Write-Host "[$ProfileName] starting 120s native lease"
    $lease = Start-RemoteLease -StepName $stepName -Duration $DurationSeconds

    $referenceSamples = New-Object System.Collections.Generic.List[object]
    $referenceStart = Get-Date
    $referenceIterations = [int][math]::Ceiling(($DurationSeconds + 30) / 5.0)
    for ($i = 0; $i -lt $referenceIterations; $i++) {
        $now = Get-Date
        $reference = Get-ReferenceSlots
        $referenceSamples.Add([pscustomobject]@{
            timestamp_utc = $now.ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
            timestamp_unix = [double]([DateTimeOffset]$now.ToUniversalTime()).ToUnixTimeMilliseconds() / 1000.0
            elapsed_seconds = [double]([math]::Round((New-TimeSpan -Start $referenceStart -End $now).TotalSeconds, 3))
            reference_processed = [int64]$reference.processed
            reference_finalized = [int64]$reference.finalized
        }) | Out-Null
        Start-Sleep -Seconds 5
    }
    $referenceSamples | ConvertTo-Json -Depth 6 | Set-Content -Path $referencePath

    Write-Host "[$ProfileName] waiting for remote cleanup/summary"
    Wait-RemoteProfileCompletion -StepName $stepName

    $remoteSummaryLocal = Join-Path $profileLocalRoot "summary.txt"
    $remoteLagLocal = Join-Path $profileLocalRoot "lag_samples.jsonl"
    $postCleanupLocal = Join-Path $profileLocalRoot "post_cleanup_lag_samples.txt"

    Copy-FromRemote -RemotePath "$ResultsRoot/$stepName/summary.txt" -LocalPath $remoteSummaryLocal
    Copy-FromRemote -RemotePath "$ResultsRoot/$stepName/lag_samples.jsonl" -LocalPath $remoteLagLocal
    try {
        Copy-FromRemote -RemotePath "$ResultsRoot/$stepName/post_cleanup_lag_samples.txt" -LocalPath $postCleanupLocal
    } catch {
    }

    $summaryMap = Parse-KeyValueFile -Path $remoteSummaryLocal
    $run40SummaryRemote = $summaryMap["run40_summary_path"]
    $tpuSummaryRemote = $summaryMap["run40_tpu_summary_path"]
    $run40SummaryLocal = Join-Path $profileLocalRoot "run40_summary.txt"
    $tpuSummaryLocal = Join-Path $profileLocalRoot "tpu_summary.txt"
    if ($run40SummaryRemote) {
        try { Copy-FromRemote -RemotePath $run40SummaryRemote -LocalPath $run40SummaryLocal } catch {}
    }
    if ($tpuSummaryRemote) {
        try { Copy-FromRemote -RemotePath $tpuSummaryRemote -LocalPath $tpuSummaryLocal } catch {}
    }
    $tpuMap = Parse-KeyValueFile -Path $tpuSummaryLocal

    $remoteLag = Parse-JsonLines -Path $remoteLagLocal
    $merged = Merge-LeaseSamples -RemoteLagSamples $remoteLag -ReferenceSamples $referenceSamples
    $mergedPath = Join-Path $profileLocalRoot "merged_samples.json"
    $merged | ConvertTo-Json -Depth 6 | Set-Content -Path $mergedPath
    $lagMetrics = Compute-LagMetrics -MergedSamples $merged

    $result = [ordered]@{
        profile_name = $ProfileName
        replay_threads = $ReplayThreads
        rayon_threads = $RayonThreads
        pre_lease_processed_lag = $baseline.processed_lag_start
        pre_lease_finalized_lag = $baseline.finalized_lag_start
        pre_lease_lag_slope = $baseline.lag_slope_p95
        lease_actual_duration = [double]($summaryMap["actual_duration"])
        processed_lag_start = $lagMetrics.processed_lag_start
        processed_lag_max = $lagMetrics.processed_lag_max
        processed_lag_end = $lagMetrics.processed_lag_end
        finalized_lag_start = $lagMetrics.finalized_lag_start
        finalized_lag_max = $lagMetrics.finalized_lag_max
        finalized_lag_end = $lagMetrics.finalized_lag_end
        lag_slope_p50 = $lagMetrics.lag_slope_p50
        lag_slope_p95 = $lagMetrics.lag_slope_p95
        lag_slope_max = $lagMetrics.lag_slope_max
        time_to_lag_50 = $lagMetrics.time_to_lag_50
        time_to_lag_100 = $lagMetrics.time_to_lag_100
        time_to_slope_gt_1 = $lagMetrics.time_to_slope_gt_1
        tpu_no_send_accept_count = [int64]($tpuMap["tpu_quic_no_send_accept_count"])
        tpu_wire_send_count = [int64]($tpuMap["tpu_wire_send_count"])
        live_landed = [int64]($tpuMap["live_landed"])
        rx_missed_errors_delta = [int64]($summaryMap["rx_missed_errors_delta"])
        rx_no_dma_resources_delta = [int64]($summaryMap["rx_no_dma_resources_delta"])
        rx_dropped_delta = [int64]($summaryMap["rx_dropped_delta"])
        rx_errors_delta = [int64]($summaryMap["rx_errors_delta"])
        softnet_drop_delta = [int64]($summaryMap["softnet_drop_delta"])
        cleanup_verified = [string]$summaryMap["cleanup_verified"]
        eno1_combined_after = [string]$summaryMap["eno1_combined_after"]
        agave_health_after_cleanup = [string]$summaryMap["agave_health_after_cleanup"]
        recovery_seconds_to_green = [double]($summaryMap["recovery_seconds_to_green"])
        cleanup_verification_seconds = [double]($summaryMap["cleanup_verification_seconds"])
        remote_status = [string]$summaryMap["status"]
        remote_stop_reason = [string]$summaryMap["stop_reason"]
        remote_step = $stepName
        merged_sample_count = $merged.Count
        max_match_delta_seconds = $lagMetrics.max_match_delta_seconds
    }

    $result["profile_viable_120s"] = (
        $result.processed_lag_max -lt 100 -and
        $result.finalized_lag_max -lt 100 -and
        $result.lag_slope_p95 -lt 1.0 -and
        $result.cleanup_verified -eq "true" -and
        $result.agave_health_after_cleanup -eq "ok"
    )

    $result | ConvertTo-Json -Depth 6 | Set-Content -Path $reportPath
    return [pscustomobject]$result
}

$local79 = Join-Path $PSScriptRoot "79_shadow_eno1_lease_duration_probe.sh"
Copy-ToRemote -LocalPath $local79 -RemotePath "$RemoteRepoRoot/g1-host-pack/scripts/79_shadow_eno1_lease_duration_probe.sh"

$profileSpecs = foreach ($profile in $Profiles) {
    $parts = $profile -split "_", 2
    [pscustomobject]@{
        name = $profile
        replay = [int]$parts[0]
        rayon = [int]$parts[1]
    }
}

$results = New-Object System.Collections.Generic.List[object]
foreach ($profileSpec in $profileSpecs) {
    $result = Invoke-ProfileRun -ProfileName $profileSpec.name -ReplayThreads $profileSpec.replay -RayonThreads $profileSpec.rayon
    $results.Add($result) | Out-Null
}

$resultsPath = Join-Path $script:RunNic4LocalRoot "matrix_results.json"
$results | ConvertTo-Json -Depth 6 | Set-Content -Path $resultsPath
$results | ConvertTo-Json -Depth 6
