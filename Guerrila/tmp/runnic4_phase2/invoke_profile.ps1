param(
    [Parameter(Mandatory = $true)][string]$ProfileName,
    [Parameter(Mandatory = $true)][int]$ReplayThreads,
    [Parameter(Mandatory = $true)][int]$RayonThreads
)

$ErrorActionPreference = 'Stop'

$RepoRoot = 'C:\Shadow Engine\Guerrila'
$TmpRoot = Join-Path $RepoRoot 'tmp\runnic4_phase2'
$ProfileRoot = Join-Path $TmpRoot $ProfileName
$KeyPath = Join-Path $RepoRoot 'g1-host-pack\ssh_file.txt'
$SshExe = 'C:\WINDOWS\System32\OpenSSH\ssh.exe'
$ScpExe = 'C:\WINDOWS\System32\OpenSSH\scp.exe'
$Remote = 'ubuntu@109.94.99.15'
$RemoteRepoRoot = '/home/ubuntu/Guerrila'
$RemoteResultsRoot = '/home/ubuntu/shadow-host-audit/dpdk-runs'
$RemoteEnvPath = '/etc/shadow-engine/env.g1.local.sh'
$HeliusRpc = 'https://mainnet.helius-rpc.com/?api-key=REDACTED_API_KEY'

$BaselineStep = "epoch8_auto_strict_directarb_quote_identity_runnic4_${ProfileName}_baseline"
$LeaseStep = "epoch8_auto_strict_directarb_quote_identity_runnic4_${ProfileName}_120s_refmerge"

New-Item -ItemType Directory -Force -Path $ProfileRoot | Out-Null

function Invoke-Ssh {
    param([string]$RemoteCommand)
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($RemoteCommand)
    $b64 = [Convert]::ToBase64String($bytes)
    $runner = "echo '$b64' | base64 -d | bash"
    & $SshExe -i $KeyPath -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=3 $Remote $runner
}

function Invoke-ScpFromRemote {
    param(
        [string]$RemotePath,
        [string]$LocalPath
    )
    & $ScpExe -i $KeyPath -o ConnectTimeout=10 "${Remote}:$RemotePath" $LocalPath
}

function Get-HeliusSlots {
    $payload = '{"jsonrpc":"2.0","id":1,"method":"getSlot"}'
    $processed = Invoke-RestMethod -Method Post -Uri $HeliusRpc -ContentType 'application/json' -Body $payload -TimeoutSec 10
    $finalizedBody = '{"jsonrpc":"2.0","id":1,"method":"getSlot","params":[{"commitment":"finalized"}]}'
    $finalized = Invoke-RestMethod -Method Post -Uri $HeliusRpc -ContentType 'application/json' -Body $finalizedBody -TimeoutSec 10
    [pscustomobject]@{
        processed = [int64]$processed.result
        finalized = [int64]$finalized.result
    }
}

function Wait-RemoteGreen {
    param([int]$TimeoutSeconds = 1800)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $json = Invoke-Ssh @'
python3 - <<'PY'
import json, subprocess, urllib.request, urllib.error
def rpc(method, params=None):
    body = {"jsonrpc":"2.0","id":1,"method":method}
    if params is not None:
        body["params"] = params
    req = urllib.request.Request("http://127.0.0.1:8899", data=json.dumps(body).encode(), headers={"Content-Type":"application/json"})
    return json.loads(urllib.request.urlopen(req, timeout=5).read().decode())
try:
    health = rpc("getHealth").get("result")
except Exception:
    health = None
try:
    local_processed = rpc("getSlot").get("result")
except Exception:
    local_processed = None
try:
    local_finalized = rpc("getSlot", [{"commitment":"finalized"}]).get("result")
except Exception:
    local_finalized = None
udp = int(subprocess.check_output("ss -lun | grep -E ':(800[0-9]|801[0-9]|802[0-5])[[:space:]]' | wc -l", shell=True, text=True).strip())
rpc_8899 = int(subprocess.check_output("sh -lc \"ss -ltn | grep -c ':8899 ' || true\"", shell=True, text=True).strip() or "0")
service = subprocess.check_output("systemctl is-active sh1-agave-no-geyser-2.service || true", shell=True, text=True).strip()
print(json.dumps({
    "health": health,
    "local_processed": local_processed,
    "local_finalized": local_finalized,
    "udp": udp,
    "rpc_8899": rpc_8899,
    "service": service,
}))
PY
'@
        $state = $json | ConvertFrom-Json
        if ($state.health -eq 'ok' -and $state.udp -ge 30 -and $state.rpc_8899 -ge 1 -and $state.service -eq 'active') {
            return $state
        }
        Start-Sleep -Seconds 5
    }
    throw "Remote host did not return green within $TimeoutSeconds seconds"
}

