<#
.SYNOPSIS
    组装 Windows 便携版 Gesso（zip，不签名）。

.DESCRIPTION
    release 构建后，把 gesso.exe + 运行时资源（host 宿主页、samples 内置样例）
    组装成一个解压即用的便携 zip，并打印 SHA-256。图标已内嵌 exe（winresource），
    WebView2 使用系统 Evergreen Runtime（本脚本不打包）。

    兼容 Windows PowerShell 5.1 与 PowerShell 7+；只用内置 cmdlet，无第三方依赖。

.EXAMPLE
    packaging\windows\build_portable.ps1
    packaging\windows\build_portable.ps1 -SkipBuild   # 复用已有 release 二进制，只重新打包
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

# 仓库根（脚本在 packaging/windows/ 下，上两级）
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
Set-Location $root

# 版本号取自 workspace Cargo.toml
$cargo = Get-Content 'Cargo.toml' -Raw
if ($cargo -match '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"') {
    $version = $Matches[1]
} else {
    throw '无法从 Cargo.toml 解析版本号'
}

# 架构标记（zip 文件名用）
$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x64' }
    'ARM64' { 'arm64' }
    default { $env:PROCESSOR_ARCHITECTURE.ToLower() }
}

if (-not $SkipBuild) {
    Write-Host '>> cargo build --release -p gesso-app'
    cargo build --release -p gesso-app
    if ($LASTEXITCODE -ne 0) { throw 'release 构建失败' }
}

$exe = 'target\release\gesso.exe'
if (-not (Test-Path $exe)) {
    throw "找不到 $exe（先去掉 -SkipBuild 让脚本构建）"
}

$bundleDir = 'target\release-bundle'
$stage = Join-Path $bundleDir 'portable-staging'
$appDir = Join-Path $stage 'Gesso'   # zip 内顶层文件夹

Write-Host ">> 组装便携目录（版本 $version，$arch）"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path $appDir -Force | Out-Null

# 主程序
Copy-Item $exe (Join-Path $appDir 'gesso.exe')

# 运行时资源：assets_dir() 在 release 下定位 exe 同级 assets
$resAssets = Join-Path $appDir 'assets'
New-Item -ItemType Directory -Path $resAssets -Force | Out-Null
Copy-Item 'crates\app\assets\host'    (Join-Path $resAssets 'host')    -Recurse
Copy-Item 'crates\app\assets\samples' (Join-Path $resAssets 'samples') -Recurse

# 打成 zip（保留 Gesso 顶层文件夹，解压不散乱）
$zip = Join-Path $bundleDir "Gesso-$version-$arch-portable.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $appDir -DestinationPath $zip

Remove-Item $stage -Recurse -Force

$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
Write-Host ">> 完成：$zip"
Write-Host "   SHA-256: $hash"

# 安装器（NSIS 3，per-user，与 Tauri 项目同形态）：找到 makensis 就出
# setup exe；本地未装则跳过（GitHub Windows runner 预装 NSIS）。
$makensis = @(
    (Get-Command makensis -ErrorAction SilentlyContinue).Source,
    'C:\Program Files (x86)\NSIS\makensis.exe',
    'C:\Program Files\NSIS\makensis.exe'
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if ($makensis) {
    Write-Host ">> NSIS 打包安装器：$makensis"
    & $makensis "/DVERSION=$version" "/DARCH=$arch" 'packaging\windows\gesso.nsi'
    if ($LASTEXITCODE -ne 0) { throw 'NSIS 打包失败' }
    $setup = Join-Path $bundleDir "Gesso-$version-$arch-setup.exe"
    $hashSetup = (Get-FileHash -Algorithm SHA256 $setup).Hash.ToLower()
    Write-Host ">> 完成：$setup"
    Write-Host "   SHA-256: $hashSetup"
} else {
    Write-Host '>> 未找到 NSIS（makensis.exe），跳过安装器（仅便携 zip）'
}
