@echo off
rem =====================================================
rem  Neurodoc Editor for Android - build and install
rem  Requires a device connected over USB (adb devices).
rem =====================================================
setlocal
cd /d "%~dp0android"

call "%~dp0android\gradlew.bat" assembleDebug
if errorlevel 1 (
    echo.
    echo *** BUILD FAILED ***
    pause
    exit /b 1
)

set "ADB=%LOCALAPPDATA%\Android\Sdk\platform-tools\adb.exe"
"%ADB%" install -r app\build\outputs\apk\debug\app-debug.apk
if errorlevel 1 (
    echo.
    echo *** INSTALL FAILED ***
    pause
    exit /b 1
)

rem All-files access is needed to read neurodoc catalogs from shared storage
"%ADB%" shell appops set --uid today.vab.neurodoceditor MANAGE_EXTERNAL_STORAGE allow
"%ADB%" shell am start -n today.vab.neurodoceditor/.MainActivity

echo.
echo Installed and started on the device.
endlocal
