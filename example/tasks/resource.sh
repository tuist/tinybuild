#!/usr/bin/env bash
# tinybuild input Resources/message.txt
# tinybuild output build/message.txt
set -euo pipefail

mkdir -p build
tr '[:lower:]' '[:upper:]' < Resources/message.txt > build/message.txt
echo "processed build/message.txt"
