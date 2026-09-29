#![no_std]

//! # Soroban Forge — Escrow contract
//!
//! A three-party escrow that **holds real tokens**: a `buyer`, a `seller`,
//! and an `arbiter` agree on an `amount` of a single SEP-41 token and a
//! `timeout`. The buyer funds the escrow on-chain, and the contract
//! custodies the tokens until release, refund, or arbitration:
//!
//! ```text
//! Pending --deposit--> Funded --release_partial (×n)--> Funded  (partial)
//!                     |                                  |
//!                     |                                  +--> Completed (final partial)
//!                     |        --release--> Completed (full, direct)
//!                     |        --refund--> Refunded   (buyer back, remaining only)
//!                     |        --dispute--> Disputed --resolve--> Completed | Refunded
//!          --cancel--> Cancelled (before funding only)
//! ```
//!
//! ## Partial releases
//!
//! `release_partial(escrow_id, amount)` allows the seller to receive funds
//! incrementally while the escrow remains `Funded`. The accounting is:
//!
//! ```text
//! remaining = deposited - released
//! ```
//!
//! * Each `release_partial` transfers exactly `amount` to the seller and
//!   increments `released` by that amount.
//! * When `amount == remaining` (the final partial payment), the escrow
//!   transitions to `Completed` exactly once.
//! * `refund` and `resolve` operate on the **remaining** balance only.
//! * The existing `release` entrypoint pays the full remaining balance in
//!   one shot and is unchanged; it still transitions directly to `Completed`.
//!
//! ## Multi-asset baskets
//!
//! A single token per escrow cannot express the trades that actually happen:
//! a listing settled in a primary token plus a supplementary voucher, or a
//! swap where each side posts a different asset. `create_basket` +
//! `deposit_basket` / `release_basket` / `refund_basket` / `resolve_basket`
//! hold `Vec<EscrowAsset>` instead of one `(token, amount)` pair, under
//! `DataKey::Basket(id)`.
//!
//! **The single-token path is untouched.** `EscrowData`, `DataKey::Escrow(id)`,
//! and every existing event keep their exact shape, so a basket is additive
//! and no indexer, ABI consumer, or stored record has to change. Both record
//! kinds draw from the **same** `DataKey::Count` id sequence, so an id names
//! exactly one escrow of exactly one kind and the two can never collide.
//!
//! The two kinds are *not* two state machines. Every transfer shape the
//! contract has — pull the whole basket in, pay the whole basket out, pay one
//! leg out — is a single free function ([`pull_assets`], [`payout_assets`],
//! [`payout_part`]) that both kinds call. A single-token escrow reaches them
//! through a one-element asset list, so the terminal paths cannot drift:
//! there is one conservation rule, not two.
//!
//! ### Decisions
//!
//! 1. **Partial-deposit policy: all-or-nothing.** `deposit_basket` pulls every
//!    leg in list order and the host frame rolls the whole invocation back if
//!    any leg fails, so the basket never becomes `Funded` holding only some of
//!    its assets. A half-funded basket would make every downstream balance
//!    ambiguous, and "which assets made it in" would have to be re-derived by
//!    every consumer. The caller sees the escrow still `Pending` and retries
//!    the whole deposit.
//! 2. **Duplicate tokens: rejected, not merged.** `create_basket` returns
//!    `InvalidInput` when a token appears twice. Merging would make the result
//!    depend on merge order and would silently change what the buyer agreed to
//!    authorize; rejecting keeps the basket a literal description of the terms.
//! 3. **Basket size: at most [`MAX_BASKET_ASSETS`] (8) legs.** Each leg is a
//!    cross-contract call plus a stored entry, and both are metered by the
//!    ledger. The bound keeps one escrow inside Soroban's instruction and
//!    state budgets, and the pairwise duplicate scan is quadratic but trivial
//!    at this size.
//! 4. **TTL: per escrow, never per asset.** The whole basket is one persistent
//!    entry, so `touch_ttl` extends it as a unit and a basket can never be
//!    half-expired — no asset can outlive or expire without the others.
//! 5. **Resolve is all-or-nothing per asset, one direction for all.** A
//!    basket resolve pays *every* leg to the seller or *every* leg to the
//!    buyer, mirroring the existing `bool` decision. Per-asset or
//!    basis-point splits are deliberately out of scope (they are the separate
//!    split-award feature); a `bool` cannot honestly express them.
//!
//! ### Accounting
//!
//! Each [`EscrowAsset`] carries its own `amount` / `released` pair, so
//! per-asset accounting is the *same* rule as the single-token `EscrowData`
//! accounting it generalizes (`remaining = amount - released`) applied per leg.
//! A basket reaches `Completed` only when **every** leg is fully released; a
//! partial release that empties one leg of a two-leg basket leaves the basket
//! `Funded` with a zero-balance leg. `refund` and `resolve` settle whatever
//! remains across all legs in list order.
//!
//! ## Storage compatibility
//!
//! `EscrowData` now carries a `released: i128` field. Records written by
//! earlier contract versions do not contain this field. Soroban
//! `#[contracttype]` structs are stored as XDR symbol-keyed maps; the host's
//! `map_unpack_to_slice` rejects any map whose entry count differs from the
//! struct's field count. To stay backward compatible while keeping
//! `DataKey::Escrow(id)` unchanged, `load_escrow` attempts to deserialize the
//! stored value as the current `EscrowData` first, and on failure falls back
//! to deserializing as `EscrowDataV1` (the pre-partial-release shape) and
//! converts it by defaulting `released` to `0`. No migration is required;
//! the conversion happens lazily on first read, and the upgraded record is
//! written back at the same key on the next state-changing call.
//!
//! **Document of record:** the chosen strategy is a two-type fallback decode
//! inside `load_escrow`. `EscrowDataV1` is the exact nine-field struct that
//! existed before this feature. It carries no `released` field and is never
//! written by new code. On a successful `EscrowDataV1` decode the result is
//! immediately upcast to `EscrowData` with `released = 0`.
//!
//! ## Events
//!
//! A new `PartiallyReleased` event is emitted by `release_partial` rather
//! than reusing `Released`. The two events carry different semantics: a
//! `Released` event signals terminal completion; a `PartiallyReleased` event
//! signals an incremental payout with a non-zero remaining balance (except
//! when it is also the final partial, in which case the `EscrowData` in the
//! payload will carry `status = Completed`). Extending the existing `Released`
//! event would silently break existing indexers that treat `Released` as a
//! terminal signal.
//!
//! ## Custody model
//!
//! On `deposit`, the token contract moves `amount` from the buyer to this
//! contract's own address. From that moment the funds are inside the
//! contract and can only leave via `release` / `release_partial` (to
//! seller), `refund` (to buyer), or `resolve` (either, by arbiter decision).
//! There is no admin key and no other exit.
//!
//! ## Ordering discipline (load-bearing)
//!
//! Every method that moves tokens performs the token transfer **first**
//! and writes state **after** the transfer succeeds. A failed transfer
//! reverts the whole invocation with storage untouched — there is no
//! state/ledger divergence window and no recovery path needed. The
//! inverse ordering (state first, transfer second) would strand funds
//! behind a failed transfer and is the classic escrow bug.
//!
//! ## Authorization model
//!
//! - `create_escrow` — **buyer only**. The seller is recorded but does
//!   not authorize creation: nothing of theirs is at risk before funding,
//!   their consent is expressed by the refund path (they can return funds
//!   at any time pre-deadline), and requiring their signature at creation
//!   forces a two-signer transaction that is hostile to wallets and CLI
//!   flows (this was learned live: a two-auth create produced `TxBadAuth`
//!   on every standard signing path during the testnet demo).
//! - `deposit` — buyer authorizes; their authorization covers the nested
//!   token pull.
//! - `release` — seller authorizes, confirming delivery. The buyer
//!   authorizing their own payout would make this a confirmation flow,
//!   not escrow.
//! - `release_partial` — **seller authorizes**, same rationale as `release`.
//!   Invalid requests (zero/negative amount, amount > remaining, non-Funded
//!   status) return `InvalidInput` without modifying storage.
//! - `refund` — seller before the deadline; buyer may reclaim after the
//!   deadline.
//! - `dispute` — the **claimant** (buyer or seller) is passed explicitly
//!   and must be one of the two parties; their `require_auth` proves the
//!   claim. Soroban has no "auth by A-or-B" primitive, so an explicit
//!   claimant parameter is the honest way to express either-party
//!   authorization.
//! - `resolve` — arbiter only; the decision is final.
//! - `cancel` — buyer, while `Pending`.
//!
//! ## Storage and TTL
//!
//! Each escrow is its own **persistent** entry (`DataKey::Escrow(id)`)
//! so the byte budget scales per record. Every participant's creation-order
//! index is likewise a **persistent** per-party entry
//! (`DataKey::ParticipantIndex(Address)`), written once per escrow creation:
//! a party's id list grows with their escrow count, so it cannot live in
//! instance storage (one party's growth would tax every shared instance
//! read). The only instance entry is the id counter. Every write bumps the
//! entry's TTL with the standard threshold/extend-to pattern, and
//! `touch_ttl` is a permissionless keeper entrypoint for escrows that sit
//! idle near expiry.
//!
//! The participant index contains every non-cancelled escrow for each
//! distinct buyer, seller, or arbiter. A successful `cancel` removes its id
//! from each distinct party's list after state and authorization checks,
//! preserving survivor order. Other terminal paths (`release`, `refund`,
//! and `resolve`) retain their ids because those escrow records remain
//! addressable. Pagination is live offset/limit, not a frozen snapshot:
//! removing an earlier id shifts later ids left, so a saved cursor can skip
//! an id. Clients should restart from cursor zero after mutation.

// WASM target guard: SDK 27 contracts must be built for wasm32v1-none.
// wasm32-unknown-unknown (os=unknown) can emit features the Soroban
// runtime rejects; wasm32v1-none (os=none) is the supported target.
#[cfg(all(target_family = "wasm", not(target_os = "none")))]
compile_error!(
    "build for wasm32v1-none (see rust-toolchain.toml); wasm32-unknown-unknown is not supported by the Soroban runtime"
);

use soroban_sdk::{
    contract, contractclient, contractevent, contractimpl, contracttype, Address, Env, Vec,
};

