# @soroban-forge/escrow-client

Unified TypeScript package with generated, strongly typed clients for all six
Soroban Forge contracts: escrow, vesting, multi-sig wallet, DAO governance,
subscription payments, and marketplace royalties. The package name is retained
for compatibility with existing escrow client consumers.

Each client is generated from its contract WASM using the Stellar CLI. This is
an offline build step and does not require deployed contract IDs or network
access.

## Install

```bash
npm install
```

## Build

Compile the TypeScript source to `dist/`:

```bash
npm run build
```

The compiled output is what consumers import.  The `dist/` directory is
published; the `src/` source is authoritative.

## Usage

```ts
import {
  escrow,
  vesting,
  multisig,
  dao,
  subscription,
  marketplace,
} from "@soroban-forge/escrow-client";

const client = new escrow.Client({
  ...escrow.networks.testnet,
  rpcUrl: "https://soroban-testnet.stellar.org", // or your own RPC
});

// Every contract client provides typed methods and `try*` variants.
const id = await client.create_escrow({
  buyer: "C…",
  seller: "C…",
  arbiter: "C…",
  token: "C…",
  amount: 500n,
  timeout: 86_400n,
});

await client.deposit({ escrow_id: id.result });
await client.release({ escrow_id: id.result });
await client.dispute({ escrow_id: id.result, claimant: seller });
await client.resolve({ escrow_id: id.result, in_favor_of_seller: true });
await client.get_status({ escrow_id: id.result });
await client.touch_ttl({ escrow_id: id.result }); // permissionless keeper

// The other generated clients are namespaced the same way:
const vestingClient = new vesting.Client({
  contractId: "<VESTING_CONTRACT_ID>",
  networkPassphrase: "Test SDF Network ; September 2015",
  rpcUrl: "https://soroban-testnet.stellar.org",
});
```

Each namespace exposes its own `Client` and contract-specific types. The
escrow client includes its existing `networks.testnet` preset; provide a
contract ID, network passphrase, and `rpcUrl` for the other clients.
Signers/wallets are supplied per call via `MethodOptions` (`sign`, `simulate`,
etc.). See the
[stellar-sdk contract client docs](https://stellar.github.io/js-stellar-sdk/).

## Generate all clients

From this directory, run:

```bash
npm run generate
```

The script builds all six contract crates for `wasm32v1-none` with Cargo's
locked release configuration, then generates typed bindings from each WASM.
The unified package modules are written to `src/clients/`; the five standalone
client packages are regenerated alongside them. Contract mappings and output
names are defined in `scripts/generate.mjs`. Generation requires stable Rust,
the `wasm32v1-none` target, and the Stellar CLI; it does not require network
access.

| Contract | Unified package export | Standalone package |
| --- | --- | --- |
| Escrow | `escrow` | `@soroban-forge/escrow-client` |
| Vesting | `vesting` | `@soroban-forge/vesting-client` |
| Multi-Sig Wallet | `multisig` | `@soroban-forge/multi-sig-wallet-client` |
| DAO Governance | `dao` | `@soroban-forge/dao-governance-client` |
| Subscription Payments | `subscription` | `@soroban-forge/subscription-payments-client` |
| Marketplace Royalties | `marketplace` | `@soroban-forge/marketplace-royalties-client` |

## Offline tests

The test suite runs completely offline.  It does **not** require:

- network access
- private keys
- funded accounts
- testnet credentials

```bash
cd packages/typescript-sdk
npm ci
npm test
```

The test script compiles TypeScript first (`tsconfig.test.json` → `dist-test/`)
and then runs the compiled JavaScript with Node's built-in test runner.

## Type-check

Run the TypeScript compiler in check-only mode (no output emitted):

```bash
npm run typecheck
```

## Optional testnet verification

A separate, **opt-in** script performs one live read-only call (`get_status`)
against the deployed testnet contract to confirm end-to-end connectivity.

This is **not** part of `npm test`.  Run it only when you have network access
and a valid escrow ID:

```bash
# 1. Build (if not already done)
npm run build

# 2. Run the verification (ESCROW_ID must be a u64 that exists on-chain)
ESCROW_ID=1 npm run verify:testnet

# Optional: use a custom RPC endpoint
RPC_URL=https://soroban-testnet.stellar.org ESCROW_ID=1 npm run verify:testnet
```

The `verify:testnet` command builds the distribution automatically before running.

Requirements:

- `ESCROW_ID` — a valid escrow ID that exists on the deployed testnet contract
- `RPC_URL` — optional; defaults to `https://soroban-testnet.stellar.org`
- No private keys or funded accounts are needed (`get_status` is read-only)

The script exits with code `0` on success and non-zero on failure, making it
suitable as a post-deployment smoke check in a manual release workflow.

## Provenance

This package replaces the v0.1.0 console-log placeholder SDK.  The contract
itself, its testnet receipt rounds, and the conservation property are
documented in the [repository README](https://github.com/Meet-hybrid/soroban-forge).
