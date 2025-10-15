@echo off
setlocal enabledelayedexpansion


REM set SOFTNODE_API_URL_BASE=https://softnode.styxheim.ru/api/softnode
REM cargo run -p softnode-client

REM cargo build --all
REM IF ERRORLEVEL 1 (
REM     exit /b 1
REM )

cd softnode-client
trunk build --release --dist ..\web
IF ERRORLEVEL 1 (
    exit /b 1
)
