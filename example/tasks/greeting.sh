#!/usr/bin/env bash
# tinybuild input names.txt
# tinybuild output build/greeting.txt
# tinybuild env GREETING
set -euo pipefail

mkdir -p build
greeting="${GREETING:-Hello}"
: > build/greeting.txt
while IFS= read -r name; do
  [ -z "$name" ] && continue
  echo "$greeting, $name!" >> build/greeting.txt
done < names.txt

echo "wrote build/greeting.txt"
