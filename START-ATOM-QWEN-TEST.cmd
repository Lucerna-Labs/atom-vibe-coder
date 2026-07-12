@echo off
setlocal
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Launch-Native-Qwen-LMStudio.ps1" -Build
if errorlevel 1 (
  echo.
  echo Qwen test launch failed. Confirm LM Studio is running with qwen3.5-9b@q6_k_xl loaded.
  pause
  exit /b 1
)
endlocal
