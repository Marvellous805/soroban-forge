# @soroban-forge/vesting-client

Generated TypeScript client for the **Soroban Forge vesting contract**:
time/condition-based token release schedules with cliff support.

The client is generated **offline** from the contract's WASM ABI by the Stellar
CLI, so every method is fully typed and carries the doc comments from the
contract source.

## Install

```bash
npm install
npm run build   # emits dist/
```

## Usage

```ts
import { Client } from "@soroban-forge/vesting-client";

const client = new Client({
  rpcUrl: "https://soroban-testnet.stellar.org", // or your own RPC
  networkPassphrase: "Test SDF Network ; September 2015",
  contractId: "…", // the deployed contract id
});

// Every contract method is available, typed, with `try*` variants:
const id = await client.create_schedule({
  beneficiary, token, total_amount, cliff, duration,
});
await client.claim({ schedule_id: id.result });
```

Signers/wallets are supplied per call via `MethodOptions` (`sign`, `simulate`,
etc.) — see the
[stellar-sdk contract client docs](https://stellar.github.io/js-stellar-sdk/).

## Provenance

Regenerate after any contract interface change:

```bash
bash scripts/generate-clients.sh
```

The script rebuilds the contract WASM and runs
`stellar contract bindings typescript --wasm … --output-dir packages/vesting-client --overwrite`.