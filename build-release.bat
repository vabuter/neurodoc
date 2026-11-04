@echo off
rem ============================================
rem  Neurodoc Editor - Release build (x64)
rem ============================================
setlocal
cd /d "%~dp0windows"

dotnet build NeurodocEditor\NeurodocEditor.csproj -c Release -p:Platform=x64
if errorlevel 1 (
    echo.
    echo *** BUILD FAILED ***
    pause
    exit /b 1
)

echo.
echo Build succeeded. Executable:
echo %~dp0windows\NeurodocEditor\bin\x64\Release\net10.0-windows10.0.22621.0\win-x64\NeurodocEditor.exe
endlocal