function Set-RemoteThreads {
    param([int]$Replay, [int]$Rayon)
    $cmd = @"
sudo python3 - <<'PY'
from pathlib import Path
path = Path("$RemoteEnvPath")
text = path.read_text() if path.exists() else ""
if "\\n" in text:
    text = text.replace("\\n", "\n")
lines = text.splitlines()
out = []
seen_replay = False
seen_rayon = False
for line in lines:
    if not line.strip():
        continue
    if line.startswith("export SH1_REPLAY_TRANSACTIONS_THREADS="):
        if not seen_replay:
            out.append("export SH1_REPLAY_TRANSACTIONS_THREADS=$Replay")
            seen_replay = True
    elif line.startswith("export SH1_RAYON_GLOBAL_THREADS="):
        if not seen_rayon:
            out.append("export SH1_RAYON_GLOBAL_THREADS=$Rayon")
            seen_rayon = True
    else:
        out.append(line)
if not seen_replay:
    out.append("export SH1_REPLAY_TRANSACTIONS_THREADS=$Replay")
if not seen_rayon:
    out.append("export SH1_RAYON_GLOBAL_THREADS=$Rayon")
path.write_text("\n".join(out) + "\n")
PY
sudo systemctl restart sh1-agave-no-geyser-2.service
"@
    Invoke-Ssh $cmd | Out-Null
}

function Run-RemoteBaseline {
    param([string]$StepName)
    $cmd = @"
set -e
STEP='$StepName'
OUT='$RemoteResultsRoot/'"\$STEP"
mkdir -p "\$OUT"
python3 - <<'PY'
import json, subprocess, time, urllib.request, urllib.error
out = "$RemoteResultsRoot/$StepName"
samples = []
helius = "$HeliusRpc"
def rpc(url, method, params=None, timeout=8):
    body = {"jsonrpc":"2.0","id":1,"method":method}
    if params is not None:
        body["params"] = params
    req = urllib.request.Request(url, data=json.dumps(body).encode(), headers={"Content-Type":"application/json"})
    return json.loads(urllib.request.urlopen(req, timeout=timeout).read().decode())
start = time.time()
for _ in range(10):
    ts = time.time()
    health = rpc("http://127.0.0.1:8899", "getHealth").get("result")
    lp = rpc("http://127.0.0.1:8899", "getSlot").get("result")
    lf = rpc("http://127.0.0.1:8899", "getSlot", [{"commitment":"finalized"}]).get("result")
    rp = rpc(helius, "getSlot").get("result")
    rf = rpc(helius, "getSlot", [{"commitment":"finalized"}]).get("result")
    sample = {
        "timestamp_unix": ts,
        "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(ts)),
        "health": health,
        "local_processed": lp,
        "local_finalized": lf,
        "reference_processed": rp,
        "reference_finalized": rf,
        "processed_lag": rp - lp,
        "finalized_lag": rf - lf,
    }
    samples.append(sample)
    print(json.dumps(sample), flush=True)
    time.sleep(30)
slopes = []
for i in range(1, len(samples)):
    dt = samples[i]["timestamp_unix"] - samples[i-1]["timestamp_unix"]
    if dt > 0:
        slopes.append((samples[i]["processed_lag"] - samples[i-1]["processed_lag"]) / dt)
slopes = sorted(slopes)
def pct(values, q):
    if not values:
        return None
    idx = max(0, min(len(values)-1, int((len(values) * q + 0.999999)) - 1))
    return values[idx]
summary = {
    "profile": "$ProfileName",
    "replay_threads": $ReplayThreads,
    "rayon_threads": $RayonThreads,
    "sample_count": len(samples),
    "processed_lag_start": samples[0]["processed_lag"],
    "processed_lag_end": samples[-1]["processed_lag"],
    "processed_lag_max": max(s["processed_lag"] for s in samples),
    "finalized_lag_start": samples[0]["finalized_lag"],
    "finalized_lag_end": samples[-1]["finalized_lag"],
    "finalized_lag_max": max(s["finalized_lag"] for s in samples),
    "lag_slope_p95": pct(slopes, 0.95),
}
with open(f"{out}/baseline_samples.jsonl", "w", encoding="utf-8") as fh:
    for sample in samples:
        fh.write(json.dumps(sample) + "\\n")
with open(f"{out}/baseline_summary.json", "w", encoding="utf-8") as fh:
    json.dump(summary, fh, indent=2)
print("BASELINE_SUMMARY=" + json.dumps(summary), flush=True)
PY
"@
    Invoke-Ssh $cmd
}

