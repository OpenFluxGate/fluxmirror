@echo off
rem FluxMirror cross-shell hook entry point (Windows cmd variant).
rem
rem Usage:
rem   shim.cmd <kind>                # kind = claude | gemini
rem
rem Strategy: prefer node (shim.mjs has identical logic and cross-platform
rem download handling). Fall back to PowerShell if node is missing.
rem Always exits with code 0 — must never break the calling agent.

setlocal

set "KIND=%~1"
if "%KIND%"=="" set "KIND=%FLUXMIRROR_KIND%"
if "%KIND%"=="" set "KIND=claude"

where node >NUL 2>&1
if not errorlevel 1 (
    node "%~dp0shim.mjs" "%KIND%"
    endlocal
    exit /b 0
)

set "CACHE=%FLUXMIRROR_CACHE%"
if "%CACHE%"=="" set "CACHE=%LOCALAPPDATA%\fluxmirror\cache"
if "%CACHE%"=="" set "CACHE=%USERPROFILE%\fluxmirror\cache"

if not exist "%CACHE%" mkdir "%CACHE%" >NUL 2>&1

set "ARCH=x64"
if /I "%PROCESSOR_ARCHITECTURE%"=="ARM64" set "ARCH=arm64"
if /I "%PROCESSOR_ARCHITEW6432%"=="ARM64" set "ARCH=arm64"

set "ASSET=fluxmirror-windows-%ARCH%.exe"
set "BIN=%CACHE%\%ASSET%"

if not exist "%BIN%" (
    powershell -NoProfile -ExecutionPolicy Bypass -Command ^
      "$ErrorActionPreference='SilentlyContinue';" ^
      "$url='https://github.com/OpenFluxGate/fluxmirror/releases/latest/download/%ASSET%';" ^
      "try { Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile '%BIN%.tmp' -TimeoutSec 15;" ^
      "Move-Item -Force '%BIN%.tmp' '%BIN%' } catch { Remove-Item -Force '%BIN%.tmp' -ErrorAction SilentlyContinue }"
)

if exist "%BIN%" (
    "%BIN%" hook --kind %KIND%
)

endlocal
exit /b 0
