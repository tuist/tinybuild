#!/usr/bin/env bash
# tinybuild needs greeting
# tinybuild needs banner
# tinybuild input build/greeting.txt
# tinybuild input build/banner.txt
# tinybuild output build/package.txt
set -euo pipefail

cat build/banner.txt build/greeting.txt > build/package.txt
echo "wrote build/package.txt"
