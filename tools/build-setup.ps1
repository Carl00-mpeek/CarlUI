# Tek dosyalık kurucuyu üretir: dist\CarlUI-Setup.exe
#
# Exe'ler ayrı bir hedef klasörde (target\dist) derlenir; böylece çalışan
# CarlUI'nin kilitlediği target\release'e dokunulmaz, kabuğu kapatmak gerekmez.
# Mozilla'nın imzaladığı extension\carlui-downloads.xpi varsa kurucuya eklenir
# ve kurulumda Firefox'a otomatik tanıtılır.
#
# Güncellemeden sonraki Yenilikler ekranı için sürüm notu (GitHub sürüm notuyla
# aynı metin, "## Türkçe" … "---" … "## English") docs\release-notes.md'den
# (yoksa eski konum dist\release-notes.md'den) kurucuya gömülür; dosya yoksa
# ekran çıkmaz.
#
# Kullanım:  powershell -ExecutionPolicy Bypass -File tools\build-setup.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$target = Join-Path $root 'target\dist'
$crates = 'carlui', 'carlui-dock', 'carlui-tray', 'carlui-island', 'carlui-start', 'carlui-settings', 'carlui-taskmgr', 'carlui-convert', 'carlui-sensors', 'carlui-vault', 'carlui-remover', 'carlui-office', 'carlui-tools', 'carlui-expand', 'carlui-replay'
$pkgs = $crates | ForEach-Object { '-p'; $_ }

Write-Host '1/3  CarlUI exe dosyaları derleniyor…'
cargo build --release --target-dir $target @pkgs
if ($LASTEXITCODE -ne 0) { throw 'exe derlemesi başarısız' }
$env:CARLUI_PAYLOAD_DIR = Join-Path $target 'release'

# Kaldırıcı: kurucunun dosya gömmeyen küçük derlemesi (yalnızca kaldırır).
Write-Host '2/3  Kaldırıcı derleniyor…'
cargo build --release --target-dir $target -p carlui-setup --features uninstaller
if ($LASTEXITCODE -ne 0) { throw 'kaldırıcı derlemesi başarısız' }
$uninstaller = Join-Path $target 'CarlUI-Uninstall.exe'
Copy-Item (Join-Path $target 'release\CarlUI-Setup.exe') $uninstaller -Force

if (-not ((Test-Path (Join-Path $root 'docs\release-notes.md')) -or (Test-Path (Join-Path $root 'dist\release-notes.md')))) { Write-Warning 'docs\release-notes.md yok: Yenilikler ekranı boş kalacak' }
Write-Host '3/3  Kurucu derleniyor (dosyalar sıkıştırılıyor)…'
$env:CARLUI_UNINSTALLER = $uninstaller
cargo build --release --target-dir $target -p carlui-setup
if ($LASTEXITCODE -ne 0) { throw 'kurucu derlemesi başarısız' }
Remove-Item Env:CARLUI_PAYLOAD_DIR, Env:CARLUI_UNINSTALLER

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item (Join-Path $target 'release\CarlUI-Setup.exe') $dist -Force
$size = [math]::Round((Get-Item (Join-Path $dist 'CarlUI-Setup.exe')).Length / 1MB, 1)
Write-Host "Hazır: dist\CarlUI-Setup.exe ($size MB)"