use soroban_forge_shared_utils::{bump_entry as shared_bump_entry, ForgeError};

/// Ledger-time constants for TTL bumps.
///
/// One ledger closes roughly every 5 seconds, so 17,280 ledgers ≈ 1 day.
/// `BUMP_AMOUNT` is the lifetime written on every touch; `BUMP_THRESHOLD`
/// is how close to expiry an entry must be before a bump applies. The
/// 30-day horizon comfortably covers a funded escrow between keeper
/// touches.
mod ttl {
    pub const DAY_IN_LEDGERS: u32 = 17_280;
    /// Lifetime applied on every TTL touch.
    pub const BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
    /// Bump only when the entry is within this window of expiring.
    pub const BUMP_THRESHOLD: u32 = BUMP_AMOUNT - DAY_IN_LEDGERS;
}

/// Maximum number of legs one basket escrow may hold.
///
/// Every leg is a cross-contract SEP-41 call plus a stored entry, and both are
/// metered by the ledger: a deposit of an `N`-leg basket costs `N` invocations
/// and a payout costs another `N`. The bound keeps a single escrow's worst
/// case inside Soroban's instruction budget, and it bounds the pairwise
/// duplicate-token scan performed at creation.
const MAX_BASKET_ASSETS: u32 = 8;

/// Public interface for the Soroban Forge escrow contract.
#[contractclient(name = "SorobanForgeEscrowClient")]
pub trait SorobanForgeEscrow {
    /// Create a new escrow and return its stable id.
    ///
    /// Requires `amount > 0`, `timeout > 0`. Only the buyer authorizes
    /// creation (see the authorization model in the module docs); the
    /// seller takes no risk until funding occurs.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::InvalidInput`] — non-positive amount or zero timeout.
    /// * [`ForgeError::ArithmeticOverflow`] — the id counter overflowed.
    fn create_escrow(
        env: Env,
        buyer: Address,
        seller: Address,
        arbiter: Address,
        token: Address,
        amount: i128,
        timeout: u64,
    ) -> Result<u64, ForgeError>;

    /// Fund the escrow, pulling `amount` of the escrow's token from the
    /// buyer into this contract. Requires the buyer; only valid while
    /// `Pending`.
    ///
    /// The token transfer is performed **before** any state is written, so
    /// a failed transfer leaves no partial state (see module docs).
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no escrow with this id.
    /// * [`ForgeError::InvalidInput`] — escrow is not `Pending`.
    /// * [`ForgeError::TokenTransferFailed`] — the token contract rejected
    ///   the transfer (insufficient balance, missing trustline, deauthorized
    ///   token, or undeployed token contract).
    fn deposit(env: Env, escrow_id: u64) -> Result<(), ForgeError>;

    /// Release the full remaining balance to the seller. Requires the
    /// seller (confirms delivery); only valid while `Funded`.
    ///
    /// Equivalent to calling `release_partial` with the full remaining
    /// balance, but in a single call. Backward-compatible with pre-partial-
    /// release code: behaves identically to the old `release` when no
    /// partial releases have been made.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no escrow with this id.
    /// * [`ForgeError::InvalidInput`] — escrow is not `Funded`.
    /// * [`ForgeError::TokenTransferFailed`] — the token contract rejected
    ///   the payout.
    fn release(env: Env, escrow_id: u64) -> Result<(), ForgeError>;

    /// Release a partial amount to the seller. Requires the seller;
    /// only valid while `Funded`.
    ///
    /// * `amount` must be positive and must not exceed the remaining balance
    ///   (`deposited - released`). Invalid requests return `InvalidInput`
    ///   without modifying any storage.
    /// * When `amount == remaining`, the escrow transitions to `Completed`.
    /// * Emits a `PartiallyReleased` event (even on the final partial that
    ///   completes the escrow).
    ///
    /// The token transfer is performed **before** any state write (transfer-
    /// before-state ordering).
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no escrow with this id.
    /// * [`ForgeError::InvalidInput`] — escrow is not `Funded`, `amount <= 0`,
    ///   or `amount > remaining`.
    /// * [`ForgeError::TokenTransferFailed`] — the token contract rejected
    ///   the payout.
    fn release_partial(env: Env, escrow_id: u64, amount: i128) -> Result<(), ForgeError>;

    /// Refund the buyer.
    ///
    /// Before the deadline the seller may refund; after the deadline the
    /// buyer may reclaim. Only valid while `Funded`. Refunds the
    /// **remaining** balance only (i.e., `deposited - released`).
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no escrow with this id.
    /// * [`ForgeError::InvalidInput`] — escrow is not `Funded`.
    /// * [`ForgeError::Unauthorized`] — wrong party for the current phase.
    /// * [`ForgeError::TokenTransferFailed`] — the token contract rejected
    ///   the payout.
    fn refund(env: Env, escrow_id: u64) -> Result<(), ForgeError>;

    /// Raise a dispute. `claimant` must be the buyer or the seller and
    /// must authorize the call; only valid while `Funded`. Freezes the
    /// escrow until the arbiter resolves it. Only the **remaining** balance
    /// is at stake.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no escrow with this id.
    /// * [`ForgeError::InvalidInput`] — escrow is not `Funded`, or the
    ///   claimant is neither buyer nor seller.
    /// * [`ForgeError::Unauthorized`] — the claimant did not authorize.
    fn dispute(env: Env, escrow_id: u64, claimant: Address) -> Result<(), ForgeError>;

    /// Resolve a dispute. Requires the arbiter; only valid while
    /// `Disputed`. Pays the **remaining** balance to the seller (`true`)
    /// or back to the buyer (`false`). The decision is final.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no escrow with this id.
    /// * [`ForgeError::InvalidInput`] — escrow is not `Disputed`.
    /// * [`ForgeError::Unauthorized`] — caller is not the arbiter.
    /// * [`ForgeError::TokenTransferFailed`] — the token contract rejected
    ///   the payout.
    fn resolve(env: Env, escrow_id: u64, in_favor_of_seller: bool) -> Result<(), ForgeError>;

    /// Resolve a dispute with a basis-point split between seller and buyer.
    ///
    /// Requires the arbiter; only valid while `Disputed` and only for
    /// `seller_bps <= 10_000`. The seller share is computed as
    /// `amount * seller_bps / 10_000`, with the remainder retained by the
    /// buyer. The split is final and settles both transfers atomically.
    fn resolve_split(env: Env, escrow_id: u64, seller_bps: u32) -> Result<(), ForgeError>;

    /// Cancel a `Pending` escrow before it is funded. Requires the buyer.
    /// Cancel a `Pending` escrow before it is funded. Requires the buyer and
    /// removes its id from each distinct party's participant index.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no escrow with this id.
    /// * [`ForgeError::InvalidInput`] — escrow is not `Pending`.
    fn cancel(env: Env, escrow_id: u64) -> Result<(), ForgeError>;

    // -------------------------------------------------------------------
    // Multi-asset baskets
    //
    // Additive: the single-token entrypoints above keep their exact behavior
    // and wire shapes. Every transfer shape is shared with the single-token
    // path (see the module docs on baskets).
    // -------------------------------------------------------------------

    /// Create a basket escrow over `assets` and return its stable id.
    ///
    /// Only the buyer authorizes, exactly as for [`Self::create_escrow`]: the
    /// seller and arbiter take no risk before funding. The id is drawn from
    /// the same sequence as a single-token escrow's.
    ///
    /// Each asset must carry `amount > 0`, the list must hold between 1 and
    /// [`MAX_BASKET_ASSETS`] legs, and no two legs may name the same token
    /// (duplicates are rejected, not merged — see the module docs). Validation
    /// runs before authorization.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::InvalidInput`] — zero timeout, an empty or oversized
    ///   basket, a non-positive leg amount, or a repeated token.
    /// * [`ForgeError::ArithmeticOverflow`] — the id counter overflowed.
    fn create_basket(
        env: Env,
        buyer: Address,
        seller: Address,
        arbiter: Address,
        assets: Vec<EscrowAsset>,
        timeout: u64,
    ) -> Result<u64, ForgeError>;

    /// Fund the basket, pulling **every** leg from the buyer into this
    /// contract. Requires the buyer; only valid while `Pending`.
    ///
    /// All-or-nothing: the legs are pulled in list order and the host frame
    /// rolls the whole invocation back if any leg fails, so a basket never
    /// becomes `Funded` holding only part of its assets. The buyer may need a
    /// trustline on each token.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no basket with this id.
    /// * [`ForgeError::InvalidInput`] — the basket is not `Pending`.
    /// * [`ForgeError::TokenTransferFailed`] — any leg was rejected; nothing
    ///   was pulled.
    fn deposit_basket(env: Env, escrow_id: u64) -> Result<(), ForgeError>;

    /// Release every leg's remaining balance to the seller. Requires the
    /// seller; only valid while `Funded`.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no basket with this id.
    /// * [`ForgeError::InvalidInput`] — the basket is not `Funded`.
    /// * [`ForgeError::TokenTransferFailed`] — a leg was rejected; no leg was
    ///   paid.
    fn release_basket(env: Env, escrow_id: u64) -> Result<(), ForgeError>;

    /// Release `amount` of a single leg to the seller. Requires the seller;
    /// only valid while `Funded`.
    ///
    /// The per-leg generalization of [`Self::release_partial`]: the same
    /// accounting, applied to one named leg. The basket transitions to
    /// `Completed` only when **every** leg is fully released, so emptying one
    /// leg of a multi-leg basket leaves it `Funded` with a zero-balance leg.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no basket with this id, or the basket does
    ///   not hold `token`.
    /// * [`ForgeError::InvalidInput`] — the basket is not `Funded`,
    ///   `amount <= 0`, or `amount > remaining` of that leg.
    /// * [`ForgeError::TokenTransferFailed`] — the token contract rejected the
    ///   payout.
    fn release_partial_basket(
        env: Env,
        escrow_id: u64,
        token: Address,
        amount: i128,
    ) -> Result<(), ForgeError>;

