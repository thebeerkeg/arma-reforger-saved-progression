# TBK Progression Bridge — smoke test
#
# Usage:
#   .\Tools\smoke-test.ps1 -ApiKey "your-api-key-here"
#   .\Tools\smoke-test.ps1 -ApiKey "your-api-key-here" -BaseUrl "http://127.0.0.1:8787"
#
# Exits 0 on success, non-zero on any failure. Safe to re-run; uses unique
# temp UIDs per invocation so it does not pollute real player rows.

param(
    [Parameter(Mandatory=$true)]
    [string]$ApiKey,

    [string]$BaseUrl = "http://127.0.0.1:8787"
)

$ErrorActionPreference = "Stop"
$headers = @{ "X-Api-Key" = $ApiKey; "Content-Type" = "application/json" }
$testUid1 = "smoketest-$(Get-Random)-1"
$testUid2 = "smoketest-$(Get-Random)-2"

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Ok($msg)   { Write-Host "    OK: $msg" -ForegroundColor Green }
function Fail($msg) { Write-Host "    FAIL: $msg" -ForegroundColor Red; exit 1 }

# --- 1. Health ---
Step "health check"
$health = Invoke-RestMethod -Method GET -Uri "$BaseUrl/health"
if ($health -ne "ok") { Fail "expected 'ok', got '$health'" }
Ok "bridge is up"

# --- 2. Auth rejection (no key) ---
Step "rejects unauthenticated requests"
try {
    Invoke-RestMethod -Method GET -Uri "$BaseUrl/leaderboard"
    Fail "request with no key should have been rejected"
} catch {
    if ($_.Exception.Response.StatusCode.value__ -ne 401) {
        Fail "expected 401, got $($_.Exception.Response.StatusCode.value__)"
    }
    Ok "401 returned without API key"
}

# --- 3. GET non-existent player → 404 ---
Step "non-existent player returns 404"
try {
    Invoke-RestMethod -Method GET -Uri "$BaseUrl/player/$testUid1" -Headers $headers
    Fail "expected 404 for unknown uid"
} catch {
    if ($_.Exception.Response.StatusCode.value__ -ne 404) {
        Fail "expected 404, got $($_.Exception.Response.StatusCode.value__)"
    }
    Ok "404 for unknown uid"
}

# --- 4. Single increment (creates row) ---
Step "single increment creates the player"
$body = @{
    last_known_name = "SmokeTester"
    kills           = 3
    total_score     = 150
    deaths          = 1
} | ConvertTo-Json
$rec = Invoke-RestMethod -Method POST -Uri "$BaseUrl/player/$testUid1/increment" -Headers $headers -Body $body
if ($rec.player_uid -ne $testUid1) { Fail "uid mismatch in response" }
if ($rec.kills -ne 3)              { Fail "expected 3 kills, got $($rec.kills)" }
if ($rec.total_score -ne 150)      { Fail "expected 150 score, got $($rec.total_score)" }
Ok "row created: $($rec.kills) kills, $($rec.total_score) score"

# --- 5. Second increment is additive ---
Step "second increment is additive"
$body = @{ last_known_name = "SmokeTester"; kills = 2; total_score = 100 } | ConvertTo-Json
$rec = Invoke-RestMethod -Method POST -Uri "$BaseUrl/player/$testUid1/increment" -Headers $headers -Body $body
if ($rec.kills -ne 5)         { Fail "expected 5 kills (3+2), got $($rec.kills)" }
if ($rec.total_score -ne 250) { Fail "expected 250 score (150+100), got $($rec.total_score)" }
Ok "deltas accumulate: $($rec.kills) kills, $($rec.total_score) score"

# --- 6. GET reads back the same record ---
Step "GET returns persisted state"
$rec = Invoke-RestMethod -Method GET -Uri "$BaseUrl/player/$testUid1" -Headers $headers
if ($rec.kills -ne 5 -or $rec.total_score -ne 250) { Fail "GET state mismatch" }
Ok "state matches"

# --- 7. Batch increment ---
Step "batch increment"
$body = @{
    entries = @(
        @{ player_uid = $testUid1; last_known_name = "SmokeTester";  ai_kills = 10; total_score = 50  }
        @{ player_uid = $testUid2; last_known_name = "SmokeTester2"; kills    = 1;  total_score = 100 }
    )
} | ConvertTo-Json -Depth 10
$resp = Invoke-RestMethod -Method POST -Uri "$BaseUrl/player/batch-increment" -Headers $headers -Body $body
if ($resp.applied -ne 2) { Fail "expected applied=2, got $($resp.applied)" }
Ok "batch of 2 applied"

# --- 8. Verify batch deltas landed ---
Step "verify batch results"
$rec = Invoke-RestMethod -Method GET -Uri "$BaseUrl/player/$testUid1" -Headers $headers
if ($rec.ai_kills -ne 10)     { Fail "expected 10 ai_kills, got $($rec.ai_kills)" }
if ($rec.total_score -ne 300) { Fail "expected 300 score (250+50), got $($rec.total_score)" }
$rec2 = Invoke-RestMethod -Method GET -Uri "$BaseUrl/player/$testUid2" -Headers $headers
if ($rec2.total_score -ne 100) { Fail "expected 100 score for uid2" }
Ok "both rows updated correctly"

# --- 9. Leaderboard ---
Step "leaderboard"
$lb = Invoke-RestMethod -Method GET -Uri "$BaseUrl/leaderboard?limit=10" -Headers $headers
if (-not $lb.entries) { Fail "no entries in leaderboard" }
$found = $lb.entries | Where-Object { $_.player_uid -eq $testUid1 }
if (-not $found) { Fail "test uid not in top 10" }
Ok "leaderboard returned $($lb.entries.Count) entries; test uid ranked #$($found.rank)"

Write-Host ""
Write-Host "All smoke tests passed." -ForegroundColor Green
Write-Host "Test UIDs left in DB (you may want to clean them up):"
Write-Host "  $testUid1"
Write-Host "  $testUid2"
