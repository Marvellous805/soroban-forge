# Feature Status Matrix

Per-entrypoint status across all six contracts. **Implemented** means:
implemented, tested, and covered by workspace CI. The escrow contract is
the flagship: it moves real SEP-41 tokens; marketplace royalties settles
its splits the same way via `settle_sale` and, for batches, `settle_sales`,
and `distribute` now pays the royalty share directly; vesting settles its
claims via
`claim`. The subscription state machine is honestly labeled where it tracks
but does not settle; DAO governance now dispatches approved opaque actions
on-chain and settles a SEP-41 proposal bond — pulled at `propose`, refunded
to the proposer or forfeited to the treasury at a terminal transition.
Multi-sig wallet transactions live in persistent storage with a
permissionless TTL keeper, alongside its threshold-and-confirmation state
machine.
its splits the same way via `settle_sale`; vesting settles its claims via
`claim`; subscriptions bill each due period with a real subscriber →
provider transfer and retry past-due payments; DAO governance now
dispatches approved opaque actions on-chain and settles a SEP-41 proposal
bond — pulled at `propose`, refunded to the proposer or forfeited to the
treasury at a terminal transition.

**Last verified against:** the SDK 27 migration (workspace v0.2.0).

---

## Escrow (`crates/escrow`) — **flagship**