    /// Refund the buyer every remaining leg. Before the deadline the seller
    /// authorizes; after it the buyer does. Only valid while `Funded`.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no basket with this id.
    /// * [`ForgeError::InvalidInput`] — the basket is not `Funded`.
    /// * [`ForgeError::Unauthorized`] — wrong party for the current phase.
    /// * [`ForgeError::TokenTransferFailed`] — a leg was rejected; no leg was
    ///   paid.
    fn refund_basket(env: Env, escrow_id: u64) -> Result<(), ForgeError>;

    /// Raise a dispute on a basket. `claimant` must be the buyer or the seller
    /// and must authorize the call; only valid while `Funded`. Freezes **all**
    /// legs until the arbiter resolves.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no basket with this id.
    /// * [`ForgeError::InvalidInput`] — the basket is not `Funded`, or the
    ///   claimant is neither buyer nor seller.
    /// * [`ForgeError::Unauthorized`] — the claimant did not authorize.
    fn dispute_basket(env: Env, escrow_id: u64, claimant: Address) -> Result<(), ForgeError>;

    /// Resolve a basket dispute. Requires the arbiter; only valid while
    /// `Disputed`. Pays every leg's remaining balance to the seller (`true`)
    /// or to the buyer (`false`) — one direction for the whole basket. The
    /// decision is final.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no basket with this id.
    /// * [`ForgeError::InvalidInput`] — the basket is not `Disputed`.
    /// * [`ForgeError::Unauthorized`] — caller is not the arbiter.
    /// * [`ForgeError::TokenTransferFailed`] — a leg was rejected; no leg was
    ///   paid.
    fn resolve_basket(env: Env, escrow_id: u64, in_favor_of_seller: bool)
        -> Result<(), ForgeError>;

    /// Cancel a `Pending` basket before it is funded. Requires the buyer.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no basket with this id.
    /// * [`ForgeError::InvalidInput`] — the basket is not `Pending`.
    fn cancel_basket(env: Env, escrow_id: u64) -> Result<(), ForgeError>;

    /// Read the full basket record, including per-leg `released` accounting.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no **basket** with this id. A
    ///   single-token escrow id also reads as `NotFound` here: the two record
    ///   kinds are distinct types, and [`Self::get_escrow`] is the view for the
    ///   single-token one. [`Self::get_status`] covers either kind.
    fn get_basket(env: Env, escrow_id: u64) -> Result<BasketEscrowData, ForgeError>;

    /// Read the current lifecycle status of **either** record kind.
    ///
    /// Works for a single-token escrow and for a basket, so a status board
    /// needs one call per id regardless of which kind it is.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no escrow of either kind with this id.
    fn get_status(env: Env, escrow_id: u64) -> Result<EscrowStatus, ForgeError>;

    /// Read the full **single-token** escrow record, including `released` and
    /// derived `remaining` accounting.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no single-token escrow with this id. A
    ///   basket id reads as `NotFound` here because `EscrowData` cannot express
    ///   a basket; use [`Self::get_basket`] for those.
    fn get_escrow(env: Env, escrow_id: u64) -> Result<EscrowData, ForgeError>;

    /// List the non-cancelled escrow ids a participant is party to (buyer,
    /// seller, or arbiter), in creation order. Other terminal escrows remain
    /// listed because their records remain addressable.
    ///
    /// Read-only: requires no authorization and never mutates storage. An
    /// address with no escrows — or an address this contract has never seen
    /// — returns an empty page, not an error. Good for building "my
    /// escrows" views without an off-chain indexer.
    ///
    /// Pagination is a live offset/limit scheme. `cursor` is the offset in
    /// the current index when this call runs and `limit` caps the page size.
    /// The returned [`ParticipantEscrowsPage::next_cursor`] continues from
    /// that offset; `None` means the current list is exhausted. A `limit` of
    /// `0` returns an empty page with no next cursor. If cancellation removes
    /// an id before a saved cursor, later ids shift left and that cursor may
    /// skip an id. Restart at cursor `0` after any index mutation to enumerate
    /// the current list without omissions.
    ///
    /// # Errors
    ///
    /// This view never errors; unknown participants yield an empty page.
    fn escrows_for_participant(
        env: Env,
        participant: Address,
        cursor: u32,
        limit: u32,
    ) -> ParticipantEscrowsPage;

    /// Permissionless TTL keeper: bumps the escrow entry's TTL to the
    /// [`soroban_forge_shared_utils::ttl::BUMP_AMOUNT`] horizon when it
    /// falls inside [`soroban_forge_shared_utils::ttl::BUMP_THRESHOLD`].
    /// Call periodically for escrows that must
    /// outlive their entry's current TTL. Costs fees; changes nothing
    /// else.
    ///
    /// Covers both record kinds: a basket is one persistent entry, so it is
    /// extended as a unit and can never be half-expired (expiry is per escrow,
    /// never per asset).
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no escrow of either kind with this id.
    fn touch_ttl(env: Env, escrow_id: u64) -> Result<(), ForgeError>;

    /// Read the estimated remaining TTL of a present escrow record in ledgers.
    ///
    /// The view is read-only and does not extend either persistent entry.
    fn ttl_info(env: Env, escrow_id: u64) -> Result<u32, ForgeError>;
}

/// Lifecycle state of an escrow.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscrowStatus {
    /// Created but not funded.
    Pending,
    /// Tokens held by the contract.
    Funded,
    /// Released to the seller (full release or final partial release).
    Completed,
    /// Refunded to the buyer.
    Refunded,
    /// A party raised a dispute; frozen until the arbiter resolves.
    Disputed,
    /// Cancelled before funding.
    Cancelled,
}

/// A single three-party escrow record.
///
/// ## Accounting fields
///
/// * `amount` — total tokens deposited by the buyer; never changes after
///   `deposit`.
/// * `released` — cumulative tokens already transferred to the seller via
///   `release_partial`; `0` before any partial release is made.
/// * `remaining` (derived) — `amount - released`; the balance currently
///   held in custody. `refund` and `resolve` pay this amount; `release`
///   and the final `release_partial` must consume it entirely.
///
/// ## Storage compatibility
///
/// Records written by contract versions prior to the partial-release feature
/// do not contain a `released` field. They are loaded via a backward-compat
/// decode path that defaults `released` to `0` (see `load_escrow` internals
/// and the module-level storage compatibility note).
#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowData {
    /// Stable id, never reused.
    pub escrow_id: u64,
    /// Party funding the escrow and the default refund recipient.
    pub buyer: Address,
    /// Party paid on release.
    pub seller: Address,
    /// Neutral party deciding disputes. Recorded at creation; authorizes
    /// only `resolve`.
    pub arbiter: Address,
    /// SEP-41 token contract custodied by this escrow.
    pub token: Address,
    /// Total amount of `token` deposited by the buyer. Never changes after
    /// `deposit`.
    pub amount: i128,
    /// Cumulative amount already transferred to the seller via
    /// `release_partial`. `0` until the first partial release.
    pub released: i128,
    /// Seconds after `created_at` at which the buyer may self-refund.
    pub timeout: u64,
    /// Current lifecycle state.
    pub status: EscrowStatus,
    /// Unix timestamp of creation.
    pub created_at: u64,
}

impl EscrowData {
    /// The balance currently held in custody: `amount - released`.
    pub fn remaining(&self) -> i128 {
        self.amount
            .checked_sub(self.released)
            .expect("released never exceeds amount by contract invariant")
    }
}

/// Pre-partial-release escrow record shape (schema V1).
///
/// Used **only** by [`load_escrow`] for backward-compatible decoding of
/// storage entries written by contract versions that pre-date the
/// `released` field. New code never writes this type; it is a read-only
/// migration aid.
///
/// Soroban `#[contracttype]` structs are XDR symbol-keyed maps. The host
/// rejects deserialization when the map's entry count differs from the
/// struct's field count. Old records have 9 fields (no `released`), so they
/// fail to decode as the 10-field `EscrowData`. `load_escrow` catches that
/// failure and retries as `EscrowDataV1`, then upgrades to `EscrowData` with
/// `released = 0`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowDataV1 {
    /// Matches `EscrowData::escrow_id`.
    pub escrow_id: u64,
    /// Matches `EscrowData::buyer`.
    pub buyer: Address,
    /// Matches `EscrowData::seller`.
    pub seller: Address,
    /// Matches `EscrowData::arbiter`.
    pub arbiter: Address,
    /// Matches `EscrowData::token`.
    pub token: Address,
    /// Matches `EscrowData::amount`.
    pub amount: i128,
    /// Matches `EscrowData::timeout`.
    pub timeout: u64,
    /// Matches `EscrowData::status`.
    pub status: EscrowStatus,
    /// Matches `EscrowData::created_at`.
    pub created_at: u64,
}

impl From<EscrowDataV1> for EscrowData {
    fn from(v1: EscrowDataV1) -> Self {
        EscrowData {
            escrow_id: v1.escrow_id,
            buyer: v1.buyer,
            seller: v1.seller,
            arbiter: v1.arbiter,
            token: v1.token,
            amount: v1.amount,
            released: 0,
            timeout: v1.timeout,
            status: v1.status,
            created_at: v1.created_at,
        }
    }
}

/// One leg of a basket escrow: a single SEP-41 token and its accounting.
///
/// The field names mirror [`EscrowData`]'s single-token accounting exactly so
/// the two record kinds express one rule rather than two: `remaining` is
/// `amount - released`, `refund`/`resolve` pay `remaining`, and a full release
/// consumes it.
#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowAsset {
    /// SEP-41 token contract for this leg.
    pub token: Address,
    /// Total amount of `token` this leg escrows. Fixed at creation and
    /// unchanged by `deposit_basket`.
    pub amount: i128,
    /// Cumulative amount of `token` already paid to the seller via
    /// `release_partial_basket`. `0` until the first partial release.
    pub released: i128,
}

impl EscrowAsset {
    /// The balance of this leg currently held in custody: `amount - released`.
    pub fn remaining(&self) -> i128 {
        self.amount
            .checked_sub(self.released)
            .expect("released never exceeds amount by contract invariant")
    }
}

