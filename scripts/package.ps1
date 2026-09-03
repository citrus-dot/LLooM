# LLooM 安装包组装脚本（Windows）
#
# 前置产物（CI 或手动执行 build 步骤）：
#   target/release/lloom-server.exe
#   target/release/lloom-cli.exe            （可选，缺失则跳过）
#   webui/dist/                             （npm run build）
#   dist/ai-service/                        （PyInstaller onedir：ai-service.exe + _internal/）
#
# 产出：dist/LLooM-<version>-windows-<arch>.zip
# 目录布局对应 config::install_dir / ui_dir / processes::start_ai 的查找逻辑。
#
# 用法: powershell -File scripts/package.ps1
$ErrorActionPreference = "Stop"

$ProjectDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $ProjectDir

$Arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { "x86" }
$Version = (git describe --tags --always 2>$null)
if (-not $Version) { $Version = "dev" }
$Stage = "dist/pkg/LLooM"
$Out = "dist/LLooM-$Version-windows-$Arch.zip"

foreach ($f in @("target/release/lloom-server.exe", "webui/dist/index.html", "dist/ai-service")) {
  if (-not (Test-Path $f)) {
    Write-Error "缺少构建产物: $f （先跑 CI 或对应 build 步骤）"
    exit 1
  }
}

if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force -Path "$Stage/resources" | Out-Null

Copy-Item target/release/lloom-server.exe $Stage/
if (Test-Path target/release/lloom-cli.exe) { Copy-Item target/release/lloom-cli.exe $Stage/ }
New-Item -ItemType Directory -Force -Path "$Stage/resources/webui" | Out-Null
Copy-Item -Recurse webui/dist "$Stage/resources/webui/dist"
Copy-Item -Recurse dist/ai-service "$Stage/resources/ai-service"
New-Item -ItemType Directory -Force -Path "$Stage/scripts" | Out-Null
Copy-Item scripts/aiq_replay.py "$Stage/scripts/"   # N2 路由体检 job 按 install_dir/scripts 查找
Copy-Item .env.example $Stage/

# 不用 here-string：PS 5.1 对 LF 行尾脚本的 here-string 解析不可靠，
# 用 `r`n 显式写 CRLF，保证 start.bat 在任意检出配置下正确。
$bat = "@echo off`r`ncd /d %~dp0`r`nlloom-server.exe`r`n"
Set-Content -Path "$Stage/start.bat" -Value $bat -Encoding ascii

New-Item -ItemType Directory -Force -Path dist/pkg | Out-Null
Compress-Archive -Path $Stage -DestinationPath $Out -Force
Remove-Item -Recurse -Force $Stage

$size = "{0:N1} MB" -f ((Get-Item $Out).Length / 1MB)
Write-Host "OK 安装包: $Out ($size)"
