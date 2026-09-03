@echo off
rem LLooM installer packaging for Windows.
rem Prereqs: target\release\lloom-server.exe, webui\dist\, dist\ai-service\ (PyInstaller onedir)
rem Output:  dist\LLooM-<version>-windows-x86_64.zip
rem NOTE: keep this file ASCII-only (cmd parses scripts under the OEM codepage).
setlocal
cd /d "%~dp0.."

set "ARCH=x86_64"
set "VERSION=dev"
for /f "delims=" %%i in ('git describe --tags --always 2^>nul') do set "VERSION=%%i"
set "STAGE=dist\pkg\LLooM"
set "OUT=dist\LLooM-%VERSION%-windows-%ARCH%.zip"

if not exist "target\release\lloom-server.exe" echo missing: target\release\lloom-server.exe & exit /b 1
if not exist "webui\dist\index.html" echo missing: webui\dist\index.html & exit /b 1
if not exist "dist\ai-service" echo missing: dist\ai-service & exit /b 1

rmdir /s /q "dist\pkg" 2>nul
mkdir "%STAGE%\resources\webui" || exit /b 1
mkdir "%STAGE%\resources\ai-service" || exit /b 1
mkdir "%STAGE%\scripts" || exit /b 1

copy /y "target\release\lloom-server.exe" "%STAGE%\" >nul || exit /b 1
if exist "target\release\lloom-cli.exe" copy /y "target\release\lloom-cli.exe" "%STAGE%\" >nul
robocopy "webui\dist" "%STAGE%\resources\webui\dist" /E /NFL /NDL /NJH /NJS /NP >nul
if errorlevel 8 exit /b 1
robocopy "dist\ai-service" "%STAGE%\resources\ai-service" /E /NFL /NDL /NJH /NJS /NP >nul
if errorlevel 8 exit /b 1
copy /y ".env.example" "%STAGE%\" >nul || exit /b 1
copy /y "scripts\aiq_replay.py" "%STAGE%\scripts\" >nul || exit /b 1

(
  echo @echo off
  echo cd /d %%~dp0
  echo lloom-server.exe
) > "%STAGE%\start.bat"

tar -a -c -f "%OUT%" -C "dist\pkg" "LLooM" || exit /b 1
rmdir /s /q "dist\pkg"

for %%A in ("%OUT%") do echo OK installer: %OUT% %%~zA bytes
exit /b 0
