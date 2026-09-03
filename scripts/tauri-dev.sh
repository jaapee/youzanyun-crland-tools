#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."
exec npm run tauri dev
