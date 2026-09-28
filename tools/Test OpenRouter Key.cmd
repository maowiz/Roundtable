@echo off
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0test-openrouter.ps1" %*
pause
