# Tek dosyalık kurucuyu üretir: dist\CarlUI-Setup.exe
#
# Exe'ler ayrı bir hedef klasörde (target\dist) derlenir; böylece çalışan
# CarlUI'nin kilitlediği target\release'e dokunulmaz, kabuğu kapatmak gerekmez.
# Mozilla'nın imzaladığı extension\carlui-downloads.xpi varsa kurucuya eklenir
# ve kurulumda Firefox'a otomatik tanıtılır.
#
# Kullanım:  powershell -ExecutionPolicy Bypass -File tools\build-setup.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$target = Join-Path $root 'target\dist'
$crates = 'carlui', 'carlui-dock', 'carlui-tray', 'carlui-island', 'carlui-start', 'carlui-settings', 'carlui-taskmgr', 'carlui-convert', 'carlui-sensors', 'carlui-vault'
$pkgs = $crates | ForEach-Object { '-p'; $_ }

Write-Host '1/2  CarlUI exe dosyaları derleniyor…'
cargo build --release --target-dir $target @pkgs
if ($LASTEXITCODE -ne 0) { throw 'exe derlemesi başarısız' }

Write-Host '2/2  Kurucu derleniyor…'
$env:CARLUI_PAYLOAD_DIR = Join-Path $target 'release'
cargo build --release --target-dir $target -p carlui-setup
if ($LASTEXITCODE -ne 0) { throw 'kurucu derlemesi başarısız' }
Remove-Item Env:CARLUI_PAYLOAD_DIR

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item (Join-Path $target 'release\CarlUI-Setup.exe') $dist -Force
$size = [math]::Round((Get-Item (Join-Path $dist 'CarlUI-Setup.exe')).Length / 1MB, 1)
Write-Host "Hazır: dist\CarlUI-Setup.exe ($size MB)"