/// A basket escrow record: the same three-party lifecycle as [`EscrowData`],
/// with an ordered list of legs instead of one token.
///
/// Stored at [`DataKey::Basket`], a separate key class from
/// [`DataKey::Escrow`], so the single-token record's stored shape and the
/// events that carry it are untouched. Ids come from the same
/// [`DataKey::Count`] sequence, so an id belongs to exactly one escrow of
/// exactly one kind.
#[contracttype]
#[derive(Clone, Debug)]
pub struct BasketEscrowData {
    /// Stable id, never reused. Shares the sequence with single-token escrows.
    pub escrow_id: u64,
    /// Party funding the basket and the default refund recipient.
    pub buyer: Address,
    /// Party paid on release.
    pub seller: Address,
    /// Neutral party deciding disputes. Recorded at creation; authorizes only
    /// `resolve_basket`.
    pub arbiter: Address,
    /// The basket: one leg per token, in the order the buyer declared. No two
    /// legs name the same token.
    pub assets: Vec<EscrowAsset>,
    /// Seconds after `created_at` at which the buyer may self-refund.
    pub timeout: u64,
    /// Current lifecycle state. Identical semantics to [`EscrowStatus`].
    pub status: EscrowStatus,
    /// Unix timestamp of creation.
    pub created_at: u64,
}

impl BasketEscrowData {
    /// Whether every leg has been paid out in full. A basket is `Completed`
    /// exactly when this holds, mirroring how a single-token escrow completes
    /// when its one `remaining` reaches zero.
    pub fn fully_released(&self) -> bool {
        self.assets.iter().all(|asset| asset.remaining() == 0)
    }
}

/// A page of escrow ids involving a participant.
///
/// Returned by [`SorobanForgeEscrow::escrows_for_participant`]; powered by
/// the per-party persistent index written at creation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticipantEscrowsPage {
    /// Escrow ids on this page, in creation order.
    pub ids: Vec<u64>,
    /// Total escrows involving the participant across all pages.
    pub total: u32,
    /// Offset for the next page, or `None` when `ids` ends the participant's
    /// list. Replay it until `None` to iterate the whole list.
    pub next_cursor: Option<u32>,
}

/// Storage keys. Escrow records are per-id **persistent** entries so the
/// byte budget scales per record; only the id counter lives in instance
/// storage (one small entry, written once per creation).
#[contracttype]
pub enum DataKey {
    /// The escrow record for `u64` id. Always a single-token [`EscrowData`]
    /// (or a legacy `EscrowDataV1` awaiting lazy upgrade).
    Escrow(u64),
    /// The basket escrow record for `u64` id, always a [`BasketEscrowData`].
    ///
    /// A separate key class from [`DataKey::Escrow`] rather than a new variant
    /// of the same key's value: an escrow id names exactly one record of
    /// exactly one kind, so the two shapes can never be confused on read, and
    /// the stored bytes of every pre-existing single-token escrow — and of
    /// every event payload carrying one — stay exactly as they were. The id
    /// itself is drawn from the shared [`DataKey::Count`] sequence, so the two
    /// classes can never mint the same id.
    Basket(u64),
    /// Monotonic id counter.
    Count,
    /// Creation-order ids of every non-cancelled escrow the `Address`
    /// participates in (as buyer, seller, or arbiter). Written at creation
    /// and removed only when a pending escrow is cancelled.
    ///
    /// Persistent, not instance: the entry grows with that party's escrow
    /// count and is written only on the create path, so parking it in
    /// instance storage would bloat a shared hot entry with one party's
    /// growth (the same per-record-scaling argument that puts `Escrow(id)`
    /// in persistent storage rather than a single instance map). It also
    /// carries its own extensible TTL under the standard `bump_entry`
    /// threshold/extend pattern, mirroring every other persistent write.
    ///
    /// The value is a single `Vec<u64>` appended in creation order rather
    /// than sharded per-party keys (`ParticipantIndex(Address, u64)`).
    /// Tradeoff: one entry per party keeps reads cheap — a page is one
    /// entry read plus an in-memory slice — and appends are one read + one
    /// rewrite of that party's (id-sized, 8 bytes each) list. The cost is
    /// that a party's entry grows unboundedly and each append rewrites the
    /// whole list; for the overwhelming majority of parties the list stays
    /// tiny, and a party accumulating so many escrows that a single entry
    /// fills (Soroban's ~64 KB entry cap is tens of thousands of ids)
    /// would migrate to a sharded scheme — a compatible upgrade since the
    /// view only ever reads through this key class.
    ParticipantIndex(Address),
    /// Expiration ledger mirrored for an escrow record because SDK 27
    /// exposes TTL introspection to test utilities but not contract code.
    EscrowExpiration(u64),
}

/// The deployable escrow contract.
#[contract]
pub struct Escrow;

#[contractimpl]
impl Escrow {
    // -------------------------------------------------------------------
    // Lifecycle
    // -------------------------------------------------------------------

    /// Create a new escrow and return its stable id.
    ///
    /// Only the buyer authorizes at creation. The arbiter does not
    /// authorize either: they must be able to `resolve` later even if
    /// they never participated in creation.
    pub fn create_escrow(
        env: Env,
        buyer: Address,
        seller: Address,
        arbiter: Address,
        token: Address,
        amount: i128,
        timeout: u64,
    ) -> Result<u64, ForgeError> {
        if amount <= 0 {
            return Err(ForgeError::InvalidInput);
        }
        if timeout == 0 {
            return Err(ForgeError::InvalidInput);
        }
        // Buyer-only authorization: a two-signer create (buyer + seller)
        // was tried live on testnet and failed on every standard signing
        // path (`TxBadAuth` from the CLI, `TxMalformed` from manually
        // chained signatures). The seller loses nothing by being recorded
        // without consenting — their protections are the refund and
        // dispute paths once funded.
        buyer.require_auth();

        let id = Self::next_id(&env)?;
        // Distinct parties only: one address in several roles (e.g. seller
        // == arbiter) is indexed once so iteration yields this id exactly
        // once, per the index's read contract.
        let participants = distinct_participants(&env, &buyer, &seller, &arbiter);
        let escrow = EscrowData {
            escrow_id: id,
            buyer,
            seller,
            arbiter,
            token,
            amount,
            released: 0,
            timeout,
            status: EscrowStatus::Pending,
            created_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Escrow(id), &escrow);
        bump_entry(&env, &DataKey::Escrow(id));
        // Index write joins the rest of the success-path writes: it runs
        // after every fallible step (validation, `require_auth`, id
        // allocation), so it cannot observe or create partial state.
        Self::index_participants(&env, id, &participants);
        events::escrow_created(&env, &escrow);
        Ok(id)
    }

