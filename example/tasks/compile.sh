#!/usr/bin/env bash
# tinybuild input Sources/main.swift
# tinybuild output MyApp
# tinybuild env CONFIGURATION
# tinybuild tool swiftc --version
# tinybuild tool xcrun --sdk iphonesimulator --show-sdk-version
set -euo pipefail

flags="-Onone"
[ "${CONFIGURATION:-debug}" = "release" ] && flags="-O"

arch="$(uname -m)"
sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
swiftc $flags -sdk "$sdk" -target "${arch}-apple-ios16.0-simulator" Sources/main.swift -o MyApp
