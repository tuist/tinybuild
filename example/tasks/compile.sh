#!/usr/bin/env bash
# tinybuild input Sources/main.swift
# tinybuild output build/MyApp
# tinybuild env CONFIGURATION
set -euo pipefail

mkdir -p build
flags="-Onone"
[ "${CONFIGURATION:-debug}" = "release" ] && flags="-O"

swiftc $flags Sources/main.swift -o build/MyApp
echo "compiled build/MyApp ($flags)"