    /// Fund the escrow, pulling tokens from the buyer into this contract.
    ///
    /// Ordering: transfer **first**, state write **second** — see the
    /// module docs for why the inverse would be a fund-safety bug.
    pub fn deposit(env: Env, escrow_id: u64) -> Result<(), ForgeError> {
        let escrow = Self::load_escrow(&env, escrow_id)?;
        escrow.buyer.require_auth();

        if escrow.status != EscrowStatus::Pending {
            return Err(ForgeError::InvalidInput);
        }

        // Pull the tokens before writing any state. If the buyer lacks
        // balance or a trustline the invocation reverts here with storage
        // untouched. A single-token escrow is just a one-leg basket here, so
        // the deposit path is literally the basket's.
        pull_assets(&env, &single_leg(&env, &escrow), &escrow.buyer)?;

        let mut funded = escrow;
        funded.status = EscrowStatus::Funded;
        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &funded);
        bump_entry(&env, &DataKey::Escrow(escrow_id));
        events::deposited(&env, &funded);
        Ok(())
    }

    /// Release the full remaining balance to the seller. Seller-authorized:
    /// delivery confirmation by the paid party, not the paying one.
    ///
    /// Backward-compatible: behaves identically to the pre-partial-release
    /// `release` when `released == 0`. When partial releases have already
    /// been made, only the **remaining** balance (`amount - released`) is
    /// transferred — maintaining the conservation invariant.
    pub fn release(env: Env, escrow_id: u64) -> Result<(), ForgeError> {
        let escrow = Self::load_escrow(&env, escrow_id)?;
        escrow.seller.require_auth();

        if escrow.status != EscrowStatus::Funded {
            return Err(ForgeError::InvalidInput);
        }

        // Pay out the remaining balance before mutating state. `payout_assets`
        // is the basket's payout loop; the single-token record supplies its one
        // leg and writes back the settled total the loop produced, so the two
        // kinds cannot disagree about what "fully released" means.
        let settled = payout_assets(&env, &single_leg(&env, &escrow), &escrow.seller)?;

        let mut completed = escrow;
        completed.released = settled.get_unchecked(0).released;
        completed.status = EscrowStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &completed);
        bump_entry(&env, &DataKey::Escrow(escrow_id));
        events::released(&env, &completed);
        Ok(())
    }

    /// Release a partial `amount` to the seller. Seller-authorized.
    ///
    /// Only valid while `Funded`. `amount` must be positive and must not
    /// exceed the remaining balance. When `amount == remaining`, the escrow
    /// transitions to `Completed` in the same call.
    ///
    /// Transfer-before-state ordering is preserved: the token payout happens
    /// before any storage write, so a failed transfer leaves no partial state.
    pub fn release_partial(env: Env, escrow_id: u64, amount: i128) -> Result<(), ForgeError> {
        let escrow = Self::load_escrow(&env, escrow_id)?;
        escrow.seller.require_auth();

        // All validation before any state change or transfer.
        if escrow.status != EscrowStatus::Funded {
            return Err(ForgeError::InvalidInput);
        }
        if amount <= 0 {
            return Err(ForgeError::InvalidInput);
        }
        let remaining = escrow.remaining();
        if amount > remaining {
            return Err(ForgeError::InvalidInput);
        }

        // Transfer first (transfer-before-state ordering), through the same
        // per-leg primitive the basket's partial release uses.
        let legs = single_leg(&env, &escrow);
        payout_part(&env, &legs, 0, &escrow.seller, amount)?;

        let new_released = escrow
            .released
            .checked_add(amount)
            .ok_or(ForgeError::ArithmeticOverflow)?;

        let mut updated = escrow;
        updated.released = new_released;
        // Final partial: consume remaining → Completed.
        if amount == remaining {
            updated.status = EscrowStatus::Completed;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &updated);
        bump_entry(&env, &DataKey::Escrow(escrow_id));
        events::partially_released(&env, &updated, amount);
        Ok(())
    }

    /// Refund the buyer.
    ///
    /// Pre-deadline: seller authorizes (voluntary refund). Post-deadline:
    /// buyer authorizes (reclaim of unfulfilled funds). Refunds only the
    /// **remaining** balance (`amount - released`).
    pub fn refund(env: Env, escrow_id: u64) -> Result<(), ForgeError> {
        let escrow = Self::load_escrow(&env, escrow_id)?;
        if escrow.status != EscrowStatus::Funded {
            return Err(ForgeError::InvalidInput);
        }

        let authorizer = refund_authorizer(
            &env,
            escrow.created_at,
            escrow.timeout,
            &escrow.buyer,
            &escrow.seller,
        )?;
        authorizer.require_auth();

        // Terminal payout through the shared loop; the settled leg reports the
        // full deposited amount as released, which is what zeroes `remaining()`
        // even after earlier partial releases.
        let settled = payout_assets(&env, &single_leg(&env, &escrow), &escrow.buyer)?;

        let mut refunded = escrow;
        refunded.released = settled.get_unchecked(0).released;
        refunded.status = EscrowStatus::Refunded;
        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &refunded);
        bump_entry(&env, &DataKey::Escrow(escrow_id));
        events::refunded(&env, &refunded);
        Ok(())
    }

    /// Raise a dispute: the claimant (buyer or seller) authorizes, while
    /// `Funded`. Freezes the remaining balance until the arbiter resolves.
    pub fn dispute(env: Env, escrow_id: u64, claimant: Address) -> Result<(), ForgeError> {
        let escrow = Self::load_escrow(&env, escrow_id)?;
        if escrow.status != EscrowStatus::Funded {
            return Err(ForgeError::InvalidInput);
        }
        // The claim must come from a party to the escrow, and the claimant
        // must have actually authorized this invocation. Requiring auth on
        // the claimant (not on buyer-then-seller) is the correct Soroban
        // idiom: the auth envelope is checked against exactly one address.
        if claimant != escrow.buyer && claimant != escrow.seller {
            return Err(ForgeError::InvalidInput);
        }
        claimant.require_auth();

        let mut disputed = escrow;
        disputed.status = EscrowStatus::Disputed;
        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &disputed);
        bump_entry(&env, &DataKey::Escrow(escrow_id));
        events::disputed(&env, &disputed);
        Ok(())
    }

    /// Resolve a dispute: arbiter only, final. Pays the **remaining**
    /// balance to the seller (`true`) or refunds it to the buyer (`false`).
    pub fn resolve(env: Env, escrow_id: u64, in_favor_of_seller: bool) -> Result<(), ForgeError> {
        let escrow = Self::load_escrow(&env, escrow_id)?;
        if escrow.status != EscrowStatus::Disputed {
            return Err(ForgeError::InvalidInput);
        }
        // Arbiter auth is checked before any transfer: an unauthorized
        // resolve must fail without touching the token contract.
        escrow.arbiter.require_auth();

        let mut resolved = escrow;
        if in_favor_of_seller {
            let settled = payout_assets(&env, &single_leg(&env, &resolved), &resolved.seller)?;
            resolved.released = settled.get_unchecked(0).released;
            resolved.status = EscrowStatus::Completed;
        } else {
            let settled = payout_assets(&env, &single_leg(&env, &resolved), &resolved.buyer)?;
            resolved.released = settled.get_unchecked(0).released;
            resolved.status = EscrowStatus::Refunded;
        }
        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &resolved);
        bump_entry(&env, &DataKey::Escrow(escrow_id));
        events::resolved(&env, &resolved, in_favor_of_seller);
        Ok(())
    }

    /// Resolve a dispute by splitting the escrowed amount between the seller
    /// and buyer according to `seller_bps` out of 10_000.
    ///
    /// The seller share is floored at `amount * seller_bps / 10_000` and the
    /// remainder stays with the buyer. `seller_bps == 0` resolves to a
    /// buyer-only refund and `seller_bps == 10_000` resolves to a full seller
    /// payout. A failed second payout rolls back the whole invocation.
    pub fn resolve_split(env: Env, escrow_id: u64, seller_bps: u32) -> Result<(), ForgeError> {
        let escrow = Self::load_escrow(&env, escrow_id)?;
        if escrow.status != EscrowStatus::Disputed {
            return Err(ForgeError::InvalidInput);
        }
        if seller_bps > 10_000 {
            return Err(ForgeError::InvalidInput);
        }
        escrow.arbiter.require_auth();

        let remaining = escrow.remaining();
        let (seller_share, buyer_share) = split_amount(remaining, seller_bps)?;
        let mut resolved = escrow;

        if seller_share > 0 {
            transfer_from_contract(&env, &resolved.token, &resolved.seller, seller_share)?;
        }
        if buyer_share > 0 {
            transfer_from_contract(&env, &resolved.token, &resolved.buyer, buyer_share)?;
        }

        resolved.released = resolved.amount;
        resolved.status = if seller_share == 0 {
            EscrowStatus::Refunded
        } else {
            EscrowStatus::Completed
        };

        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &resolved);
        bump_entry(&env, &DataKey::Escrow(escrow_id));
        events::resolved_split(&env, &resolved, seller_bps, seller_share, buyer_share);
        Ok(())
    }

    /// Cancel a `Pending` escrow. Requires the buyer. Nothing has moved,
    /// so no token transfer occurs.
    pub fn cancel(env: Env, escrow_id: u64) -> Result<(), ForgeError> {
        let escrow = Self::load_escrow(&env, escrow_id)?;
        if escrow.status != EscrowStatus::Pending {
            return Err(ForgeError::InvalidInput);
        }
        escrow.buyer.require_auth();

        let mut cancelled = escrow;
        cancelled.status = EscrowStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &cancelled);
        bump_entry(&env, &DataKey::Escrow(escrow_id));
        // All fallible checks, including buyer auth, are complete. Remove the
        // id from each distinct participant index on the success path.
        Self::remove_participant_indexes(&env, escrow_id, &cancelled);
        events::cancelled(&env, &cancelled);
        Ok(())
    }

    // -------------------------------------------------------------------
    // Multi-asset baskets
    // -------------------------------------------------------------------

    /// Create a basket escrow. Buyer-authorized only, exactly as
    /// [`Self::create_escrow`], and drawing from the same id sequence.
    pub fn create_basket(
        env: Env,
        buyer: Address,
        seller: Address,
        arbiter: Address,
        assets: Vec<EscrowAsset>,
        timeout: u64,
    ) -> Result<u64, ForgeError> {
        // Validation before authorization, matching the single-token create
        // path: a malformed basket is rejected on its own terms rather than
        // after a signature has been solicited for it.
        if timeout == 0 {
            return Err(ForgeError::InvalidInput);
        }
        validate_basket(&assets)?;
        // Same single-signer rationale as `create_escrow`: the seller loses
        // nothing by not consenting before funding.
        buyer.require_auth();

        let id = Self::next_id(&env)?;
        let participants = distinct_participants(&env, &buyer, &seller, &arbiter);
        let basket = BasketEscrowData {
            escrow_id: id,
            buyer,
            seller,
            arbiter,
            assets,
            timeout,
            status: EscrowStatus::Pending,
            created_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Basket(id), &basket);
        bump_entry(&env, &DataKey::Basket(id));
        // Same post-validation position as the single-token create path: the
        // index write cannot observe or create partial state.
        Self::index_participants(&env, id, &participants);
        events::basket_created(&env, &basket);
        Ok(id)
    }

    /// Fund a basket: pull every leg from the buyer. All-or-nothing.
    pub fn deposit_basket(env: Env, escrow_id: u64) -> Result<(), ForgeError> {
        let basket = Self::load_basket(&env, escrow_id)?;
        basket.buyer.require_auth();

        if basket.status != EscrowStatus::Pending {
            return Err(ForgeError::InvalidInput);
        }

        // Every leg before any state write. A failure on leg `k` reverts legs
        // `0..k` through host frame rollback, so the basket never becomes
        // Funded holding part of its assets.
        pull_assets(&env, &basket.assets, &basket.buyer)?;

        let mut funded = basket;
        funded.status = EscrowStatus::Funded;
        env.storage()
            .persistent()
            .set(&DataKey::Basket(escrow_id), &funded);
        bump_entry(&env, &DataKey::Basket(escrow_id));
        events::basket_deposited(&env, &funded);
        Ok(())
    }

    /// Pay every leg's remaining balance to the seller. Seller-authorized.
    pub fn release_basket(env: Env, escrow_id: u64) -> Result<(), ForgeError> {
        let basket = Self::load_basket(&env, escrow_id)?;
        basket.seller.require_auth();

        if basket.status != EscrowStatus::Funded {
            return Err(ForgeError::InvalidInput);
        }

        let settled = payout_assets(&env, &basket.assets, &basket.seller)?;

        let mut completed = basket;
        completed.assets = settled;
        completed.status = EscrowStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Basket(escrow_id), &completed);
        bump_entry(&env, &DataKey::Basket(escrow_id));
        events::basket_released(&env, &completed);
        Ok(())
    }

    /// Pay `amount` of one leg to the seller. Seller-authorized.
    ///
    /// All validation before any state change or transfer, mirroring
    /// `release_partial`.
    pub fn release_partial_basket(
        env: Env,
        escrow_id: u64,
        token: Address,
        amount: i128,
    ) -> Result<(), ForgeError> {
        let basket = Self::load_basket(&env, escrow_id)?;
        basket.seller.require_auth();

        if basket.status != EscrowStatus::Funded {
            return Err(ForgeError::InvalidInput);
        }
        if amount <= 0 {
            return Err(ForgeError::InvalidInput);
        }
        // Resolve the leg before validating the amount against it, so an
        // unknown token cannot be reported as a balance problem.
        let at = leg_index(&basket.assets, &token)?;
        let leg = basket.assets.get_unchecked(at);
        if amount > leg.remaining() {
            return Err(ForgeError::InvalidInput);
        }

        // Transfer first (transfer-before-state ordering).
        payout_part(&env, &basket.assets, at, &basket.seller, amount)?;

        let new_released = leg
            .released
            .checked_add(amount)
            .ok_or(ForgeError::ArithmeticOverflow)?;

        let mut assets = basket.assets.clone();
        let mut updated_leg = leg.clone();
        updated_leg.released = new_released;
        assets.set(at, updated_leg);

        let mut updated = basket;
        updated.assets = assets;
        // A basket completes only when *every* leg is exhausted, so emptying
        // one leg of a multi-leg basket leaves it Funded.
        if updated.fully_released() {
            updated.status = EscrowStatus::Completed;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Basket(escrow_id), &updated);
        bump_entry(&env, &DataKey::Basket(escrow_id));
        events::basket_partially_released(&env, &updated, &token, amount);
        Ok(())
    }

    /// Pay every leg's remaining balance back to the buyer. Seller-authorized
    /// before the deadline, buyer-authorized after, as for a single-token
    /// escrow.
    pub fn refund_basket(env: Env, escrow_id: u64) -> Result<(), ForgeError> {
        let basket = Self::load_basket(&env, escrow_id)?;
        if basket.status != EscrowStatus::Funded {
            return Err(ForgeError::InvalidInput);
        }

        let authorizer = refund_authorizer(
            &env,
            basket.created_at,
            basket.timeout,
            &basket.buyer,
            &basket.seller,
        )?;
        authorizer.require_auth();

        let settled = payout_assets(&env, &basket.assets, &basket.buyer)?;

        let mut refunded = basket;
        refunded.assets = settled;
        refunded.status = EscrowStatus::Refunded;
        env.storage()
            .persistent()
            .set(&DataKey::Basket(escrow_id), &refunded);
        bump_entry(&env, &DataKey::Basket(escrow_id));
        events::basket_refunded(&env, &refunded);
        Ok(())
    }

    /// Raise a dispute on a basket. The claimant must be a party and must
    /// authorize the call; freezes every leg until the arbiter resolves.
    pub fn dispute_basket(env: Env, escrow_id: u64, claimant: Address) -> Result<(), ForgeError> {
        let basket = Self::load_basket(&env, escrow_id)?;
        if basket.status != EscrowStatus::Funded {
            return Err(ForgeError::InvalidInput);
        }
        if claimant != basket.buyer && claimant != basket.seller {
            return Err(ForgeError::InvalidInput);
        }
        claimant.require_auth();

        let mut disputed = basket;
        disputed.status = EscrowStatus::Disputed;
        env.storage()
            .persistent()
            .set(&DataKey::Basket(escrow_id), &disputed);
        bump_entry(&env, &DataKey::Basket(escrow_id));
        events::basket_disputed(&env, &disputed);
        Ok(())
    }

    /// Resolve a basket dispute: arbiter only, final, one direction for every
    /// leg. Payout goes through the same loop as `release_basket`, so a
    /// resolve can never settle legs differently than a release.
    pub fn resolve_basket(
        env: Env,
        escrow_id: u64,
        in_favor_of_seller: bool,
    ) -> Result<(), ForgeError> {
        let basket = Self::load_basket(&env, escrow_id)?;
        if basket.status != EscrowStatus::Disputed {
            return Err(ForgeError::InvalidInput);
        }
        // Arbiter auth before any transfer: an unauthorized resolve must fail
        // without touching a single token contract.
        basket.arbiter.require_auth();

        let mut resolved = basket;
        let (payee, status) = if in_favor_of_seller {
            (resolved.seller.clone(), EscrowStatus::Completed)
        } else {
            (resolved.buyer.clone(), EscrowStatus::Refunded)
        };
        let settled = payout_assets(&env, &resolved.assets, &payee)?;
        resolved.assets = settled;
        resolved.status = status;

        env.storage()
            .persistent()
            .set(&DataKey::Basket(escrow_id), &resolved);
        bump_entry(&env, &DataKey::Basket(escrow_id));
        events::basket_resolved(&env, &resolved, in_favor_of_seller);
        Ok(())
    }

    /// Cancel a `Pending` basket. Buyer-authorized; nothing has moved, so no
    /// transfer occurs.
    pub fn cancel_basket(env: Env, escrow_id: u64) -> Result<(), ForgeError> {
        let basket = Self::load_basket(&env, escrow_id)?;
        if basket.status != EscrowStatus::Pending {
            return Err(ForgeError::InvalidInput);
        }
        basket.buyer.require_auth();

        let mut cancelled = basket;
        cancelled.status = EscrowStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Basket(escrow_id), &cancelled);
        bump_entry(&env, &DataKey::Basket(escrow_id));
        events::basket_cancelled(&env, &cancelled);
        Ok(())
    }

    // -------------------------------------------------------------------
    // Views
    // -------------------------------------------------------------------

    /// Read a basket record.
    pub fn get_basket(env: Env, escrow_id: u64) -> Result<BasketEscrowData, ForgeError> {
        Self::load_basket(&env, escrow_id)
    }

    /// Read the current lifecycle status of either record kind.
    pub fn get_status(env: Env, escrow_id: u64) -> Result<EscrowStatus, ForgeError> {
        if let Ok(single) = Self::load_escrow(&env, escrow_id) {
            return Ok(single.status);
        }
        if let Ok(basket) = Self::load_basket(&env, escrow_id) {
            return Ok(basket.status);
        }
        Err(ForgeError::NotFound)
    }

    /// Read the full escrow record. The returned `EscrowData` exposes
    /// `released` (cumulative seller payments) and `remaining()` (balance
    /// in custody). For records created before the partial-release feature
    /// was deployed, `released` will be `0`.
    pub fn get_escrow(env: Env, escrow_id: u64) -> Result<EscrowData, ForgeError> {
        Self::load_escrow(&env, escrow_id)
    }

    /// Read the current creation-order escrow ids for a participant one page
    /// at a time. Cancellation removes ids from the compacted index. A cursor
    /// is an offset into the current list; if an earlier id is removed between
    /// pages, later ids may shift before the cursor. Restart at zero after
    /// mutations to enumerate the current list completely. This view itself
    /// never mutates storage and never errors — an unknown address or one with
    /// no escrows simply yields an empty page.
    pub fn escrows_for_participant(
        env: Env,
        participant: Address,
        cursor: u32,
        limit: u32,
    ) -> ParticipantEscrowsPage {
        let ids = env
            .storage()
            .persistent()
            .get(&DataKey::ParticipantIndex(participant))
            .unwrap_or_else(|| Vec::new(&env));
        let total = ids.len();
        // `cursor` may exceed `total`; clamp so an over-run returns an
        // empty page rather than panicking on a missing index.
        let (mut at, end) = match limit {
            // A zero limit must not report a next cursor that points at
            // itself forever; treat it as "list not requested".
            0 => (total, total),
            _ => (cursor.min(total), cursor.saturating_add(limit).min(total)),
        };
        let mut page = Vec::new(&env);
        while at < end {
            page.push_back(ids.get_unchecked(at));
            at += 1;
        }
        let next_cursor = if end < total { Some(end) } else { None };
        ParticipantEscrowsPage {
            ids: page,
            total,
            next_cursor,
        }
    }

    /// Permissionless keeper: bump the escrow entry's TTL without changing
    /// any state. The existence check is deliberate — touching a missing
    /// id must fail loudly so a keeper can distinguish "extended" from
    /// "no such escrow".
    pub fn touch_ttl(env: Env, escrow_id: u64) -> Result<(), ForgeError> {
        // The id belongs to exactly one record kind; bump whichever key holds
        // it. A basket is a single entry, so it is extended as a unit and can
        // never expire leg-by-leg.
        if let Ok(_basket) = Self::load_basket(&env, escrow_id) {
            bump_entry(&env, &DataKey::Basket(escrow_id));
        } else if let Ok(_single) = Self::load_escrow(&env, escrow_id) {
            bump_entry(&env, &DataKey::Escrow(escrow_id));
        } else {
            return Err(ForgeError::NotFound);
        }
        Ok(())
    }

    /// Return the remaining ledger count before the escrow record expires.
    ///
    /// `NotFound` means the id never existed or its persistent entry is
    /// already archived. This view performs no TTL extension.
    pub fn ttl_info(env: Env, escrow_id: u64) -> Result<u32, ForgeError> {
        Self::load_escrow(&env, escrow_id)?;
        let expiry: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::EscrowExpiration(escrow_id))
            .ok_or(ForgeError::NotFound)?;
        Ok(expiry.saturating_sub(env.ledger().sequence()))
    }

    // -------------------------------------------------------------------
    // Internals
    // -------------------------------------------------------------------

    /// Load an escrow record by id with backward-compatible schema migration.
    ///
    /// Attempt to deserialize as `EscrowData` (current schema, 10 fields
    /// including `released`). If that fails — which happens for records
    /// written by contract versions that pre-date the `released` field —
    /// retry as `EscrowDataV1` (9 fields, no `released`) and upcast to
    /// `EscrowData` with `released = 0`.
    ///
    /// The fallback path is zero-cost for new records; it only fires on
    /// legacy records. The upgraded value is **not** written back here —
    /// the next state-changing call will write the current schema, lazily
    /// migrating the record on first use.
    fn load_escrow(env: &Env, escrow_id: u64) -> Result<EscrowData, ForgeError> {
        let key = DataKey::Escrow(escrow_id);
        // Try current schema first.
        if let Some(data) = env.storage().persistent().get::<DataKey, EscrowData>(&key) {
            return Ok(data);
        }
        // Fallback: legacy schema (no `released` field).
        if let Some(v1) = env
            .storage()
            .persistent()
            .get::<DataKey, EscrowDataV1>(&key)
        {
            return Ok(EscrowData::from(v1));
        }
        Err(ForgeError::NotFound)
    }

    /// Load a basket record by id.
    ///
    /// Baskets have no legacy schema — `BasketEscrowData` is the only shape
    /// ever written under [`DataKey::Basket`] — so no migration fallback is
    /// needed. A single-token escrow id reads as `NotFound` here, exactly as
    /// a basket id reads as `NotFound` from [`Self::load_escrow`]: a record
    /// has one kind, and the caller picks the shape they mean.
    fn load_basket(env: &Env, escrow_id: u64) -> Result<BasketEscrowData, ForgeError> {
        let key = DataKey::Basket(escrow_id);
        env.storage()
            .persistent()
            .get::<DataKey, BasketEscrowData>(&key)
            .ok_or(ForgeError::NotFound)
    }

    /// Allocate the next monotonic escrow id. Instance storage: one small
    /// entry, written once per creation.
    fn next_id(env: &Env) -> Result<u64, ForgeError> {
        let count: u64 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);
        let id = count.checked_add(1).ok_or(ForgeError::ArithmeticOverflow)?;
        env.storage().instance().set(&DataKey::Count, &id);
        Ok(id)
    }

    /// Append `id` to the creation-order index of every distinct
    /// participant. Runs only on the create success path; each write bumps
    /// the entry TTL like any other persistent write.
    fn index_participants(env: &Env, id: u64, participants: &Vec<Address>) {
        for participant in participants.iter() {
            let key = DataKey::ParticipantIndex(participant.clone());
            let mut ids = env
                .storage()
                .persistent()
                .get(&key)
                .unwrap_or_else(|| Vec::new(env));
            ids.push_back(id);
            env.storage().persistent().set(&key, &ids);
            bump_entry(env, &key);
        }
    }

    /// Remove `id` from each distinct participant's index while preserving
    /// the relative creation order of every remaining id.
    fn remove_participant_indexes(env: &Env, id: u64, escrow: &EscrowData) {
        Self::remove_index(env, &escrow.buyer, id);
        if escrow.seller != escrow.buyer {
            Self::remove_index(env, &escrow.seller, id);
        }
        if escrow.arbiter != escrow.buyer && escrow.arbiter != escrow.seller {
            Self::remove_index(env, &escrow.arbiter, id);
        }
    }

    /// Remove one id from a participant's index without reordering survivors.
    fn remove_index(env: &Env, participant: &Address, id: u64) {
        let key = DataKey::ParticipantIndex(participant.clone());
        let Some(ids) = env.storage().persistent().get::<_, Vec<u64>>(&key) else {
            return;
        };
        let mut remaining = Vec::new(env);
        let mut removed = false;
        for indexed_id in ids.iter() {
            if indexed_id == id {
                removed = true;
            } else {
                remaining.push_back(indexed_id);
            }
        }
        if removed {
            env.storage().persistent().set(&key, &remaining);
            bump_entry(env, &key);
        }
    }
}

