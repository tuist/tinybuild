#!/usr/bin/env bash
# tinybuild input Resources/message.txt
# tinybuild output message.txt
set -euo pipefail

tr '[:lower:]' '[:upper:]' < Resources/message.txt > message.txt
