@echo off
setlocal EnableExtensions
cd /d "%~dp0"
title YuE2 Studio - Local Portable Builder

rem Rust's Windows MSVC target needs link.exe and the Windows SDK in PATH.
rem The normal Windows command prompt does not initialize these automatically.
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" goto :missing_cpp

set "VSROOT="
for /f "usebackq delims=" %%I in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSROOT=%%I"
if not defined VSROOT goto :missing_cpp

set "VSDEVCMD=%VSROOT%\Common7\Tools\VsDevCmd.bat"
if not exist "%VSDEVCMD%" goto :missing_cpp

echo [YuE2] Activating Visual Studio 2022 x64 C++ build tools...
call "%VSDEVCMD%" -no_logo -arch=x64 -host_arch=x64
if errorlevel 1 goto :init_failed

where link.exe >nul 2>nul
if errorlevel 1 goto :missing_linker

echo [YuE2] MSVC linker available:
where link.exe
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0BUILD-YUE2-LOCAL.ps1"
exit /b %ERRORLEVEL%

:missing_cpp
echo.
echo [ERROR] Visual Studio C++ Build Tools were not detected.
echo Open Visual Studio Installer and install "Desktop development with C++".
echo Required component: MSVC v143 x64/x86 build tools.
echo.
pause
exit /b 1

:init_failed
echo.
echo [ERROR] Visual Studio's x64 developer environment could not initialize.
echo Try repairing the C++ workload using Visual Studio Installer.
echo.
pause
exit /b 1

:missing_linker
echo.
echo [ERROR] MSVC link.exe is still unavailable after activating the C++ tools.
echo In Visual Studio Installer, check MSVC v143 x64/x86 build tools.
echo.
pause
exit /b 1
