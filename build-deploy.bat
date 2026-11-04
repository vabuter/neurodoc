@echo off
rem ==========================================================
rem  Neurodoc Editor - Release build + deploy to a folder
rem  The deployed folder is self-contained (no .NET required).
rem ==========================================================

rem Target folder (change if needed):
set "DEPLOY_DIR=C:\d\deployments\neurodoc_editor"

setlocal
cd /d "%~dp0windows"

rem --- Guard: a running instance locks NeurodocEditor.exe/.dll. Without this
rem --- check the cleanup below silently leaves the old executable in place and
rem --- publish writes fresh dependencies around it - a broken mixed deployment.
tasklist /fi "IMAGENAME eq NeurodocEditor.exe" 2>nul | find /i "NeurodocEditor.exe" >nul
if not errorlevel 1 (
    echo.
    echo *** NeurodocEditor.exe IS RUNNING ***
    echo Close the application and run this script again,
    echo otherwise the deployed copy would mix new files with the old executable.
    pause
    exit /b 1
)

echo Publishing Release (x64, self-contained) to "%DEPLOY_DIR%" ...
echo.

rem Safe cleanup: remove the folder only if it looks like a previous deploy
if exist "%DEPLOY_DIR%\NeurodocEditor.exe" rmdir /s /q "%DEPLOY_DIR%"

rem --- Verify the cleanup really happened: a leftover executable means some
rem --- file is still locked, and publishing on top would corrupt the deploy.
if exist "%DEPLOY_DIR%\NeurodocEditor.exe" (
    echo.
    echo *** CLEANUP FAILED: "%DEPLOY_DIR%" still holds NeurodocEditor.exe ***
    echo Some file is locked. Close anything using the deployed folder and retry.
    pause
    exit /b 1
)

if not exist "%DEPLOY_DIR%" mkdir "%DEPLOY_DIR%"

dotnet publish NeurodocEditor\NeurodocEditor.csproj -c Release -p:Platform=x64 -r win-x64 --self-contained true -o "%DEPLOY_DIR%"
if errorlevel 1 (
    echo.
    echo *** PUBLISH FAILED ***
    pause
    exit /b 1
)

rem --- Final sanity check: the executable must actually be there.
if not exist "%DEPLOY_DIR%\NeurodocEditor.exe" (
    echo.
    echo *** PUBLISH REPORTED SUCCESS BUT NeurodocEditor.exe IS MISSING ***
    pause
    exit /b 1
)

echo.
echo Deploy finished. Run the app from:
echo %DEPLOY_DIR%\NeurodocEditor.exe
endlocal
