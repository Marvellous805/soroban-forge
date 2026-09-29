#!/usr/bin/env bash
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec node "$ROOT/packages/typescript-sdk/scripts/generate.mjs" "$@"