/// Pull every leg of `assets` from `from` into this contract's custody, in
/// list order.
///
/// The single primitive behind both `deposit` and `deposit_basket` (the
/// single-token path hands it a one-element list), so the two cannot drift on
/// ordering or error handling.
///
/// All-or-nothing by host frame rollback: if a leg fails this returns an error
/// and every leg already pulled in this invocation is reverted, so custody
/// never holds part of a basket. The buyer's `require_auth` on the calling
/// entrypoint covers each nested token authorization.
fn pull_assets(env: &Env, assets: &Vec<EscrowAsset>, from: &Address) -> Result<(), ForgeError> {
    for asset in assets.iter() {
        transfer_to_contract(env, &asset.token, from, asset.amount)?;
    }
    Ok(())
}

/// Pay every leg's `remaining` balance to `to`, in list order, and return the
/// post-payout asset list with each leg marked fully released.
///
/// The single primitive behind `release`, `refund`, and both `*_basket`
/// terminal paths, so there is exactly one payout loop and therefore exactly
/// one conservation rule. Transfer-before-state: callers write storage only
/// after this returns `Ok`, and a failure on any leg reverts the legs already
/// paid in this invocation, so a basket is never half-paid.
fn payout_assets(
    env: &Env,
    assets: &Vec<EscrowAsset>,
    to: &Address,
) -> Result<Vec<EscrowAsset>, ForgeError> {
    let mut settled = Vec::new(env);
    for asset in assets.iter() {
        transfer_from_contract(env, &asset.token, to, asset.remaining())?;
        settled.push_back(EscrowAsset {
            released: asset.amount,
            ..asset
        });
    }
    Ok(settled)
}

