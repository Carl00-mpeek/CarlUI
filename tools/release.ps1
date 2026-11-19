# Yeni sürüm yayınlar: sürümü artırır, Cargo.lock'u günceller, testleri
# çalıştırır, commit'ler, vX.Y.Z etiketini push eder. Kurucuyu GitHub Actions
# (.github/workflows/release.yml) derler ve sürümü yayınlar.
#
# Sürüm kuralı (docs/GELISTIRME-NOTLARI.md → Sürüm numaraları): her hane 0-9.
# Son hane bir artar, 9'u geçince sıfırlanır ve soldaki hane artar:
#   1.1.8 → 1.1.9 → 1.2.0,   1.9.9 → 2.0.0
#
# Önce docs/release-notes.md yazılmış olmalı: içinde "CarlUI X.Y.Z" geçmezse
# (eski sürümün notları kalmışsa) betik durur.
#
# Kullanım:
#   powershell -ExecutionPolicy Bypass -File tools\release.ps1            sonraki sürüm
#   powershell -ExecutionPolicy Bypass -File tools\release.ps1 -Version 2.0.0
#   powershell -ExecutionPolicy Bypass -File tools\release.ps1 -DryRun    yalnızca göster
#   -NoTest  testleri atlar

param([string]$Version = '', [switch]$DryRun, [switch]$NoTest)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

function Get-NextVersion([string]$cur) {
    $p = $cur.Split('.') | ForEach-Object { [int]$_ }
    if ($p.Count -ne 3) { throw "Sürüm X.Y.Z olmalı: $cur" }
    $p[2]++
    if ($p[2] -gt 9) { $p[2] = 0; $p[1]++ }
    if ($p[1] -gt 9) { $p[1] = 0; $p[0]++ }
    return ($p -join '.')
}

$toml = Join-Path $root 'Cargo.toml'
$text = [IO.File]::ReadAllText($toml)
$m = [regex]::Match($text, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $m.Success) { throw 'Cargo.toml içinde sürüm bulunamadı' }
$cur = $m.Groups[1].Value
if ($Version -eq '') { $Version = Get-NextVersion $cur }
if ($Version -notmatch '^\d\.\d\.\d$') { throw "Sürüm kurala uymuyor (her hane 0-9): $Version" }
$tag = "v$Version"
Write-Host "Sürüm: $cur → $Version ($tag)"

# Ön denetimler.
$branch = (git rev-parse --abbrev-ref HEAD).Trim()
if ($branch -ne 'main') { throw "main dalında olunmalı (şu an: $branch)" }
if (git tag --list $tag) { throw "$tag etiketi zaten var" }
$dirty = git status --porcelain | Where-Object { $_ -notmatch 'docs/release-notes\.md$' }
if ($dirty) { throw "Commit'lenmemiş değişiklik var:`n$($dirty -join "`n")" }
$notes = Join-Path $root 'docs\release-notes.md'
if (-not (Test-Path $notes)) { throw 'docs\release-notes.md yok' }
$body = Get-Content $notes -Raw -Encoding utf8
if ($body -notmatch [regex]::Escape("CarlUI $Version")) { throw "docs\release-notes.md içinde 'CarlUI $Version' geçmiyor: önce bu sürümün notlarını yaz" }

if ($DryRun) { Write-Host 'Deneme: hiçbir şey değiştirilmedi.'; return }

# Sürümü yaz (yalnızca ilk eşleşme: [workspace.package]).
$text = $text.Substring(0, $m.Groups[1].Index) + $Version + $text.Substring($m.Groups[1].Index + $m.Groups[1].Length)
[IO.File]::WriteAllText($toml, $text, (New-Object Text.UTF8Encoding $false))
cargo update -w
if ($LASTEXITCODE -ne 0) { throw 'cargo update başarısız' }

if (-not $NoTest) {
    Write-Host 'Testler çalışıyor…'
    cargo test --release -q --workspace
    if ($LASTEXITCODE -ne 0) { throw 'testler başarısız: sürüm commit''lenmedi (Cargo.toml ve Cargo.lock değişti)' }
}

git add Cargo.toml Cargo.lock docs/release-notes.md
git commit -q -m "Sürüm $Version"
if ($LASTEXITCODE -ne 0) { throw 'commit başarısız' }
git tag -a $tag -m "CarlUI $tag"
git push -q origin HEAD
if ($LASTEXITCODE -ne 0) { throw 'push başarısız' }
git push -q origin $tag
if ($LASTEXITCODE -ne 0) { throw 'etiket push edilemedi' }

Write-Host "$tag push edildi. Derleme ve yayın GitHub Actions'ta (~7 dk):"
Write-Host "  gh run watch (gh run list -L 1 --workflow release.yml --json databaseId --jq '.[0].databaseId')"
