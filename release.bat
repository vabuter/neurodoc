@echo off
rem ============================================================
rem  Neurodoc Editor - tag a release commit on main as vX.Y.Z
rem
rem  The tag is the single source of the version number: the Windows,
rem  Linux and Android builds all read it via "git describe".
rem  Nothing is pushed - publishing stays an explicit user action.
rem ============================================================
setlocal
cd /d "%~dp0"

set "VERSION=%~1"

if "%VERSION%"=="" (
    echo Usage:   release.bat X.Y.Z
    echo Example: release.bat 0.2.0
    exit /b 1
)

echo %VERSION%|findstr /r /c:"^[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*$" >nul
if errorlevel 1 (
    echo.
    echo *** BAD VERSION: "%VERSION%" ***
    echo Expected three numbers separated by dots, for example 0.2.0
    exit /b 1
)

rem --- Releases are tagged on main. A tag on another branch would name a
rem --- version that never reaches the released history.
set "BRANCH="
for /f "delims=" %%b in ('git rev-parse --abbrev-ref HEAD 2^>nul') do set "BRANCH=%%b"
if not "%BRANCH%"=="main" (
    echo.
    echo *** WRONG BRANCH: "%BRANCH%" ***
    echo Releases are tagged on main. Switch with: git switch main
    exit /b 1
)

rem --- The tree must be clean: otherwise the tag points at a commit that does
rem --- not match what will be built, and every build reports a "-dirty" version.
for /f "delims=" %%s in ('git status --porcelain') do (
    echo.
    echo *** WORKING TREE IS NOT CLEAN ***
    echo Commit or stash your changes first, then run this script again.
    exit /b 1
)

git rev-parse -q --verify "refs/tags/v%VERSION%" >nul
if not errorlevel 1 (
    echo.
    echo *** TAG v%VERSION% ALREADY EXISTS ***
    echo Tags are never moved. Pick a higher version number.
    exit /b 1
)

git tag -a "v%VERSION%" -m "v%VERSION%"
if errorlevel 1 (
    echo.
    echo *** FAILED TO CREATE TAG ***
    exit /b 1
)

echo.
echo Tagged v%VERSION% at:
git --no-pager log -1 --oneline
echo.
echo Nothing has been pushed yet. To publish the release run:
echo     git push origin main --follow-tags
echo.
echo Then rebuild to stamp the new version into the binaries:
echo     build-deploy.bat
echo     android-build.bat
echo     ./linux-deploy.sh    (on Linux)
endlocal
