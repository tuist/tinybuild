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
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp build/MyApp "$app/Contents/MacOS/MyApp"
cp build/message.txt "$app/Contents/Resources/message.txt"
cp Info.plist "$app/Contents/Info.plist"

echo "assembled $app"
