@echo off
setlocal EnableExtensions
cd /d "%~dp0"
title YuE2 Studio - Local Portable Builder

rem Visual Studio's MSVC linker is usually installed, but not on PATH in a
rem normal cmd.exe window. Prefer standard install locations; never rely on
rem FOR /F command-substitution quoting to find the user's Build Tools.
set "VSDEVCMD="

for %%Y in (2022 2026 2019) do (
  for %%E in (BuildTools Community Professional Enterprise) do (
    if not defined VSDEVCMD if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\%%Y\%%E\Common7\Tools\VsDevCmd.bat" (
      set "VSDEVCMD=%ProgramFiles(x86)%\Microsoft Visual Studio\%%Y\%%E\Common7\Tools\VsDevCmd.bat"
    )
  )
)

rem Other editions may live under Program Files rather than Program Files (x86).
if not defined VSDEVCMD (
  for %%Y in (2022 2026 2019) do (
    for %%E in (BuildTools Community Professional Enterprise) do (
      if not defined VSDEVCMD if exist "%ProgramFiles%\Microsoft Visual Studio\%%Y\%%E\Common7\Tools\VsDevCmd.bat" (
        set "VSDEVCMD=%ProgramFiles%\Microsoft Visual Studio\%%Y\%%E\Common7\Tools\VsDevCmd.bat"
      )
    )
  )
)

if not defined VSDEVCMD goto :missing_vs

echo [YuE2] Found Visual Studio C++ environment:
echo         "%VSDEVCMD%"
echo [YuE2] Activating MSVC x64 tools...
call "%VSDEVCMD%" -no_logo -arch=x64 -host_arch=x64
if errorlevel 1 goto :init_failed

where link.exe >nul 2>nul
if errorlevel 1 goto :missing_linker

echo [YuE2] MSVC linker available:
where link.exe
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0BUILD-YUE2-LOCAL.ps1"
exit /b %ERRORLEVEL%

:missing_vs
echo.
echo [ERROR] Could not locate VsDevCmd.bat in a standard Visual Studio folder.
echo Your previous checks detected Build Tools at:
echo   C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools
echo.
echo Check whether this file exists:
echo   C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat
echo If it was installed elsewhere, open the Visual Studio Installer to view its location.
echo.
pause
exit /b 1

:init_failed
echo.
echo [ERROR] Visual Studio's x64 developer environment did not initialize.
echo Check that the Desktop development with C++ workload is installed.
echo.
pause
exit /b 1

:missing_linker
echo.
echo [ERROR] Visual Studio was found, but link.exe is still missing.
echo In Visual Studio Installer, modify Build Tools and enable MSVC v143 x64/x86 build tools.
echo.
pause
exit /b 1