| Entrypoint | Status | Notes |
|---|---|---|
| `create_escrow` | ✅ Implemented | Validates amount/timeout, buyer+seller auth, takes the SEP-41 token address |
| `deposit` | ✅ Implemented | **Real token transfer** buyer → contract, before any state write |
| `release` | ✅ Implemented | Seller-authorized; **real token transfer** contract → seller (full remaining balance) |
| `release_partial` | ✅ Implemented | Seller-authorized; **real token transfer** of a partial amount contract → seller; `released` accounting tracked; final partial transitions to `Completed`; `refund`/`resolve` operate on remaining balance |
| `refund` | ✅ Implemented | Seller pre-deadline / buyer post-deadline; **real token transfer** of remaining balance |
| `dispute` | ✅ Implemented | Claimant (buyer or seller) authorized, `Funded` only — see [design notes](KNOWN-LIMITATIONS.md#design-notes-not-limitations-but-worth-knowing) |
| `resolve` | ✅ Implemented | Arbiter-only, final; pays **remaining balance** either direction via **real token transfer** |
| `cancel` | ✅ Implemented | Buyer, `Pending` only; removes id from each distinct participant index after validation/auth |
| `get_status` / `get_escrow` / `escrows_for_participant` | ✅ Implemented | Read-only views; participant index excludes cancelled ids but retains other terminal records; live offset pagination, restart at cursor 0 after cancellation |
| `touch_ttl` | ✅ Implemented | Permissionless TTL keeper for the escrow's persistent entry |
| Events | ✅ Implemented | `EscrowCreated`, `Deposited`, `Released`, `PartiallyReleased`, `Refunded`, `Disputed`, `Resolved`, `Cancelled`; id as topic |
| Storage | ✅ Persistent + TTL | Per-id persistent entries; instance storage only for the id counter; **backward-compatible schema migration** via `EscrowDataV1` fallback decode (old records default `released = 0`) |
| Tests | ✅ 85 | Full lifecycle, dispute paths, cancellation index maintenance and live-pagination interleaving, failure ordering, conservation property, partial-release (valid/multi/final/zero/negative/over-remaining/non-Funded/after-completion/→refund/→dispute→resolve, storage compat, conservation), **randomized property suite** (proptest): conservation over random paths + partial-release sequences, tamper-resilient pool conservation, fund safety over arbitrary call sequences (now includes `release_partial`), multi-party create/cancel participant-index consistency; **negative-auth suite** (`authz.rs`): per-entrypoint wrong-signer rejection, `release_partial` seller-only auth + mutation test, signature/args replay rejection, `env.auths()` authorization-tree assertions |

## Vesting (`crates/vesting`)

| Entrypoint                    | Status             | Notes                                                                                                                                                                                                                                |
| ----------------------------- | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `create_schedule`             | ✅ Implemented     | Validates `total_amount > 0`, `duration > 0`, `cliff <= duration`                                                                                                                                                                    |
| `claim`                       | ✅ Implemented     | **Real token transfer** contract → beneficiary before the state write (transfer-before-state); zero-claim calls skip the transfer; a failed transfer surfaces as `ForgeError::TokenTransferFailed` with `claimed`/`status` unchanged |
| `claimable`                   | ✅ Implemented     | Read-only                                                                                                                                                                                                                            |
| `get_status`                  | ✅ Implemented     | Read-only                                                                                                                                                                                                                            |
| Revocation                    | ❌ Not implemented | `Revoked` status reserved                                                                                                                                                                                                            |
| `VestingSchedule.token` field | ✅ Wired           | Read by `claim` for the SEP-41 payout                                                                                                                                                                                                |
| Events                        | ✅ Implemented     | `ScheduleCreated` and `Claimed` (`schedule_id` as topic); zero claims are silent                                                                                                                                                      |

## Multi-Sig Wallet (`crates/multi-sig-wallet`)

| Entrypoint                                         | Status         | Notes                                                                                                                                                                                                                   |
| -------------------------------------------------- | -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `initialize`                                       | ✅ Implemented | Owner set + threshold validation                                                                                                                                                                                        |
| `submit`                                           | ✅ Implemented | Creates pending transaction record in **persistent storage** (TTL-bumped on write)                                                                                                                                      |
| `confirm`                                          | ✅ Implemented | One-confirmation-per-owner enforced; tx record re-bumped in persistent storage                                                                                                                                          |
| `execute`                                          | ✅ Implemented | Threshold check + cross-contract `try_invoke_contract` to recorded `target`; status flip **after** invocation; target revert surfaces as `ForgeError::ContractInvocationFailed` and leaves tx `Pending`; events emitted |
| `get_threshold` / `get_tx`                         | ✅ Implemented | Read-only; `get_tx` reads from persistent storage                                                                                                                                                                       |
| `submit_withdrawal`                                | ✅ Implemented | Typed `TxKind::Withdrawal` tx; balance validated at execution (transfer first, state second); per-token rolling limit enforced at submission                                                                            |
| `deposit` / `balance` / `touch_ttl`                | ✅ Implemented | Per-token persistent balance entries; permissionless TTL keeper                                                                                                                                                         |
| `touch_tx_ttl`                                     | ✅ Implemented | Permissionless keeper: extends the persistent TTL of a transaction record without touching its state; `NotFound` for unknown ids                                                                                         |
| `set_withdrawal_limit` / `remove_withdrawal_limit` | ✅ Implemented | `TxKind::LimitChange` txs on the same threshold+confirmation machinery; no effect below threshold                                                                                                                       |
| `get_withdrawal_limit` / `get_withdrawal_window` / `get_window_usage` / `check_withdrawal` | ✅ Implemented | Read-only; missing limit/window reads as `None`/empty; simulation reports initialization, amount, limit, and custody-balance outcomes; no storage mutation |
| Storage                                            | ✅ Persistent + TTL | `DataKey::Tx` records in persistent storage (migrated from instance) with 30-day TTL maintenance; owners, threshold, and limits remain instance storage                                                                |
| Tests                                              | ✅ 109         | Full lifecycle, threshold/quorum, withdrawal + limit-change paths, keeper TTL tests, failure ordering, negative-auth suite (`authz.rs`)                                                                                  |
| `submit`                                           | ✅ Implemented | Creates pending transaction record with opaque payload                                                                                                                                                                  |
| `submit_call`                                      | ✅ Implemented | Creates pending typed cross-contract call with target address, function name, and arguments                                                                                                                             |
| `confirm`                                          | ✅ Implemented | One-confirmation-per-owner enforced                                                                                                                                                                                     |
| `execute`                                          | ✅ Implemented | Threshold check + cross-contract `try_invoke_contract`; supports opaque payloads and typed calls; status flip **after** invocation; target revert surfaces as `ForgeError::ContractInvocationFailed` and leaves tx `Pending` |
| `get_threshold` / `get_tx`                         | ✅ Implemented | Read-only                                                                                                                                                                                                               |
| `submit_withdrawal`                                | ✅ Implemented | Typed `TxKind::Withdrawal` tx; balance validated at execution (transfer first, state second); per-token rolling limit enforced at submission                                                                            |
| `deposit` / `balance` / `touch_ttl`                | ✅ Implemented | Per-token persistent balance entries; permissionless TTL keeper                                                                                                                                                         |
| `set_withdrawal_limit` / `remove_withdrawal_limit` | ✅ Implemented | `TxKind::LimitChange` txs on the same threshold+confirmation machinery; no effect below threshold                                                                                                                       |
| `get_withdrawal_limit` / `get_withdrawal_window` / `get_window_usage` / `check_withdrawal` | ✅ Implemented | Read-only; missing limit/window reads as `None`/empty; simulation reports initialization, amount, limit, and custody-balance outcomes; no storage mutation |
| Entrypoint | Status | Notes |
|---|---|---|
| `initialize` | ✅ Implemented | Owner set + threshold validation |
| `submit` | ✅ Implemented | Creates pending transaction record |
| `confirm` | ✅ Implemented | One-confirmation-per-owner enforced |
| `execute` | ✅ Implemented | Threshold check + cross-contract `try_invoke_contract` to recorded `target`; status flip **after** invocation; target revert surfaces as `ForgeError::ContractInvocationFailed` and leaves tx `Pending`; events emitted |
| `get_threshold` / `get_tx` | ✅ Implemented | Read-only |
| `submit_withdrawal` | ✅ Implemented | Typed `TxKind::Withdrawal` tx; balance validated at execution (transfer first, state second); per-token rolling limit enforced at submission |
| `deposit` / `balance` / `touch_ttl` | ✅ Implemented | Per-token persistent balance entries; permissionless TTL keeper |
| `set_withdrawal_limit` / `remove_withdrawal_limit` | ✅ Implemented | `TxKind::LimitChange` txs on the same threshold+confirmation machinery; no effect below threshold |
| `get_withdrawal_limit` / `get_window_usage` | ✅ Implemented | Read-only; `None`/`0` when no limit is configured |
| `add_owner` / `remove_owner` / `set_threshold` | ✅ Implemented | Owner-set and threshold governance via the same typed-tx machinery: `TxKind::AddOwner` / `RemoveOwner` / `SetThreshold` must cross the **current** threshold before `execute` applies them; submission and execution both re-validate the resulting state; removed-owner confirmations are scrubbed from still-pending txs; `remove_owner` guards the final owner and `set_threshold` requires `1 <= t <= owners.len()` |
| `get_owners` / `is_owner` / `get_confirmations` / `get_rejections` / `get_tx_count` / `get_transactions` / `get_transactions_by_status` | ✅ Implemented | Read-only views |

## DAO Governance (`crates/dao-governance`)

| Entrypoint | Status | Notes |
|---|---|---|
| `configure_bond` | ✅ Implemented | One-time permissionless config (token, amount, treasury); first caller wins; `propose` is rejected with `NotInitialized` while unconfigured |
| `propose` | ✅ Implemented | Stores the target contract, opaque action payload, and voting deadline; **real token transfer** proposer → contract for the bond, before any state write |
| `vote` | ✅ Implemented | One-vote-per-voter enforced |
| `execute` | ✅ Implemented | Permissionless majority finalisation, then `try_invoke_contract` to `target.execute(action)`; target failure leaves the proposal `Succeeded`; on terminal transitions the bond is **refunded** to the proposer (`Executed`) or **forfeited** to the treasury (`Defeated`) in the same frame |
| `cancel_proposal` | ✅ Implemented | Proposer-authorized revocation; **real token transfer** refund of the bond |
| `get_proposal` / `get_proposal_count` / `get_proposals` / `has_voted` | ✅ Implemented | Read-only views; pagination with bounds clamping |
| `get_bond_config` | ✅ Implemented | Read-only; `NotInitialized` when no bond is configured |
| `touch_ttl` | ✅ Implemented | Permissionless keeper: extends the persistent TTL of a proposal; `NotFound` for unknown ids |
| Events | ✅ Implemented | `Proposed`, `VoteCast`, `Finalised`, `BondPosted`, `BondReleased` (`proposal_id` as topic) |
| Storage | ✅ Persistent + TTL | `DataKey::Proposal` records in persistent storage with 30-day TTL maintenance; count, bond config, custody total, and vote markers in instance storage |
| Tests | ✅ 74 | Bond custody lifecycle (post/refund/forfeit/conservation), arithmetic boundary tests, rollback-on-failure ordering, cross-contract dispatch + retry, introspection/pagination views, plus a **negative-auth suite** (`authz.rs`): wrong-signer and args-replay rejection, the nested token authorization frame for the bond pull, `env.auths()` authorization-tree assertions |
| Weighted voting | ❌ Not implemented | Follow-up |

## Subscription Payments (`crates/subscription-payments`)

| Entrypoint         | Status             | Notes                                                                   |
| ------------------ | ------------------ | ----------------------------------------------------------------------- |
| `subscribe`        | ✅ Implemented     | Plan validation, periodic scheduling                                    |
| `charge`           | ✅ Implemented     | Real SEP-41 transfer for one period; failed transfers enter `PastDue`   |
| `charge_catchup`   | ✅ Implemented     | Atomically settles up to 32 elapsed periods per call; refuses `PastDue` |
| `cancel`           | ✅ Implemented     | Subscriber or owner                                                     |
| `get_subscription` | ✅ Implemented     | Read-only                                                               |
| Storage / `touch_ttl` | ✅ Implemented | Subscription records use persistent storage with 30-day TTL bumps; counter and indexes stay in instance storage |
| Plan management    | ❌ Not implemented | Follow-up                                                               |

## Marketplace Royalties (`crates/marketplace-royalties`)

| Entrypoint               | Status             | Notes                                                                                                                 |
| ------------------------ | ------------------ | --------------------------------------------------------------------------------------------------------------------- |
| `set_royalty`            | ✅ Implemented     | Basis-point caps validated                                                                                            |
| `disable_royalty` / `enable_royalty` | ✅ Implemented | Collection-authorized status transitions preserve stored bps and recipient |
| `distribute`             | ⚠️ Computes only   | Pure split math; **pays no recipients** (use `settle_sale`)                                                           |
| `settle_sale`            | ✅ Implemented     | **Real token transfers** payer → seller, then payer → royalty recipient; transfer-before-state, totals committed last |
| `get_royalty`            | ✅ Implemented     | Read-only                                                                                                             |
| `get_settlement_summary` | ✅ Implemented     | Read-only; cumulative sales, volume, and royalties per collection                                                     |
| Multi-recipient splits   | ❌ Not implemented | Follow-up                                                                                                             |
| Entrypoint | Status | Notes |
|---|---|---|
| `subscribe` | ✅ Implemented | Validates `amount > 0` / `period > 0` before auth; subscriber-authorized; record + sequential id + both subscriber/provider indexes written atomically |
| `subscribe_on_behalf_of` | ✅ Implemented | Provider-initiated; requires a **subscriber opt-in** (`ProviderOptIn`) checked before provider auth; shares the same id counter and record shape as `subscribe` |
| `authorize_provider` / `revoke_provider` / `is_provider_authorized` | ✅ Implemented | Explicit per-relationship opt-in; subscriber-authorized; idempotent; read-only view has no auth |
| `charge` | ✅ Implemented | Provider-authorized; bills when a full period has elapsed via a **real SEP-41 transfer** subscriber → provider; on failure increments `failed_attempts` → `PastDue`, and `Cancelled` after `MAX_RETRIES` (3) |
| `pause` / `resume` | ✅ Implemented | Subscriber-authorized; `resume` advances the next due date by the elapsed paused duration |
| `cancel` | ✅ Implemented | Subscriber-authorized from `Active` / `Paused` / `PastDue`; rejects already-`Cancelled` |
| `get_subscription` / `get_subscription_count` / `subscriptions_for_subscriber` / `subscriptions_for_provider` | ✅ Implemented | Read-only views; paged by `offset`/`limit` with `limit == 0` → `InvalidInput`; empty index yields an empty page, not an error |
| Plan management | ❌ Not implemented | Follow-up |

## Marketplace Royalties (`crates/marketplace-royalties`)

| Entrypoint | Status | Notes |
|---|---|---|
| `set_royalty` | ✅ Implemented | Basis-point caps validated |
| `distribute` | ✅ Implemented | **Real SEP-41 royalty payout**: `distribute(collection, token, payer, seller, amount)` transfers `amount * bps / 10_000` from the payer to the configured recipient and returns the seller's net; a `Disabled`/zero-bps config transfers nothing; standalone settlement for sales handled outside `settle_sale`, the seller's net is not transferred here |
| `settle_sale` | ✅ Implemented | **Real token transfers** payer → seller, then payer → royalty recipient; transfer-before-state, totals committed last |
| `settle_sales` | ✅ Implemented | **Atomic batch settlement**: up to 20 sales per call against one collection + one payer authorization; every validation (cap, amounts, aggregate split math) before the first transfer, per-sale seller-then-recipient order, aggregate summary committed exactly once; any failure rolls the whole batch back |
| `get_royalty` | ✅ Implemented | Read-only |
| `get_settlement_summary` | ✅ Implemented | Read-only; cumulative sales, volume, and royalties per collection |
| `touch_ttl` | ✅ Implemented | Permissionless keeper: extends the persistent TTL of a collection's royalty + summary records; `NotFound` when unregistered |
| Storage | ✅ Persistent + TTL | `Royalty` and `SettlementSummary` records in persistent storage with 30-day TTL maintenance |
| Multi-recipient splits | ❌ Not implemented | Follow-up |

---

## Cross-cutting

| Concern | Status | Notes |
|---|---|---|
| Checked arithmetic | ✅ Workspace-wide | Overflow-safe; vesting guards documented |
| `require_auth` on every state change | ✅ Workspace-wide | Escrow, vesting, DAO governance, and marketplace royalties proven against wrong signers via their negative-auth suites (`authz.rs`) + authorization-tree assertions; other two: call-graph level only (see [Known Limitations §4](KNOWN-LIMITATIONS.md)) |
| Events | ✅ Escrow + Multi-Sig + DAO + Marketplace | Full lifecycle events on escrow, multi-sig wallet, DAO governance, and marketplace royalties |
| Persistent storage + TTL | ✅ Escrow + Royalties + Multi-Sig + DAO | Per-record persistent entries with `touch_ttl`/`touch_tx_ttl` keepers on escrow, marketplace royalties (royalty + summary), multi-sig (transactions), and DAO (proposals); vesting and subscriptions remain instance-only |
| SEP-41 token settlement | ✅ Escrow + royalties + multi-sig + vesting + subscriptions | Real transfers with transfer-before-state ordering on escrow (`deposit`/`release`/`refund`/`resolve`) and vesting (`claim`); marketplace `settle_sale`/`settle_sales`/`distribute` settle splits; DAO `propose` pulls the proposal bond and refunds/forfeits it on settlement; multi-sig `execute` performs cross-contract `try_invoke_contract` calls on opaque payloads; subscriptions still store amounts only |
| Testnet deployment | ✅ Escrow deployed | Contract ID, WASM sha256, and receipt rounds in the README "Proof at a glance" table; the other five are not deployed |
| Mainnet deployment | ⚠️ Partial | Smoke SAC live (`CBBCLWWU…DN4CW`, Horizon-confirmed); escrow WASM upload measured at **17.57 XLM rent** via simulation and deferred pending funding — see [Known Limitations §6](KNOWN-LIMITATIONS.md) |
| TypeScript SDK | ✅ Generated | `@soroban-forge/escrow-client` generated from the deployed escrow ABI (no own test suite yet) |
| Provenance + verification | ✅ CLI | `soroban-forge verify` checks a WASM artifact / expected hash against a deterministic rebuild using `provenance-manifest.json` | 
| `require_auth` on every state change | ✅ Workspace-wide | Escrow, vesting, and DAO governance proven against wrong signers via their negative-auth suites (`authz.rs`) + authorization-tree assertions; other three: call-graph level only (see [Known Limitations §4](KNOWN-LIMITATIONS.md)) |
| Events | ⚠️ Escrow + Multi-Sig + DAO | Full lifecycle events on escrow, multi-sig wallet, and DAO governance |
| Persistent storage + TTL | ⚠️ Escrow only | Per-id persistent entries + `touch_ttl` keeper; others instance-only |
| SEP-41 token settlement | ⚠️ Escrow + royalties + multi-sig + vesting + DAO + subscriptions | Real transfers with transfer-before-state ordering on escrow (`deposit`/`release`/`refund`/`resolve`) and vesting (`claim`); marketplace `settle_sale` settles splits; DAO `propose` pulls the proposal bond and refunds/forfeits it on settlement; multi-sig `execute` performs cross-contract `try_invoke_contract` calls on opaque payloads; subscriptions `charge` executes a real subscriber → provider transfer (past-due retry → auto-cancel after 3 failed attempts) |
| Testnet deployment | ✅ Escrow deployed | Contract ID, WASM sha256, and receipt rounds in the README "Proof at a glance" table; the other five are not deployed |
| Mainnet deployment | ⚠️ Partial | Smoke SAC live (`CBBCLWWU…DN4CW`, Horizon-confirmed); escrow WASM upload measured at **17.57 XLM rent** via simulation and deferred pending funding — see [Known Limitations §6](KNOWN-LIMITATIONS.md) |
| TypeScript SDK | ✅ Generated | `@soroban-forge/escrow-client` generated from the deployed escrow ABI (no own test suite yet) |
| CLI | ✅ Implemented | Developer CLI with `build`, `test`, `lint`, `deploy`, `invoke`, and `events` commands |
| ForgeError registry | ✅ Generated | Machine-readable error registry from Rust source; generates `errors.json` and TypeScript module |
| TypeScript SDK | ✅ Generated | `@soroban-forge/escrow-client` from the deployed escrow ABI + five generated clients (`vesting`, `multi-sig-wallet`, `subscription-payments`, `marketplace-royalties`, `dao-governance`) via `scripts/generate-clients.sh`; all six build with `npm run build` (no own test suites yet) |
| Next.js demo | ✅ Example | `packages/nextjs-example` — testnet escrow demo UI (Freighter connect, friendbot funding, create/deposit/release/refund/dispute/resolve, read, participant list) |
| CI (fmt/clippy/test/audit/WASM size/provenance) | ✅ Enforced | `--locked`, `-D warnings`, stable toolchain, `wasm32v1-none`, size budget, **provenance manifest job** (SHA-256 of all six WASM artifacts from a clean rebuild) |
| External audit | ❌ Not performed | Planned as a grant-funded tranche deliverable before any mainnet value custody |
| Soroban SDK version | ✅ 27.0.6 | Stable Rust; `wasm32v1-none` target |