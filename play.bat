@echo off
rem Launcher for the MGS2 Tanker rebuild: builds (release) and opens the
rem start menu (New game / Continue / Stage select).
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

cargo run --release -p tanker -- --title
endlocal
