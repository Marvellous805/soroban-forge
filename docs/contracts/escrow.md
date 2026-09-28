# Escrow Contract

Secure fund custody for buyer-seller transactions with optional arbiter dispute resolution.

## Interface

```rust
fn create_escrow(buyer, seller, arbiter, token, amount, timeout) -> Result<u64, ForgeError>
fn deposit(escrow_id) -> Result<(), ForgeError>
fn release(escrow_id) -> Result<(), ForgeError>
fn release_partial(escrow_id, amount) -> Result<(), ForgeError>
fn refund(escrow_id) -> Result<(), ForgeError>
fn dispute(escrow_id, claimant) -> Result<(), ForgeError>
fn resolve(escrow_id, in_favor_of_seller) -> Result<(), ForgeError>
fn cancel(escrow_id) -> Result<(), ForgeError>
fn get_status(escrow_id) -> Result<EscrowStatus, ForgeError>   // either record kind
fn get_escrow(escrow_id) -> Result<EscrowData, ForgeError>     // single-token records
fn escrows_for_participant(participant, cursor, limit) -> ParticipantEscrowsPage
fn touch_ttl(escrow_id) -> Result<(), ForgeError>              // either record kind

// Multi-asset baskets (additive; the single-token interface above is unchanged)
fn create_basket(buyer, seller, arbiter, assets: Vec<EscrowAsset>, timeout) -> Result<u64, ForgeError>
fn deposit_basket(escrow_id) -> Result<(), ForgeError>
fn release_basket(escrow_id) -> Result<(), ForgeError>
fn release_partial_basket(escrow_id, token, amount) -> Result<(), ForgeError>
fn refund_basket(escrow_id) -> Result<(), ForgeError>
fn dispute_basket(escrow_id, claimant) -> Result<(), ForgeError>
fn resolve_basket(escrow_id, in_favor_of_seller) -> Result<(), ForgeError>
fn cancel_basket(escrow_id) -> Result<(), ForgeError>
fn get_basket(escrow_id) -> Result<BasketEscrowData, ForgeError> // basket records
```

## Multi-Asset Baskets

A basket is an escrow over an ordered list of legs
(`EscrowAsset { token, amount, released }`) instead of a single
`(token, amount)` pair. It is **additive**: `EscrowData`, `DataKey::Escrow(id)`,
and every single-token event keep their exact shape, so the single-token flow,
its ABI consumers, and its stored records are untouched.

- **Shared id space.** Baskets draw from the same monotonic `DataKey::Count`
  sequence as single-token escrows, so an id names exactly one record of one
  kind. `get_status` / `touch_ttl` work for either kind; `get_escrow` reads
  single-token records and `get_basket` reads baskets (`NotFound` for the
  other kind). The participant index is shared and deduplicated per address.
- **Basket bounds.** 1..=8 legs (`MAX_BASKET_ASSETS`), each `amount > 0`, no
  token repeated (duplicates are rejected, not merged). `create_basket` is
  buyer-authorized, list order is preserved, and the assets vector is part of
  the approved invocation.
- **All-or-nothing deposit.** `deposit_basket` pulls every leg in list order;
  a failure on any leg rolls the whole frame back (host rollback), so custody
  never holds a partial basket and the basket stays `Pending` and retryable.
  The buyer must authorize **one transfer per leg**.
- **Per-leg accounting.** `remaining = amount - released` per leg. A basket
  reaches `Completed` only when **every** leg is fully released, so emptying
  one leg leaves the basket `Funded` with a zero-balance leg.
  `release_partial_basket(escrow_id, token, amount)` pays one named leg
  (unknown token → `NotFound`).
- **Terminal paths pay every leg.** `release_basket` / `refund_basket` /
  `resolve_basket` move each leg's remaining balance through the same payout
  loop, one direction for the whole basket. `refund_basket` keeps the
  single-token refund party rule (seller pre-deadline, buyer post-deadline).
  Auth is per entrypoint as with the single-token paths (see the lifecycle
  diagram below).
- **Dispute freezes all legs.** `dispute_basket` freezes every leg until the
  arbiter resolves (`resolve_basket`, arbiter-only, final).
- **Shared internals.** The single-token paths run through the *same* transfer
  primitives as baskets (a single-token escrow is a one-leg basket), so the
  payout loop and its conservation rule literally cannot drift between kinds.
- **TTL per escrow.** A basket is one persistent entry, extended as a unit;
  legs can never expire independently.

### Basket events

