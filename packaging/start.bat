@echo off
rem LLooM launcher (portable zip). Runs in the foreground; the server cleans
rem up its child processes on Ctrl+C. Port comes from .env (LLOOM_WEB_PORT),
rem defaulting to 7861 -- same source of truth as the server itself.
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

rem Already running? Just (re)open the browser instead of failing to bind.
curl -sf -m 2 -o nul "%URL%/api/health"
if not errorlevel 1 (
  echo LLooM already running: %URL%
  start "" "%URL%"
  exit /b 0
)

echo Starting LLooM on %URL% ...
lloom-server.exe --open
