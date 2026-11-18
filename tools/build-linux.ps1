# Carl Vault'un Linux sürümünü Windows'ta derler ve paketler:
#   dist\Carl-Vault-<sürüm>-linux-x86_64.tar.gz
#
# Paketin içinde: carl-vault (x86_64, glibc 2.28 ve üstü), install.sh,
# .desktop dosyaları, README.md (docs\LINUX-VAULT.md) ve LICENSE.
# Linux'ta açıp `sh install.sh` ile kurulur.
#
# Gerekenler (bir kez):
#   rustup target add x86_64-unknown-linux-gnu
#   cargo install cargo-zigbuild --locked
#   zig 0.13: https://ziglang.org/download/ zip'ini aç; zig.exe PATH'te olsun
#   ya da klasörünü -Zig ile ver.
#
# Kullanım:  powershell -ExecutionPolicy Bypass -File tools\build-linux.ps1 [-Zig <klasör>]

param([string]$Zig = '')

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

if ($Zig) { $env:PATH = "$Zig;$env:PATH" }
if (-not (Get-Command zig -ErrorAction SilentlyContinue)) { throw 'zig bulunamadı: PATH''e ekle ya da -Zig <klasör> ver' }
if (-not (Get-Command cargo-zigbuild -ErrorAction SilentlyContinue)) { throw 'cargo-zigbuild yok: cargo install cargo-zigbuild --locked' }

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$target = Join-Path $root 'target\linux'

Write-Host '1/2  carl-vault Linux için derleniyor…'
# .2.28: daha eski glibc'li dağıtımlarda da çalışsın diye sembol sürümü sabitlenir.
cargo zigbuild --release -p carlui-vault-linux --target x86_64-unknown-linux-gnu.2.28 --target-dir $target
if ($LASTEXITCODE -ne 0) { throw 'Linux derlemesi başarısız' }

Write-Host '2/2  Paket hazırlanıyor…'
$name = "Carl-Vault-$version-linux-x86_64"
$stage = Join-Path $target "pkg\$name"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null
$pack = 'crates\carlui-vault-linux\packaging'
Copy-Item (Join-Path $target 'x86_64-unknown-linux-gnu\release\carl-vault') $stage
Copy-Item "$pack\install.sh", "$pack\carl-vault.desktop", "$pack\carl-vault-autostart.desktop", LICENSE $stage
Copy-Item docs\LINUX-VAULT.md (Join-Path $stage 'README.md')

New-Item -ItemType Directory -Force dist | Out-Null
$out = Join-Path $root "dist\$name.tar.gz"
if (Test-Path $out) { Remove-Item $out -Force }
# Windows'un tar.exe'si (bsdtar). Çalıştırma izni install.sh'deki install -m755 ile verilir.
tar -czf $out -C (Split-Path $stage) $name
if ($LASTEXITCODE -ne 0) { throw 'tar başarısız' }

$hash = (Get-FileHash $out -Algorithm SHA256).Hash.ToLower()
Set-Content -Path "$out.sha256" -Value "$hash  $name.tar.gz" -Encoding ascii -NoNewline
Write-Host "Hazır: $out"
Write-Host "SHA256: $hash"
