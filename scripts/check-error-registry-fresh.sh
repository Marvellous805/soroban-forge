#!/usr/bin/env bash
# Check that error registry files are fresh (up to date with the source)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "Checking ForgeError registry freshness..."

# Generate fresh registry files
bash scripts/generate-error-registry.sh

# Check if any files changed
if ! git diff --exit-code -- errors.json packages/typescript-sdk/src/errors.generated.ts; then
    echo ""
    echo "Error: Generated error registry files are out of date."
    echo "Run 'bash scripts/generate-error-registry.sh' to update them."
    exit 1
fi

echo "Error registry files are fresh."