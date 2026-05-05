#!/usr/bin/env bash
# depdencies:
# cargo install -f cargo-upgrades --version 2.1.1
set -e

cd "$(dirname "${BASH_SOURCE[0]}")/.."

cargo upgrades
cd rukey_config_web_app
cargo upgrades
cd ../rukey_firmware
cargo upgrades
