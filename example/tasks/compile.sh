#!/usr/bin/env bash
# tinybuild input Sources/main.swift
# tinybuild output build/MyApp
# tinybuild env CONFIGURATION
set -euo pipefail

mkdir -p build
flags="-Onone"
[ "${CONFIGURATION:-debug}" = "release" ] && flags="-O"

arch="$(uname -m)"
sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
swiftc $flags -sdk "$sdk" -target "${arch}-apple-ios16.0-simulator" Sources/main.swift -o build/MyApp

echo "compiled build/MyApp for the iOS Simulator ($flags)"