function Start-RemoteLease {
    param([string]$StepName)
    $remoteLog = "$RemoteResultsRoot/$StepName/controller.log"
    $cmd = @"
nohup bash -lc 'cd $RemoteRepoRoot && sudo -E env SHADOW_RUNNIC3_STEP=$StepName SHADOW_RUNNIC3_DURATION_SECONDS=120 SHADOW_RUNNIC3_ENFORCE_SELF_REFERENCE=true SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL=http://127.0.0.1:8899 bash g1-host-pack/scripts/79_shadow_eno1_lease_duration_probe.sh' > '$remoteLog' 2>&1 < /dev/null &
echo \$!
"@
    (Invoke-Ssh $cmd | Select-Object -Last 1).Trim()
}

function Wait-RemoteSummary {
    param([string]$StepName, [int]$TimeoutSeconds = 600)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $summaryPath = "$RemoteResultsRoot/$StepName/summary.txt"
    while ((Get-Date) -lt $deadline) {
        $result = Invoke-Ssh "if [ -f '$summaryPath' ]; then echo READY; else echo WAIT; fi"
        if (($result | Select-Object -Last 1).Trim() -eq 'READY') {
            return
        }
        Start-Sleep -Seconds 5
    }
    throw "Remote summary $summaryPath was not written within $TimeoutSeconds seconds"
}

