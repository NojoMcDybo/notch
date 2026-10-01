# Schickt einen Blutzucker-Verlauf (Demo) an die Notch — zum Testen ohne Haze.
#   .\tools\bg-demo.ps1                 aktueller Wert, 24 h Verlauf, Trend + Aenderung
#   .\tools\bg-demo.ps1 -AgeMin 15      letzter Messwert 15 Min alt  -> grau + "vor 15 Min"
#   .\tools\bg-demo.ps1 -Ttl 20         ttl 20 s                     -> danach "keine Daten"
#   .\tools\bg-demo.ps1 -Remove         Eintrag loeschen
param([int]$AgeMin = 2, [int]$Ttl = 900, [switch]$Alert, [switch]$Remove)
$base = 'http://127.0.0.1:47800'
if ($Remove) { Invoke-RestMethod -Method Delete "$base/activity/haze:demo"; return }
$step = 300000
$end = [long]([math]::Floor(([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $AgeMin * 60000) / $step) * $step)
$pts = for ($i = 288; $i -ge 0; $i--) {
  $t = $end - $i * $step; $h = $t / 3600000.0
  $v = 125 + 45 * [math]::Sin($h * 2 * [math]::PI / 7.3) + 28 * [math]::Sin($h * 2 * [math]::PI / 2.9 + 1.3) + 14 * [math]::Sin($h * 2 * [math]::PI / 1.1)
  ,@($t, [math]::Round([math]::Max(45, [math]::Min(320, $v))))
}
$last = $pts[-1][1]; $prev = $pts[-2][1]; $ref = $pts[-4][1]
$rate = ($last - $ref) / 15.0; $a = [math]::Abs($rate); $s = if ($rate -gt 0) { 'up' } else { 'down' }
$trend = if ($a -lt 1) { 'flat' } elseif ($a -lt 2) { "${s}45" } elseif ($a -lt 3) { $s } else { "${s}2" }
$body = @{ id = 'haze:demo'; app = 'Haze'; title = 'Blutzucker (Notch-Test)'; value = $last; unit = 'mg/dL'
  trend = $trend; delta = $last - $prev; priority = 10; ttl = $Ttl; alert = [bool]$Alert
  chart = @{ low = 70; high = 180; points = $pts; ranges = @(3, 6, 12, 24); range = 3 } } | ConvertTo-Json -Depth 6 -Compress
Invoke-RestMethod -Method Post "$base/activity" -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($body))
"gesendet: $last $trend ($($last - $prev)), letzter Messwert vor $AgeMin Min, ttl $Ttl s"
