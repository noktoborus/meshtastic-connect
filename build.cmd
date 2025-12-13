@echo off
setlocal enabledelayedexpansion


set SOFTNODE_API_URL_BASE=https://softnode.styxheim.ru/api/softnode
cargo run -p softnode-client

REM cargo build --all
REM IF ERRORLEVEL 1 (
REM     exit /b 1
REM )

REM cd softnode-client
REM trunk build --release --dist ..\web
REM IF ERRORLEVEL 1 (
REM     exit /b 1
REM )
