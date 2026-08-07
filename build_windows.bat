@echo off
rem ============================================================
rem hostswitch Windows build script (native MSVC on Windows)
rem
rem Usage:
rem   build_windows.bat          build release exe into dist\
rem   build_windows.bat sign     build then Authenticode-sign
rem   build_windows.bat clean    remove packaging artifacts
rem
rem Needs: Rust msvc toolchain + VS Build Tools with C++ tools
rem Unified output dir (same as macOS): dist\
rem ============================================================

setlocal EnableExtensions
set "APP_NAME=hostswitch"
rem Native host build puts cargo output here. Do NOT use target\<triple>\...
rem unless you also pass --target to cargo.
set "EXE=target\release\%APP_NAME%.exe"
set "DIST=dist"
set "OUT_EXE=%DIST%\%APP_NAME%.exe"
set "PFX=cert\hostswitch.pfx"

cd /d "%~dp0" || goto :fail

if /i "%~1"=="clean" goto :clean

call :ensure_path
where cargo >nul 2>&1
if errorlevel 1 (
    echo error: cargo not found. Install Rust from https://rustup.rs
    echo        Or open "x64 Native Tools Command Prompt" and re-run.
    goto :fail
)

call :ensure_msvc
if errorlevel 1 goto :fail

echo ==^> rustc / cargo
rustc --version
cargo --version

echo ==^> cargo build --release
cargo build --release
if errorlevel 1 (
    echo error: cargo build failed
    goto :fail
)

if not exist "%EXE%" (
    echo error: build finished but exe missing: %CD%\%EXE%
    goto :fail
)

if not exist "%DIST%" mkdir "%DIST%"
copy /y "%EXE%" "%OUT_EXE%" >nul
if errorlevel 1 (
    echo error: failed to copy to %DIST%\
    goto :fail
)

if /i "%~1"=="sign" goto :sign

call :print_success
goto :end_ok

:clean
if exist "%DIST%" rmdir /s /q "%DIST%"
if exist "%EXE%" del /q "%EXE%"
echo.
echo cleaned packaging artifacts under: %CD%\%DIST%
echo.
goto :end_ok

:sign
if not exist "%PFX%" (
    echo error: %PFX% not found
    goto :fail
)
if "%PFX_PASS%"=="" (
    echo error: set PFX_PASS to the pfx password
    goto :fail
)
where signtool >nul 2>&1
if errorlevel 1 (
    echo error: signtool not found. Install Windows SDK.
    goto :fail
)
signtool sign /f "%PFX%" /p "%PFX_PASS%" /fd SHA256 /t http://timestamp.digicert.com "%OUT_EXE%"
if errorlevel 1 goto :fail
signtool verify /pa "%OUT_EXE%"
if errorlevel 1 goto :fail
call :print_success
echo note: Authenticode signature applied.
goto :end_ok

:print_success
echo.
echo ========================================
echo  Build succeeded.
echo  Output directory:
echo    %CD%\%DIST%
echo  Artifacts:
echo    %CD%\%OUT_EXE%
echo ========================================
echo.
exit /b 0

:end_ok
echo Press any key to close this window...
pause >nul
exit /b 0

:fail
echo.
echo Build failed. See messages above.
echo.
echo Press any key to close this window...
pause >nul
exit /b 1

:ensure_path
for /f "tokens=2*" %%A in ('reg query "HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment" /v Path 2^>nul') do set "MACHINE_PATH=%%B"
for /f "tokens=2*" %%A in ('reg query "HKCU\Environment" /v Path 2^>nul') do set "USER_PATH=%%B"
if defined MACHINE_PATH set "PATH=%MACHINE_PATH%;%PATH%"
if defined USER_PATH set "PATH=%USER_PATH%;%PATH%"
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
exit /b 0

:ensure_msvc
where link >nul 2>&1
if not errorlevel 1 (
    where cl >nul 2>&1
    if not errorlevel 1 (
        echo ==^> MSVC already on PATH
        exit /b 0
    )
)

set "VCVARS="
if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" (
    for /f "usebackq delims=" %%I in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find VC\Auxiliary\Build\vcvars64.bat 2^>nul`) do (
        if exist "%%~I" set "VCVARS=%%~I"
    )
)
if not defined VCVARS if exist "C:\BuildTools\VC\Auxiliary\Build\vcvars64.bat" set "VCVARS=C:\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if not defined VCVARS if exist "%ProgramFiles%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" set "VCVARS=%ProgramFiles%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if not defined VCVARS if exist "%ProgramFiles%\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" set "VCVARS=%ProgramFiles%\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
if not defined VCVARS if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" set "VCVARS=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if not defined VCVARS if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat" set "VCVARS=%ProgramFiles(x86)%\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat"

if not defined VCVARS (
    echo error: MSVC not found. Install VS Build Tools with C++ workload,
    echo        or open "x64 Native Tools Command Prompt" and re-run.
    exit /b 1
)

echo ==^> loading MSVC: %VCVARS%
call "%VCVARS%" >nul
if errorlevel 1 (
    echo error: failed to run vcvars64.bat
    exit /b 1
)
where link >nul 2>&1
if errorlevel 1 (
    echo error: link.exe still not on PATH after vcvars
    exit /b 1
)
exit /b 0
