#!/usr/bin/env bash
# tinybuild needs compile
# tinybuild needs resource
# tinybuild input build/MyApp
# tinybuild input build/message.txt
# tinybuild input Info.plist
# tinybuild output build/MyApp.app
set -euo pipefail

app="build/MyApp.app"
rm -rf "$app"
mkdir -p "$app"

cp build/MyApp "$app/MyApp"
cp build/message.txt "$app/message.txt"
cp Info.plist "$app/Info.plist"

echo "assembled $app"
