# @soroban-forge/subscription-payments-client

Generated TypeScript client for the **Soroban Forge subscription payments
contract**: provider-initiated subscriptions via explicit-actor authorization
(`authorize_provider` / `is_provider_authorized` / `subscribe_on_behalf_of`),
with SEP-41 settlement and a PastDue retry model.

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
import { Client } from "@soroban-forge/subscription-payments-client";

const client = new Client({
  rpcUrl: "https://soroban-testnet.stellar.org", // or your own RPC
  networkPassphrase: "Test SDF Network ; September 2015",
  contractId: "…", // the deployed contract id
});

// Every contract method is available, typed, with `try*` variants:
await client.authorize_provider({ subscriber, provider });
const id = await client.subscribe_on_behalf_of({
  provider, subscriber, token, amount, period,
});
await client.charge({ subscription_id: id.result });
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
`stellar contract bindings typescript --wasm … --output-dir packages/subscription-payments-client --overwrite`.