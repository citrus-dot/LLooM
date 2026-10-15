@echo off
rem LLooM stopper (portable zip). Prefers the graceful /api/shutdown path (the
rem server also cleans up any Ollama it spawned); falls back to taskkill only
rem if HTTP is unreachable or the graceful path times out.
rem NOTE: keep this file ASCII-only (cmd parses scripts under the OEM codepage).
setlocal
cd /d "%~dp0"

set "PORT=7861"
if exist ".env" (
  for /f "usebackq tokens=1,* delims==" %%A in (".env") do (
    if /i "%%A"=="LLOOM_WEB_PORT" set "PORT=%%B"
  )
)
set "URL=http://localhost:%PORT%"

curl -sf -m 2 -o nul "%URL%/api/health"
if errorlevel 1 (
  echo LLooM is not running (nothing at %URL%)
  exit /b 0
)

echo Requesting graceful shutdown ...
curl -s -m 5 -X POST "%URL%/api/shutdown" >nul 2>&1

rem Give the graceful path up to ~10s (checked every 0.5s via ping).
ping -n 2 127.0.0.1 >nul
curl -sf -m 1 -o nul "%URL%/api/health"
if errorlevel 1 (
  echo LLooM stopped.
  exit /b 0
)

echo Graceful shutdown timed out, killing by image name ...
taskkill /IM lloom-server.exe /T /F >nul 2>&1
ping -n 3 127.0.0.1 >nul
curl -sf -m 1 -o nul "%URL%/api/health"
if errorlevel 1 (
  echo LLooM stopped ^(forced^).
  exit /b 0
)
echo Failed to stop LLooM; please end lloom-server.exe manually.
exit /b 1
