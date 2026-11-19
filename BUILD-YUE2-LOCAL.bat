@echo off
setlocal
cd /d "%~dp0"
title YuE2 Studio - Local Portable Builder
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0BUILD-YUE2-LOCAL.ps1"