/// The union of the parties in `buyer`, `seller`, `arbiter`, deduplicated.
///
/// Both record kinds run every participant through this, so a single index
/// invariant holds across them: an address playing several roles on one escrow
/// appears exactly once, and iteration yields that escrow's id exactly once
/// per distinct participant.
fn distinct_participants(
    env: &Env,
    buyer: &Address,
    seller: &Address,
    arbiter: &Address,
) -> Vec<Address> {
    let mut participants = Vec::new(env);
    participants.push_back(buyer.clone());
    if seller != buyer {
        participants.push_back(seller.clone());
    }
    if arbiter != buyer && arbiter != seller {
        participants.push_back(arbiter.clone());
    }
    participants
}

/// The party authorized to refund, by phase.
///
/// Pre-deadline the **seller** authorizes (a voluntary refund of a failed
/// deal); at or after the deadline the **buyer** authorizes (reclaiming funds
/// the seller never released). Identical for both record kinds, so the refund
/// party rule exists once, not twice.
fn refund_authorizer(
    env: &Env,
    created_at: u64,
    timeout: u64,
    buyer: &Address,
    seller: &Address,
) -> Result<Address, ForgeError> {
    let deadline = created_at
        .checked_add(timeout)
        .ok_or(ForgeError::ArithmeticOverflow)?;
    if env.ledger().timestamp() >= deadline {
        Ok(buyer.clone())
    } else {
        Ok(seller.clone())
    }
}

/// Pay `amount` of the leg at `index` from custody to `to`.
///
/// The single primitive behind `release_partial` (which passes its single leg
/// at index 0) and `release_partial_basket`. The caller is responsible for
/// having validated the amount against that leg's `remaining`; this only
/// moves value, so the two partial paths differ solely in which leg they
/// select.
fn payout_part(
    env: &Env,
    assets: &Vec<EscrowAsset>,
    index: u32,
    to: &Address,
    amount: i128,
) -> Result<(), ForgeError> {
    let asset = assets.get(index).ok_or(ForgeError::InvalidInput)?;
    transfer_from_contract(env, &asset.token, to, amount)
}

/// Validate a declared basket: at least one leg, at most
/// [`MAX_BASKET_ASSETS`], every leg's `amount` positive, and no token named
/// twice.
///
/// Duplicates are **rejected** rather than summed. Merging would make the
/// result depend on the order the caller listed the legs in, and would
/// silently change the basket away from what the buyer authorized.
fn validate_basket(assets: &Vec<EscrowAsset>) -> Result<(), ForgeError> {
    if assets.is_empty() || assets.len() > MAX_BASKET_ASSETS {
        return Err(ForgeError::InvalidInput);
    }
    let mut at = 0;
    while at < assets.len() {
        let leg = assets.get(at).ok_or(ForgeError::InvalidInput)?;
        if leg.amount <= 0 {
            return Err(ForgeError::InvalidInput);
        }
        // Quadratic, but bounded by MAX_BASKET_ASSETS and only paid once per
        // basket creation.
        let mut earlier = 0;
        while earlier < at {
            let other = assets.get(earlier).ok_or(ForgeError::InvalidInput)?;
            if other.token == leg.token {
                return Err(ForgeError::InvalidInput);
            }
            earlier += 1;
        }
        at += 1;
    }
    Ok(())
}

/// Locate the leg holding `token`, or [`ForgeError::NotFound`] when the basket
/// does not hold it.
///
/// `NotFound` rather than `InvalidInput`: the basket exists, but the
/// `(escrow, token)` pair the caller asked about does not.
fn leg_index(assets: &Vec<EscrowAsset>, token: &Address) -> Result<u32, ForgeError> {
    let mut at = 0u32;
    while at < assets.len() {
        if assets.get_unchecked(at).token == *token {
            return Ok(at);
        }
        at += 1;
    }
    Err(ForgeError::NotFound)
}

/// The one-element asset list a single-token escrow is expressed as, so every
/// transfer primitive above is shared with the basket paths rather than
/// reimplemented for them. Carries the same `amount` / `released` accounting
/// the single-token record holds directly.
fn single_leg(env: &Env, escrow: &EscrowData) -> Vec<EscrowAsset> {
    let mut legs = Vec::new(env);
    legs.push_back(EscrowAsset {
        token: escrow.token.clone(),
        amount: escrow.amount,
        released: escrow.released,
    });
    legs
}

/// Move `amount` of `token` from `from` into this contract.
///
/// The buyer's `require_auth` on the calling entrypoint covers the nested
/// token authorization; no separate allowance is needed for a `transfer`
/// pull when the holder authorizes the invocation.
///
/// Token failures are bucketed into [`ForgeError::TokenTransferFailed`]
/// rather than forwarded: a client receiving `Error(Contract, #N)` cannot
/// know whether `N` came from the token or the escrow, and forwarding the
/// raw discriminant invites silent misinterpretation. The root cause
/// remains visible in the transaction's diagnostic events.
fn transfer_to_contract(
    env: &Env,
    token: &Address,
    from: &Address,
    amount: i128,
) -> Result<(), ForgeError> {
    match token::TokenClient::new(env, token).try_transfer(
        from,
        env.current_contract_address(),
        &amount,
    ) {
        Ok(Ok(())) => Ok(()),
        // Token returned a typed error (insufficient balance, missing
        // trustline, custom token logic) or the host aborted (most
        // commonly an undeployed token address). The raw discriminant is
        // intentionally discarded — see the bucketing note above.
        _ => Err(ForgeError::TokenTransferFailed),
    }
}

/// Move `amount` of `token` from this contract to `to`.
fn transfer_from_contract(
    env: &Env,
    token: &Address,
    to: &Address,
    amount: i128,
) -> Result<(), ForgeError> {
    match token::TokenClient::new(env, token).try_transfer(
        &env.current_contract_address(),
        to,
        &amount,
    ) {
        Ok(Ok(())) => Ok(()),
        _ => Err(ForgeError::TokenTransferFailed),
    }
}

