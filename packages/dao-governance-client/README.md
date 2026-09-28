# @soroban-forge/dao-governance-client

Generated TypeScript client for the **Soroban Forge DAO governance contract**:
proposal lifecycle (propose → vote → execute) with refundable proposal bonds.

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
import { Client } from "@soroban-forge/dao-governance-client";

const client = new Client({
  rpcUrl: "https://soroban-testnet.stellar.org", // or your own RPC
  networkPassphrase: "Test SDF Network ; September 2015",
  contractId: "…", // the deployed contract id
});

// Every contract method is available, typed, with `try*` variants:
const id = await client.propose({ proposer, target, action, duration });
await client.vote({ proposal_id: id.result, voter, support });
await client.execute({ proposal_id: id.result });
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
`stellar contract bindings typescript --wasm … --output-dir packages/dao-governance-client --overwrite`.