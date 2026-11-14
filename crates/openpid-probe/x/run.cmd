@echo off
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0capture.ps1" -Steps "steps.json"
echo.
pause
