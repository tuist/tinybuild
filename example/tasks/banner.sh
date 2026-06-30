#!/usr/bin/env bash
# tinybuild output build/banner.txt
set -euo pipefail

mkdir -p build
cat > build/banner.txt <<'EOF'
========================
 tinybuild demo package
========================
EOF

echo "wrote build/banner.txt"
