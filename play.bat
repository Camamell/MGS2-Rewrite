@echo off
rem Launcher for the MGS2 Tanker rebuild: builds (release) and starts the game.
rem Needs MGS2_ISO set to your disc image, or you are asked for it once.
setlocal
cd /d "%~dp0"

if "%MGS2_ISO%"=="" (
  if exist "%APPDATA%\mgs2-tanker\iso.txt" set /p MGS2_ISO=<"%APPDATA%\mgs2-tanker\iso.txt"
)
if "%MGS2_ISO%"=="" (
  set /p MGS2_ISO=Path to your MGS2 .iso:
  if not exist "%APPDATA%\mgs2-tanker" mkdir "%APPDATA%\mgs2-tanker"
  call echo %%MGS2_ISO%%>"%APPDATA%\mgs2-tanker\iso.txt"
)

echo.
echo  METAL GEAR SOLID 2 - TANKER (rebuild)
echo.
echo   1. New game
echo   2. Continue (last autosave)
echo   3. Choose a stage
echo.
set /p CHOICE=Choose 1-3:

set ARGS=
if "%CHOICE%"=="2" set ARGS=--continue
if "%CHOICE%"=="3" (
  echo Stages: w00a w00b w00c w01a w01b w01c w01d w01e w01f w02a w03a w03b w04a w04b w04c
  set /p STAGE=Stage:
)
if "%CHOICE%"=="3" set ARGS=%STAGE%
if "%CHOICE%"=="1" (
  echo.
  echo   Difficulty: 1 Very Easy  2 Easy  3 Normal  4 Hard  5 Extreme
  set /p DIFF=Choose 1-5 [3]:
)
set DIFFARG=
if "%DIFF%"=="1" set DIFFARG=--difficulty 10
if "%DIFF%"=="2" set DIFFARG=--difficulty 20
if "%DIFF%"=="4" set DIFFARG=--difficulty 40
if "%DIFF%"=="5" set DIFFARG=--difficulty 50

cargo run --release -p tanker -- %ARGS% %DIFFARG%
endlocal
