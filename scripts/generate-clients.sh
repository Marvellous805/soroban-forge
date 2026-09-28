#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Soroban Forge — TypeScript client generation for all six contracts.
#
# Single repeatable source of truth for the generated clients under
# `packages/*-client`. Builds each contract WASM locally for the
# repository's `wasm32v1-none` target and runs the pinned Stellar CLI's
# `contract bindings typescript --wasm` to emit fully typed bindings
# **offline** (no deployed contract id needed), then writes them into each
# package directory.
#
# The script is safe to run repeatedly: every package directory is wiped
# and regenerated from the freshly built WASM, and generated output is
# committed rather than rebuilt by consumers (they would need the Soroban
# toolchain otherwise).
#
# Requires:
#   - stable Rust + the wasm32v1-none target (see rust-toolchain.toml)
#   - the Stellar CLI (`stellar`), pinned at 25.1.0 in the workspace
#
# Usage:
#   bash scripts/generate-clients.sh
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TARGET="wasm32v1-none"
RELEASE_DIR="target/$TARGET/release"

# Contract crates in the order they map onto packages/*-client.
# (crate name, wasm basename, package dir)
declare -a CONTRACTS=(
  "soroban-forge-escrow|soroban_forge_escrow.wasm|packages/typescript-sdk"
  "soroban-forge-vesting|soroban_forge_vesting.wasm|packages/vesting-client"
  "soroban-forge-multi-sig-wallet|soroban_forge_multi_sig_wallet.wasm|packages/multi-sig-wallet-client"
  "soroban-forge-marketplace-royalties|soroban_forge_marketplace_royalties.wasm|packages/marketplace-royalties-client"
  "soroban-forge-subscription-payments|soroban_forge_subscription_payments.wasm|packages/subscription-payments-client"
  "soroban-forge-dao-governance|soroban_forge_dao_governance.wasm|packages/dao-governance-client"
)

# Keep in sync with scripts/provenance.sh; this list is duplicated in the
# cargo build command below so it stays explicit and deterministic.

echo "==> Building contract WASMs for target '$TARGET'"
cargo build --locked --release --target "$TARGET" \
  --package soroban-forge-escrow \
  --package soroban-forge-vesting \
  --package soroban-forge-multi-sig-wallet \
  --package soroban-forge-marketplace-royalties \
  --package soroban-forge-subscription-payments \
  --package soroban-forge-dao-governance

for entry in "${CONTRACTS[@]}"; do
  IFS='|' read -r _crate wasm out_dir <<< "$entry"
  wasm_path="$RELEASE_DIR/$wasm"
  if [ ! -f "$wasm_path" ]; then
    echo "error: expected WASM missing: $wasm_path" >&2
    exit 1
  fi
  echo "==> Generating $out_dir from $wasm_path"
  rm -rf "$out_dir"
  mkdir -p "$out_dir"
  stellar contract bindings typescript \
    --wasm "$wasm_path" \
    --output-dir "$out_dir" \
    --overwrite
done

echo "==> All six clients generated."
echo "    Next: (cd packages/<client> && npm run build) for each."