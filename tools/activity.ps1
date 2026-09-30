# Legt eine Live Activity in die Notch (oder entfernt sie).
#   .\activity.ps1 -Id blutzucker -Title Blutzucker -Value 112 -Unit mg/dL -Icon "🩸" -Color "#5ee38a"
#   .\activity.ps1 -Id blutzucker -Remove
param(
  [Parameter(Mandatory)] [string]$Id,
  [string]$Title = $Id,
  [string]$App = "",
  [string]$Subtitle,
  [string]$Value,
  [string]$Unit,
  [string]$Icon,
  [string]$Color,
  [double]$Progress = -1,
  [int]$Ttl = 0,
  [int]$Priority = 0,
  [switch]$Alert,
  [switch]$Remove
)
$base = "http://127.0.0.1:47800"
if ($Remove) { Invoke-RestMethod -Method Delete "$base/activity/$Id"; return }

$body = @{ id = $Id; title = $Title; app = $App; priority = $Priority; alert = [bool]$Alert }
foreach ($k in 'Subtitle','Value','Unit','Icon','Color') {
  $v = Get-Variable $k -ValueOnly
  if ($v) { $body[$k.ToLower()] = $v }
}
if ($Progress -ge 0) { $body.progress = $Progress }
if ($Ttl -gt 0) { $body.ttl = $Ttl }
$json = $body | ConvertTo-Json -Compress
Invoke-RestMethod -Method Post "$base/activity" -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($json))
