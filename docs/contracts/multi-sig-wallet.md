# Multi-Signature Wallet Contract

Multi-owner wallet with configurable approval threshold and transaction queue.

- **Source:** `crates/multi-sig-wallet`
- **Client:** `SorobanForgeMultiSigWalletClient` (generated)
- **Related:** [Contract index](./index.md), [Feature Status Matrix](../FEATURE-STATUS.md), [Known Limitations](../KNOWN-LIMITATIONS.md), [DAO Governance](./dao-governance.md)

## Interface

```rust
fn initialize(owners: Vec<Address>, threshold: u32) -> Result<(), ForgeError>
fn submit(proposer, target, function_name, args) -> Result<u64, ForgeError>
fn confirm(tx_id, signer) -> Result<(), ForgeError>
fn reject(tx_id, signer) -> Result<(), ForgeError>
fn execute(tx_id) -> Result<(), ForgeError>
fn get_transaction(tx_id) -> Result<Transaction, ForgeError>
fn add_owner(owner) -> Result<(), ForgeError>
fn remove_owner(owner) -> Result<(), ForgeError>
fn update_threshold(new_threshold) -> Result<(), ForgeError>
```

The wallet is configured **once** via `initialize(owners, threshold)`
(first caller wins; re-initialisation is rejected). `submit` records a
`target` contract and an opaque payload; `confirm` collects approvals until
the threshold is reached; `execute` then performs a real cross-contract
invocation to the recorded target (an opaque `TxKind::Data` tx) or moves
real tokens (a typed `TxKind::Withdrawal` tx). A target revert surfaces as
`ForgeError::ContractInvocationFailed` and leaves the tx `Pending` and
retryable. `reject` records a formal objection; any rejection blocks
execution, and reaching the rejection threshold makes the tx `Rejected`
(terminal).

## Transaction query views

The wallet exposes read-only views for loading the transaction queue without
one contract call per transaction:

```rust
fn get_transactions(offset: u32, limit: u32) -> Result<Vec<WalletTx>, ForgeError>
fn get_transactions_by_status(
    status: TxStatus,
    offset: u32,
    limit: u32,
) -> Result<Vec<WalletTx>, ForgeError>
```

Both return transactions in ascending `tx_id` order. For
`get_transactions`, `offset` is zero-based in the complete sequence (so
offset `0` starts at transaction id `1`). For
`get_transactions_by_status`, `offset` is zero-based among transactions
matching the requested status. Each returns at most `limit` records; ranges
past the end return an empty vector or the remaining records. A `limit` of
zero returns `ForgeError::InvalidInput`. An uninitialized or empty wallet
returns an empty vector for a positive limit. These views do not require
authorization and do not modify contract state.

Additional views: `get_threshold`, `get_owners`, `is_owner`,
`get_confirmations`, `get_rejections`, `get_tx_count`, `get_tx`, and the
per-token views `get_withdrawal_limit` / `get_window_usage` / `balance`.

## States

- `Pending` — Awaiting approvals
- `Executed` — Threshold met and the transaction completed
- `Rejected` — Rejection threshold met; terminal

## Storage & TTL Maintenance

Transaction records (`DataKey::Tx(u64)`) are stored in **persistent
storage**: `submit`, `confirm`, `reject`, `execute`, `submit_withdrawal`,
and the limit-change paths write through `.persistent()` and bump the
entry's TTL to a 30-day horizon on every write. `get_tx` reads from
persistent storage. Owners, threshold, per-token balances, and withdrawal
limits remain in instance storage.

A permissionless public keeper entrypoint `touch_tx_ttl(tx_id)` allows
anyone to bump a transaction's persistent TTL without modifying its state;
an unknown `tx_id` returns `ForgeError::NotFound`. A separate
`touch_ttl(token)` keeper extends the persistent balance entries' TTL.