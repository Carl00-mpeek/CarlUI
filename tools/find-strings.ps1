# Çevrilecek metinleri listeler: yorum, log ve zaten t!() içindeki satırlar hariç,
# harf içeren dize sabiti olan satırlar. Çeviri işinde yardımcıdır.
#
# Kullanım:  powershell -ExecutionPolicy Bypass -File tools\find-strings.ps1 crates\carlui-dock

param([Parameter(Mandatory)][string]$Path, [switch]$TurkishOnly)

$utf8 = New-Object System.Text.UTF8Encoding $false
[Console]::OutputEncoding = $utf8
# Türkçe harfler kod numarasıyla: PowerShell 5.1 BOM'suz betiği ANSI okur.
$tr = (-join ([char[]](0xE7, 0x11F, 0x131, 0xF6, 0x15F, 0xFC, 0xC7, 0x11E, 0x130, 0xD6, 0x15E, 0xDC)))
$rx = if ($TurkishOnly) { "`"[^`"]*[$tr][^`"]*`"" } else { "`"[^`"]*[A-Za-z$tr]{3,}[^`"]*`"" }
$skip = '^\s*(//|#\[)|\b(info|warn|error|debug|trace|log)!\(|\bt!\(|w!\("[A-Za-z.\\]+"\)|key\(|tween::|assert|starts_with\("http|\.ends_with\("\.|include_bytes'
Get-ChildItem $Path -Recurse -Filter *.rs | Where-Object { $_.FullName -notmatch '\\examples\\' -and $_.Name -ne 'build.rs' } | ForEach-Object {
    $rel = $_.FullName.Substring((Get-Location).Path.Length + 1)
    $n = 0
    $inTests = $false
    foreach ($l in [IO.File]::ReadAllLines($_.FullName, $utf8)) {
        $n++
        if ($l -match '^\s*#\[cfg\(test\)\]') { $inTests = $true }
        if ($inTests) { continue }
        if ($l -match $skip) { continue }
        if ($l -match $rx) { "${rel}:${n}: $($l.Trim())" }
    }
}