function Merge-Lease {
    $leasePath = Join-Path $ProfileRoot 'lease_lag_samples.jsonl'
    $refPath = Join-Path $ProfileRoot 'lease_reference_samples.jsonl'
    $lease = Get-Content $leasePath | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json }
    $ref = Get-Content $refPath | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json }
    $merged = foreach ($l in $lease) {
        $best = $null
        $bestDiff = [double]::PositiveInfinity
        foreach ($r in $ref) {
            $d = [math]::Abs([double]$r.timestamp_unix - [double]$l.timestamp_unix)
            if ($d -lt $bestDiff) {
                $bestDiff = $d
                $best = $r
            }
        }
        if ($null -ne $best) {
            [pscustomobject]@{
                timestamp_unix = [double]$l.timestamp_unix
                elapsed_seconds = [double]$l.elapsed_seconds
                local_processed = [int64]$l.local_processed_slot
                local_finalized = [int64]$l.local_finalized_slot
                reference_processed = [int64]$best.reference_processed
                reference_finalized = [int64]$best.reference_finalized
                processed_lag = [int64]$best.reference_processed - [int64]$l.local_processed_slot
                finalized_lag = [int64]$best.reference_finalized - [int64]$l.local_finalized_slot
                ref_diff_seconds = [math]::Round($bestDiff, 3)
            }
        }
    }
    $merged = $merged | Sort-Object elapsed_seconds
    $slopes = @()
    for ($i = 1; $i -lt $merged.Count; $i++) {
        $dt = [double]($merged[$i].elapsed_seconds - $merged[$i - 1].elapsed_seconds)
        if ($dt -gt 0) {
            $slopes += [pscustomobject]@{
                elapsed_seconds = [double]$merged[$i].elapsed_seconds
                processed_slope = ([double]($merged[$i].processed_lag - $merged[$i - 1].processed_lag) / $dt)
                finalized_slope = ([double]($merged[$i].finalized_lag - $merged[$i - 1].finalized_lag) / $dt)
                processed_lag = [int64]$merged[$i].processed_lag
                finalized_lag = [int64]$merged[$i].finalized_lag
            }
        }
    }
    function Get-Percentile([object[]]$vals, [double]$p) {
        if ($vals.Count -eq 0) { return $null }
        $sorted = @($vals | Sort-Object)
        $idx = [math]::Ceiling($p * $sorted.Count) - 1
        if ($idx -lt 0) { $idx = 0 }
        if ($idx -ge $sorted.Count) { $idx = $sorted.Count - 1 }
        [double]$sorted[$idx]
    }
    $processedLags = @($merged | ForEach-Object { [int64]$_.processed_lag })
    $finalizedLags = @($merged | ForEach-Object { [int64]$_.finalized_lag })
    $processedSlopes = @($slopes | ForEach-Object { [double]$_.processed_slope })
    $firstLag50 = $merged | Where-Object { $_.processed_lag -gt 50 } | Select-Object -First 1
    $firstLag100 = $merged | Where-Object { $_.processed_lag -gt 100 } | Select-Object -First 1
    $firstSlope1 = $slopes | Where-Object { $_.processed_slope -gt 1.0 } | Select-Object -First 1
    $warnStreak = $null
    for ($i = 2; $i -lt $slopes.Count; $i++) {
        if ($slopes[$i - 2].processed_slope -gt 0.5 -and $slopes[$i - 1].processed_slope -gt 0.5 -and $slopes[$i].processed_slope -gt 0.5) {
            $warnStreak = $slopes[$i].elapsed_seconds
            break
        }
    }
    $report = [ordered]@{
        profile_name = $ProfileName
        replay_threads = $ReplayThreads
        rayon_threads = $RayonThreads
        merged_sample_count = $merged.Count
        processed_lag_start = $processedLags[0]
        processed_lag_max = ($processedLags | Measure-Object -Maximum).Maximum
        processed_lag_end = $processedLags[-1]
        finalized_lag_start = $finalizedLags[0]
        finalized_lag_max = ($finalizedLags | Measure-Object -Maximum).Maximum
        finalized_lag_end = $finalizedLags[-1]
        lag_slope_p50 = [math]::Round((Get-Percentile $processedSlopes 0.5), 6)
        lag_slope_p95 = [math]::Round((Get-Percentile $processedSlopes 0.95), 6)
        lag_slope_max = [math]::Round((($processedSlopes | Measure-Object -Maximum).Maximum), 6)
        time_to_lag_50 = if ($firstLag50) { [math]::Round([double]$firstLag50.elapsed_seconds, 3) } else { $null }
        time_to_lag_100 = if ($firstLag100) { [math]::Round([double]$firstLag100.elapsed_seconds, 3) } else { $null }
        time_to_slope_gt_1 = if ($firstSlope1) { [math]::Round([double]$firstSlope1.elapsed_seconds, 3) } else { $null }
        warning_streak_3_elapsed = if ($warnStreak) { [math]::Round([double]$warnStreak, 3) } else { $null }
        mean_ref_match_diff_seconds = [math]::Round((($merged | Measure-Object -Property ref_diff_seconds -Average).Average), 3)
    }
    $report | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $ProfileRoot 'lease_report.json')
    $merged | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $ProfileRoot 'lease_merged_samples.json')
    return [pscustomobject]$report
}

