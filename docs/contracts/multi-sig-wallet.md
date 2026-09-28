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
per-token views `get_withdrawal_limit` / `get_withdrawal_window` /
`get_window_usage` / `check_withdrawal` / `balance`.

## Withdrawal-limit views

```rust
fn get_withdrawal_limit(token: Address) -> Option<WithdrawalLimit>
fn get_withdrawal_window(token: Address) -> WindowState
fn check_withdrawal(token: Address, amount: i128) -> CheckResult
```

These views require no authorization and do not modify or prune storage.
`get_withdrawal_limit` returns `None` when no policy is configured. The
window view reports the active total (including pending withdrawals), the
oldest active submission time as `window_start`, and that entry's projected
expiry as `reset_at`. Entries expire at `submitted_at + window_seconds`, so
they still count one second before reset and are excluded exactly at reset.
As entries expire at different times, `reset_at` describes the next expiry,
not a time when the entire total necessarily becomes zero. With no configured
limit, no active entries, an unknown token, or an uninitialized wallet, the
window is empty (`total = 0`, timestamps absent). Removing a limit makes the
limit and window views return `None` and an empty window; retained history is
still used if a limit is configured again.

`check_withdrawal` returns `CheckResult::Allowed` or a denial variant:
`WalletNotInitialized`, `InvalidAmount`, `LimitExceeded`,
`InsufficientFunds`, or `ArithmeticOverflow`. It checks initialization,
positive amount, the same rolling-limit arithmetic as submission, then the
wallet's recorded custody balance. This is a snapshot at the current ledger
timestamp, not a promise about later execution: submission reserves window
capacity, while custody funding is rechecked when the approved transaction
executes. The generated TypeScript client has not been regenerated for these
new views; future client regeneration will include them.

Example: with a 1,000-unit limit per 600 seconds, a 700-unit withdrawal
submitted at timestamp 10,000 produces `total = 700`, `window_start = 10,000`,
and `reset_at = 10,600`. A check for 400 at 10,599 returns
`CheckResult::LimitExceeded`, matching submission enforcement. At 10,600 the
700-unit entry is expired; the same check is allowed if the wallet has enough
recorded balance.

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