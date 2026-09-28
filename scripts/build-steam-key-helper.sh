#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
swiftc src/steam_api_key.swift -o swch_steam_key
