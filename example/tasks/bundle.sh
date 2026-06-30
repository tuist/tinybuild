#!/usr/bin/env bash
# tinybuild needs ./compile.sh
# tinybuild needs ./resource.sh
# tinybuild input Info.plist
# tinybuild output MyApp.app
set -euo pipefail

app="MyApp.app"
rm -rf "$app"
mkdir -p "$app"

cp MyApp "$app/MyApp"
cp message.txt "$app/message.txt"
cp Info.plist "$app/Info.plist"