Write-Host "Setting remote profile $ProfileName => replay=$ReplayThreads rayon=$RayonThreads"
Set-RemoteThreads -Replay $ReplayThreads -Rayon $RayonThreads
Write-Host "Waiting for remote green"
$green = Wait-RemoteGreen
Write-Host ("Remote green: health={0} udp={1} local_processed={2} local_finalized={3}" -f $green.health, $green.udp, $green.local_processed, $green.local_finalized)

Write-Host "Running 5-minute baseline"
Run-RemoteBaseline -StepName $BaselineStep | Tee-Object -FilePath (Join-Path $ProfileRoot 'baseline_console.log')
Invoke-ScpFromRemote -RemotePath "$RemoteResultsRoot/$BaselineStep/baseline_summary.json" -LocalPath (Join-Path $ProfileRoot 'baseline_summary.json') | Out-Null
Invoke-ScpFromRemote -RemotePath "$RemoteResultsRoot/$BaselineStep/baseline_samples.jsonl" -LocalPath (Join-Path $ProfileRoot 'baseline_samples.jsonl') | Out-Null

Write-Host "Starting detached 120s lease"
$pid = Start-RemoteLease -StepName $LeaseStep
Write-Host "Remote lease pid=$pid"

$refPath = Join-Path $ProfileRoot 'lease_reference_samples.jsonl'
if (Test-Path $refPath) { Remove-Item $refPath -Force }
$deadline = (Get-Date).AddSeconds(185)
while ((Get-Date) -lt $deadline) {
    $ts = [DateTimeOffset]::UtcNow
    try {
        $slots = Get-HeliusSlots
        [pscustomobject]@{
            timestamp_utc = $ts.ToString('yyyy-MM-ddTHH:mm:ssZ')
            timestamp_unix = [math]::Round($ts.ToUnixTimeMilliseconds() / 1000.0, 3)
            reference_processed = $slots.processed
            reference_finalized = $slots.finalized
        } | ConvertTo-Json -Compress | Add-Content $refPath
    } catch {
        [pscustomobject]@{
            timestamp_utc = $ts.ToString('yyyy-MM-ddTHH:mm:ssZ')
            timestamp_unix = [math]::Round($ts.ToUnixTimeMilliseconds() / 1000.0, 3)
            reference_processed = $null
            reference_finalized = $null
            error = $_.Exception.Message
        } | ConvertTo-Json -Compress | Add-Content $refPath
    }
    Start-Sleep -Seconds 5
}

Write-Host "Waiting for remote summary and post-lease green"
Wait-RemoteSummary -StepName $LeaseStep
$greenAfter = Wait-RemoteGreen
Write-Host ("Remote green after lease: health={0} udp={1} local_processed={2} local_finalized={3}" -f $greenAfter.health, $greenAfter.udp, $greenAfter.local_processed, $greenAfter.local_finalized)

Invoke-ScpFromRemote -RemotePath "$RemoteResultsRoot/$LeaseStep/summary.txt" -LocalPath (Join-Path $ProfileRoot 'lease_summary.txt') | Out-Null
Invoke-ScpFromRemote -RemotePath "$RemoteResultsRoot/$LeaseStep/lag_samples.jsonl" -LocalPath (Join-Path $ProfileRoot 'lease_lag_samples.jsonl') | Out-Null
Invoke-ScpFromRemote -RemotePath "${RemoteResultsRoot}/${LeaseStep}_run40/tpu_summary.txt" -LocalPath (Join-Path $ProfileRoot 'run40_tpu_summary.txt') | Out-Null

$report = Merge-Lease
Write-Host "LEASE_REPORT=$($report | ConvertTo-Json -Compress)"