`BasketCreated`, `BasketDeposited`, `BasketReleased`,
`BasketPartiallyReleased` (carries `token` + `partial_amount`), `BasketRefunded`,
`BasketDisputed`, `BasketResolved` (carries `in_favor_of_seller`),
`BasketCancelled` — each carries the id as topic and the full
`BasketEscrowData` payload. Single-token event payloads are unchanged.
## Participant Index and Pagination

Each distinct buyer, seller, and arbiter receives a persistent participant
index in creation order. Creating an escrow adds its id once to each distinct
party's list. Cancelling a `Pending` escrow removes its id from all of those
lists after the status and buyer-authorization checks succeed. Removal keeps
the remaining ids in their original relative order. `release`, `refund`, and
`resolve` do not remove ids: those terminal escrow records remain addressable
through `get_escrow` and participant views. Thus the index contains every
non-cancelled escrow record, whether pending, funded, or terminal.

`escrows_for_participant` uses live offset pagination over the current
compacted list. `cursor` is an index offset at the moment of the call, not a
snapshot token. If cancellation removes an id before a cursor returned by a
previous page, later ids shift left and continuing from that saved cursor can
skip an id. Clients that need a complete view after an intervening mutation
should restart at cursor `0`. Without mutation, replaying `next_cursor` yields
every indexed id exactly once in creation order. `limit == 0`, cursors past
the end, and `u32::MAX` cursor/limit values return an empty terminal page
without overflow.

The invariant is maintained in the successful cancel path only: missing ids,
non-`Pending` records, and failed authorization do not alter either party's
index. No entrypoint signatures or generated TypeScript ABI changed.

## Lifecycle

```text
Pending --deposit--> Funded --release_partial (×n)--> Funded  (partial)
                    |                                  |
                    |                                  +--> Completed (final partial)
                    |        --release--> Completed (full, direct)
                    |        --refund--> Refunded   (buyer back, remaining only)
                    |        --dispute--> Disputed --resolve--> Completed | Refunded
         --cancel--> Cancelled (before funding only)
```

## States

- `Pending` — Created but not funded
- `Funded` — Funds deposited; partial releases may be applied
- `Completed` — Released to seller (full release or final partial release)
- `Refunded` — Remaining balance returned to buyer
- `Disputed` — Under arbitration (remaining balance frozen)
- `Cancelled` — Cancelled before funding

## Partial Release

`release_partial(escrow_id, amount)` allows the seller to receive the escrow
balance incrementally while the escrow remains `Funded`.

### Accounting

```text
remaining = deposited - released
deposited = amount field set at create_escrow
released  = cumulative amount transferred to seller via release_partial
```

- Each successful `release_partial` call:
  1. Validates `status == Funded`, `amount > 0`, `amount <= remaining`
  2. Transfers exactly `amount` to the seller (transfer-before-state)
  3. Increments `released` by `amount`
  4. If `amount == remaining`: transitions to `Completed`
- `release` (the original full-release entrypoint) pays the **remaining** balance
  in one call and is backward-compatible — it behaves identically to before
  when no partial releases have been made.
- `refund` and `resolve` operate on the **remaining** balance only.
- `dispute` freezes the **remaining** balance.

### Authorization

`release_partial` is seller-authorized. Invalid requests (wrong status,
non-positive amount, amount exceeding remaining) return `InvalidInput` and
do not modify any storage.

### Events

`release_partial` emits `PartiallyReleased` (not `Released`). The event
carries `partial_amount` (the incremental transfer) and the full `EscrowData`
(including updated `released` and `status`). The existing `Released` event
remains exclusively for the `release` entrypoint and signals terminal
completion to indexers.

## Storage Compatibility

`EscrowData` now carries a `released: i128` field. Records written by
earlier contract versions do not contain this field.

**Strategy**: two-type fallback decode in `load_escrow`.
1. Attempt deserialization as current `EscrowData` (10 fields including `released`).
2. On failure, attempt as `EscrowDataV1` (9 fields, no `released`).
3. Convert `EscrowDataV1` → `EscrowData` by defaulting `released = 0`.

This works because Soroban `#[contracttype]` structs are stored as XDR
symbol-keyed maps. The host rejects deserialization when the map's entry count
differs from the struct's field count; old 9-field records fail step 1 and
succeed at step 2. `EscrowDataV1` is a read-only migration type; new code
never writes it. Lazy migration: the first state-changing call on an old record
writes the current schema back.

`DataKey::Escrow(id)` is unchanged.

## WASM Budget

Current size: ~28 KB  
Limit: < 150 KB

## Feature Flags

- `test-utils` — enables test-only helpers (proptest, authz tests)
