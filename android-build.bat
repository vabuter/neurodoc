@echo off
rem ============================================
rem  Neurodoc Editor for Android - debug build
rem ============================================
setlocal
cd /d "%~dp0android"

call "%~dp0android\gradlew.bat" assembleDebug
if errorlevel 1 (
    echo.
    echo *** BUILD FAILED ***
    pause
    exit /b 1
)

echo.
echo Build succeeded. APK:
echo %~dp0android\app\build\outputs\apk\debug\app-debug.apk
endlocal
