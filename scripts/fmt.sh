#!/usr/bin/env bash
# Apply formatting to all projects
set -e

cd "$(dirname "${BASH_SOURCE[0]}")/.."
cargo fmt --all
cd rukey_firmware
cargo fmt --all