/// Split a non-negative amount by basis points without overflowing an
/// intermediate `amount * seller_bps` multiplication.
fn split_amount(amount: i128, seller_bps: u32) -> Result<(i128, i128), ForgeError> {
    let denominator = 10_000_i128;
    let seller_bps = i128::from(seller_bps);
    let whole = amount
        .checked_div(denominator)
        .ok_or(ForgeError::ArithmeticOverflow)?;
    let remainder = amount
        .checked_rem(denominator)
        .ok_or(ForgeError::ArithmeticOverflow)?;
    let seller_share = whole
        .checked_mul(seller_bps)
        .and_then(|value| {
            remainder
                .checked_mul(seller_bps)
                .and_then(|fraction| value.checked_add(fraction / denominator))
        })
        .ok_or(ForgeError::ArithmeticOverflow)?;
    let buyer_share = amount
        .checked_sub(seller_share)
        .ok_or(ForgeError::ArithmeticOverflow)?;
    Ok((seller_share, buyer_share))
}

/// Bump a persistent entry's TTL to the workspace policy's 30-day horizon
/// when it falls inside its one-day threshold — see
/// `soroban_forge_shared_utils::ttl`.
///
/// Thin wrapper over [`soroban_forge_shared_utils::bump_entry`] — the
/// canonical helper (issue #127); the policy lives there.
/// Bump a persistent entry's TTL to the [`ttl::BUMP_AMOUNT`] horizon when
/// it falls inside [`ttl::BUMP_THRESHOLD`]. The standard threshold/extend
/// pattern: cheap no-op while the entry is fresh, decisive near expiry.
fn bump_entry(env: &Env, key: &DataKey) {
    shared_bump_entry(env, key);
}

/// Lifecycle events. The escrow id is a **topic** so indexers can filter
/// by escrow cheaply; the data payload carries the full record so no read
/// call is needed to reconstruct state.
///
/// ## Event decision for partial releases
///
/// `release_partial` emits `PartiallyReleased` rather than reusing the
/// existing `Released` event. The rationale:
///
/// * `Released` is treated as a terminal signal by existing indexers
///   (the `status` in its payload is always `Completed`). Emitting it for
///   non-terminal partial releases would silently break those consumers.
/// * `PartiallyReleased` carries `partial_amount` (the incremental payment)
///   alongside the full `EscrowData` (which exposes `released`, `remaining()`,
///   and `status`). Consumers can derive everything they need.
/// * The final partial release (where `amount == remaining`, causing
///   `status = Completed`) still emits `PartiallyReleased` (not `Released`),
///   keeping the event type consistent with the call site. Consumers that
///   care about completion should inspect `data.status`.
mod events {
    use super::*;

    #[contractevent]
    pub struct EscrowCreated {
        #[topic]
        pub escrow_id: u64,
        pub data: EscrowData,
    }

    #[contractevent]
    pub struct Deposited {
        #[topic]
        pub escrow_id: u64,
        pub data: EscrowData,
    }

    #[contractevent]
    pub struct Released {
        #[topic]
        pub escrow_id: u64,
        pub data: EscrowData,
    }

    /// Emitted by `release_partial` for every incremental seller payout.
    ///
    /// `partial_amount` is the amount transferred in this call; `data`
    /// carries the post-update `EscrowData` (including updated `released`
    /// and the new `status`). Check `data.status` to determine whether
    /// this partial release was the final one.
    #[contractevent]
    pub struct PartiallyReleased {
        #[topic]
        pub escrow_id: u64,
        /// The amount transferred to the seller in this particular call.
        pub partial_amount: i128,
        /// Full escrow record after this partial release, including updated
        /// `released` and `status` fields.
        pub data: EscrowData,
    }

    #[contractevent]
    pub struct Refunded {
        #[topic]
        pub escrow_id: u64,
        pub data: EscrowData,
    }

    #[contractevent]
    pub struct Disputed {
        #[topic]
        pub escrow_id: u64,
        pub data: EscrowData,
    }

    #[contractevent]
    pub struct Resolved {
        #[topic]
        pub escrow_id: u64,
        pub data: EscrowData,
        pub in_favor_of_seller: bool,
    }

    #[contractevent]
    pub struct ResolvedSplit {
        #[topic]
        pub escrow_id: u64,
        pub data: EscrowData,
        pub seller_bps: u32,
        pub seller_share: i128,
        pub buyer_share: i128,
    }

    #[contractevent]
    pub struct Cancelled {
        #[topic]
        pub escrow_id: u64,
        pub data: EscrowData,
    }

    // Basket events are additive variants that mirror the single-token ones:
    // the id is a topic, and `data` carries the full `BasketEscrowData` so an
    // indexer reconstructs per-asset state without a read call. No existing
    // single-token event payload changes shape.
    #[contractevent]
    pub struct BasketCreated {
        #[topic]
        pub escrow_id: u64,
        pub data: BasketEscrowData,
    }

    #[contractevent]
    pub struct BasketDeposited {
        #[topic]
        pub escrow_id: u64,
        pub data: BasketEscrowData,
    }

    #[contractevent]
    pub struct BasketReleased {
        #[topic]
        pub escrow_id: u64,
        pub data: BasketEscrowData,
    }

    /// Emitted by `release_partial_basket` for every incremental payout of one
    /// leg. `token` and `partial_amount` describe the transferred leg; `data`
    /// carries the post-update `BasketEscrowData` (per-leg `released` and the
    /// possible `Completed` status once **every** leg is exhausted).
    #[contractevent]
    pub struct BasketPartiallyReleased {
        #[topic]
        pub escrow_id: u64,
        /// The leg that was paid in this call.
        pub token: Address,
        /// The amount transferred to the seller in this particular call.
        pub partial_amount: i128,
        /// Full basket record after this partial release.
        pub data: BasketEscrowData,
    }

    #[contractevent]
    pub struct BasketRefunded {
        #[topic]
        pub escrow_id: u64,
        pub data: BasketEscrowData,
    }

    #[contractevent]
    pub struct BasketDisputed {
        #[topic]
        pub escrow_id: u64,
        pub data: BasketEscrowData,
    }

    #[contractevent]
    pub struct BasketResolved {
        #[topic]
        pub escrow_id: u64,
        pub data: BasketEscrowData,
        pub in_favor_of_seller: bool,
    }

    #[contractevent]
    pub struct BasketCancelled {
        #[topic]
        pub escrow_id: u64,
        pub data: BasketEscrowData,
    }

    // Publishers: thin functions so call sites read as intent, not
    // mechanics, and so a future payload change touches one module.
    pub fn escrow_created(env: &Env, escrow: &EscrowData) {
        EscrowCreated {
            escrow_id: escrow.escrow_id,
            data: escrow.clone(),
        }
        .publish(env);
    }

    pub fn deposited(env: &Env, escrow: &EscrowData) {
        Deposited {
            escrow_id: escrow.escrow_id,
            data: escrow.clone(),
        }
        .publish(env);
    }

    pub fn released(env: &Env, escrow: &EscrowData) {
        Released {
            escrow_id: escrow.escrow_id,
            data: escrow.clone(),
        }
        .publish(env);
    }

    pub fn partially_released(env: &Env, escrow: &EscrowData, partial_amount: i128) {
        PartiallyReleased {
            escrow_id: escrow.escrow_id,
            partial_amount,
            data: escrow.clone(),
        }
        .publish(env);
    }

    pub fn refunded(env: &Env, escrow: &EscrowData) {
        Refunded {
            escrow_id: escrow.escrow_id,
            data: escrow.clone(),
        }
        .publish(env);
    }

    pub fn disputed(env: &Env, escrow: &EscrowData) {
        Disputed {
            escrow_id: escrow.escrow_id,
            data: escrow.clone(),
        }
        .publish(env);
    }

    pub fn resolved(env: &Env, escrow: &EscrowData, in_favor_of_seller: bool) {
        Resolved {
            escrow_id: escrow.escrow_id,
            data: escrow.clone(),
            in_favor_of_seller,
        }
        .publish(env);
    }

    pub fn resolved_split(
        env: &Env,
        escrow: &EscrowData,
        seller_bps: u32,
        seller_share: i128,
        buyer_share: i128,
    ) {
        ResolvedSplit {
            escrow_id: escrow.escrow_id,
            data: escrow.clone(),
            seller_bps,
            seller_share,
            buyer_share,
        }
        .publish(env);
    }

    pub fn cancelled(env: &Env, escrow: &EscrowData) {
        Cancelled {
            escrow_id: escrow.escrow_id,
            data: escrow.clone(),
        }
        .publish(env);
    }

    pub fn basket_created(env: &Env, basket: &BasketEscrowData) {
        BasketCreated {
            escrow_id: basket.escrow_id,
            data: basket.clone(),
        }
        .publish(env);
    }

    pub fn basket_deposited(env: &Env, basket: &BasketEscrowData) {
        BasketDeposited {
            escrow_id: basket.escrow_id,
            data: basket.clone(),
        }
        .publish(env);
    }

    pub fn basket_released(env: &Env, basket: &BasketEscrowData) {
        BasketReleased {
            escrow_id: basket.escrow_id,
            data: basket.clone(),
        }
        .publish(env);
    }

    pub fn basket_partially_released(
        env: &Env,
        basket: &BasketEscrowData,
        token: &Address,
        amount: i128,
    ) {
        BasketPartiallyReleased {
            escrow_id: basket.escrow_id,
            token: token.clone(),
            partial_amount: amount,
            data: basket.clone(),
        }
        .publish(env);
    }

    pub fn basket_refunded(env: &Env, basket: &BasketEscrowData) {
        BasketRefunded {
            escrow_id: basket.escrow_id,
            data: basket.clone(),
        }
        .publish(env);
    }

    pub fn basket_disputed(env: &Env, basket: &BasketEscrowData) {
        BasketDisputed {
            escrow_id: basket.escrow_id,
            data: basket.clone(),
        }
        .publish(env);
    }

    pub fn basket_resolved(env: &Env, basket: &BasketEscrowData, in_favor_of_seller: bool) {
        BasketResolved {
            escrow_id: basket.escrow_id,
            data: basket.clone(),
            in_favor_of_seller,
        }
        .publish(env);
    }

    pub fn basket_cancelled(env: &Env, basket: &BasketEscrowData) {
        BasketCancelled {
            escrow_id: basket.escrow_id,
            data: basket.clone(),
        }
        .publish(env);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod authz;

#[cfg(test)]
mod props;

// Generates and validates `indexer/fixtures/escrow-events.json`, the ground
// truth consumed by the reference event indexer in `packages/typescript-sdk`
// (see `indexer/docs/event-schema.md`).
#[cfg(test)]
mod indexer_fixtures;
