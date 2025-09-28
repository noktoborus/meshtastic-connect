#!/bin/bash
set -e

cargo build --all

cd softnode-client
trunk build --release \
    --dist ../web
