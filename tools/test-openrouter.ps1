# Manual OpenRouter key test. Run:  powershell -ExecutionPolicy Bypass -File tools\test-openrouter.ps1 [model-id]
# The key is typed at the prompt (hidden) and kept only in memory for this run.

param(
  [string]$Model = "nex-agi/nex-n2.5-mini:free"
)

$secure = Read-Host "Paste your OpenRouter key (input is hidden)" -AsSecureString
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
$key = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr).Trim()
[Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
if (-not $key.StartsWith("sk-or-")) { Write-Host "Warning: OpenRouter keys start with sk-or-. Continuing anyway." -ForegroundColor Yellow }

$headers = @{ Authorization = "Bearer $key"; "Content-Type" = "application/json"; "HTTP-Referer" = "https://roundtable.local"; "X-Title" = "Roundtable key test" }

Write-Host ""
Write-Host "1) Key status and credit  (GET /api/v1/auth/key)" -ForegroundColor Cyan
try {
  $info = Invoke-RestMethod -Uri "https://openrouter.ai/api/v1/auth/key" -Headers $headers -Method Get -TimeoutSec 30
  $d = $info.data
  Write-Host ("   label: {0}" -f $d.label)
  Write-Host ("   usage so far: `${0}   limit: {1}   limit remaining: {2}" -f $d.usage, ($(if ($null -eq $d.limit) { "none" } else { "`$" + $d.limit })), ($(if ($null -eq $d.limit_remaining) { "n/a" } else { "`$" + $d.limit_remaining })))
  Write-Host ("   free tier: {0}" -f $d.is_free_tier)
  Write-Host "   OK: the key is valid." -ForegroundColor Green
} catch {
  $status = $_.Exception.Response.StatusCode.value__
  Write-Host ("   FAILED (HTTP {0}): {1}" -f $status, $_.ErrorDetails.Message) -ForegroundColor Red
  if ($status -eq 401) { Write-Host "   401 means OpenRouter does not recognise this key: revoked, mistyped, or from a deleted account. Create a new key at openrouter.ai -> Keys." -ForegroundColor Yellow }
  exit 1
}

Write-Host ""
Write-Host ("2) One tiny chat call to {0}  (POST /api/v1/chat/completions)" -f $Model) -ForegroundColor Cyan
$body = @{
  model = $Model
  messages = @(@{ role = "user"; content = "Reply with exactly the word OK." })
  max_tokens = 5
} | ConvertTo-Json -Depth 5
try {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $resp = Invoke-RestMethod -Uri "https://openrouter.ai/api/v1/chat/completions" -Headers $headers -Method Post -Body $body -TimeoutSec 60
  $sw.Stop()
  $text = $resp.choices[0].message.content
  Write-Host ("   model answered: '{0}'  in {1:N1}s  (served as {2})" -f $text.Trim(), $sw.Elapsed.TotalSeconds, $resp.model)
  Write-Host ("   tokens: prompt {0}, completion {1}" -f $resp.usage.prompt_tokens, $resp.usage.completion_tokens)
  Write-Host "   OK: the key can run models." -ForegroundColor Green
} catch {
  $status = $_.Exception.Response.StatusCode.value__
  Write-Host ("   FAILED (HTTP {0}): {1}" -f $status, $_.ErrorDetails.Message) -ForegroundColor Red
  if ($status -eq 402) { Write-Host "   402 means no credit on the account. Add credit at openrouter.ai -> Credits, or use a :free model." -ForegroundColor Yellow }
  if ($status -eq 404) { Write-Host "   404 means the model id does not exist. Check openrouter.ai/models for the exact id." -ForegroundColor Yellow }
  if ($status -eq 429) { Write-Host "   429 means rate limited. Wait a minute and try again, or try a different free model." -ForegroundColor Yellow }
  exit 1
}

Write-Host ""
Write-Host "3) A few cheap and free models you can try as the [model-id] argument:" -ForegroundColor Cyan
"   nex-agi/nex-n2.5-mini:free", "   liquid/lfm-2.5-2.6b:free", "   inclusionai/ling-3.0-flash-sante:free", "   deepseek/deepseek-v4-flash-0731", "   z-ai/glm-5.3-flash", "   qwen/qwen3.8-flash", "   google/gemini-3.8-flash" | ForEach-Object { Write-Host $_ }
$key = $null
