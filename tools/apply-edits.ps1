# Toplu metin değişikliği: JSON dosyasındaki {f, old, new, all?} kayıtlarını
# uygular. Dosyalar UTF-8 (BOM'suz) okunur ve yazılır; Türkçe karakterler
# bozulmaz. `old` dosyada bulunmazsa ya da (all yoksa) birden çok kez geçerse
# o kayıt atlanır ve raporlanır.
#
# Kullanım:  powershell -ExecutionPolicy Bypass -File tools\apply-edits.ps1 edits.json

param([Parameter(Mandatory)][string]$Spec)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$utf8 = New-Object System.Text.UTF8Encoding $false
$edits = [IO.File]::ReadAllText($Spec, $utf8) | ConvertFrom-Json
$cache = @{}
$fail = 0
foreach ($e in $edits) {
    $path = Join-Path $root $e.f
    if (-not $cache.ContainsKey($path)) { $cache[$path] = [IO.File]::ReadAllText($path, $utf8) }
    $text = $cache[$path]
    $count = ([regex]::Matches($text, [regex]::Escape($e.old))).Count
    if ($count -eq 0 -or ($count -gt 1 -and -not $e.all)) {
        Write-Host "ATLANDI ($count): $($e.f): $($e.old)"
        $fail++
        continue
    }
    $cache[$path] = $text.Replace($e.old, $e.new)
}
foreach ($k in $cache.Keys) { [IO.File]::WriteAllText($k, $cache[$k], $utf8) }
Write-Host "Uygulandı: $($edits.Count - $fail) / $($edits.Count)"
