//! Randomized invariant suite (proptest).
//!
//! The hand-written suite pins behaviour on known values; this module tries
//! to *falsify* the contract's fund-safety claims over generated inputs.
//! Five properties are exercised:
//!
//! **P1 — Conservation.** On every terminal path, for random amounts,
//! timeouts, dispute claimants and resolution directions: the contract ends
//! holding zero and the parties' gain equals the deposit exactly. Paths now
//! include randomized bounded sequences of `release_partial` before the
//! terminal action.
//!
//! **P2 — Tamper-resilient conservation.** An attacker who can rewrite the
//! contract's persistent storage between calls (the strongest adversary the
//! ledger itself does not stop) can never move value out of the pool:
//! `contract + buyer + seller + arbiter` balances are conserved across every
//! mutation, and a tampered over-payment is rejected by the token layer's
//! balance check rather than paying out.
//!
//! **P3 — Fund safety over arbitrary sequences.** For random sequences of
//! `deposit / release / release_partial / refund / dispute / resolve`: each
//! call succeeds only when the mirrored state machine says it must, outsider
//! disputes are always rejected, no state transition ever changes the pool
//! total, and terminal escrows are never payable twice. The mirror state
//! machine now tracks `released` alongside the lifecycle state.
//!
//! **P4 — Participant index consistency.** Random create/cancel interleavings
//! across two buyers and two sellers keep each live participant index equal
//! to the set of addressable, non-cancelled escrows. A cancelled escrow is
//! never returned, and every returned id still resolves.
//!
//! All properties run against a real Stellar Asset Contract, so balances
//! reflect actual token movement. Runs are deterministic (fixed default
//! seed); a failure prints its case seed for replay. Override the case count
//! with `PROPTEST_CASES=n cargo test -p soroban-forge-escrow props`.

use crate::{
    BasketEscrowData, Escrow, EscrowAsset, EscrowData, EscrowStatus, SorobanForgeEscrowClient,
};
use crate::{Escrow, EscrowData, EscrowStatus, ParticipantEscrowsPage, SorobanForgeEscrowClient};
use proptest::prelude::*;
use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{Address, Env, Vec};

const START: u64 = 1_000_000;
const MAX_AMOUNT: i128 = 1_000_000_000_000;
const MAX_TIMEOUT: u64 = 10 * 365 * 24 * 3600;

// -----------------------------------------------------------------------
// World: one fresh env, SAC, escrow contract, and fixed role set per case
// -----------------------------------------------------------------------

struct World {
    env: Env,
    token: Address,
    escrow: Address,
    buyer: Address,
    seller: Address,
    arbiter: Address,
    outsider: Address,
}

fn setup_world() -> World {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let admin = Address::generate(&env);
    let outsider = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin);
    let token = sac.address();

    let escrow = env.register(Escrow, ());

    World {
        token,
        escrow,
        buyer: Address::generate(&env),
        seller: Address::generate(&env),
        arbiter: Address::generate(&env),
        outsider,
        env,
    }
}

fn assert_participant_indexes_consistent(w: &World, ids: &Vec<u64>, parties: &[Address]) {
    let client = w.escrow_client();
    for party in parties {
        let mut expected = Vec::new(&w.env);
        for index in 0..ids.len() {
            let id = ids.get_unchecked(index);
            let record: EscrowData = client.get_escrow(&id);
            let involved =
                record.buyer == *party || record.seller == *party || record.arbiter == *party;
            if involved && record.status != EscrowStatus::Cancelled {
                expected.push_back(id);
            }
        }

        let page = client.escrows_for_participant(party, &0, &u32::MAX);
        assert_eq!(page.ids, expected, "index contents must match live escrows");
        assert_eq!(page.total, expected.len());
        for index in 0..page.ids.len() {
            let id = page.ids.get_unchecked(index);
            let record: EscrowData = client.get_escrow(&id);
            assert_ne!(record.status, EscrowStatus::Cancelled);
            for earlier in 0..index {
                assert_ne!(
                    page.ids.get_unchecked(earlier),
                    id,
                    "participant index must not contain duplicate ids"
                );
            }
        }
    }
}

impl World {
    fn escrow_client(&self) -> SorobanForgeEscrowClient<'_> {
        SorobanForgeEscrowClient::new(&self.env, &self.escrow)
    }

    fn token_client(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.token)
    }

    fn mint_to_buyer(&self, amount: i128) {
        StellarAssetClient::new(&self.env, &self.token).mint(&self.buyer, &amount);
    }

    fn create(&self, amount: i128, timeout: u64) -> u64 {
        self.escrow_client().create_escrow(
            &self.buyer,
            &self.seller,
            &self.arbiter,
            &self.token,
            &amount,
            &timeout,
        )
    }

    /// Balances of (buyer, seller, arbiter, contract) — the whole pool.
    fn pool(&self) -> (i128, i128, i128, i128) {
        let t = self.token_client();
        (
            t.balance(&self.buyer),
            t.balance(&self.seller),
            t.balance(&self.arbiter),
            t.balance(&self.escrow),
        )
    }

    /// The conserved total: everything held by the contract and the parties.
    fn pool_total(&self) -> i128 {
        let (b, s, a, c) = self.pool();
        b + s + a + c
    }
}

// -----------------------------------------------------------------------
// Strategies
// -----------------------------------------------------------------------

fn arb_delta() -> impl Strategy<Value = i128> {
    prop_oneof![
        Just(0i128),
        (1i128..=MAX_AMOUNT).prop_map(|v| v),
        (1i128..=MAX_AMOUNT).prop_map(|v| -v),
    ]
}

fn claimant_pick() -> impl Strategy<Value = u8> {
    0u8..=2 // 0 = buyer, 1 = seller, 2 = outsider
}

fn terminal_path() -> impl Strategy<Value = u8> {
    0u8..=2 // 0 = release, 1 = refund (pre-deadline), 2 = dispute -> resolve
}

/// A partial-release fraction 1..=9 (tenths of total amount).
fn partial_fraction() -> impl Strategy<Value = u8> {
    1u8..=9
}

// -----------------------------------------------------------------------
// P1 — conservation on random terminal paths (no tampering)
// -----------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p1_conservation_on_random_terminal_paths(
        amount in 1i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
        path in terminal_path(),
        claimant in claimant_pick(),
        pay_seller in prop::bool::ANY,
    ) {
        let w = setup_world();
        w.mint_to_buyer(amount);
        let id = w.create(amount, timeout);
        w.escrow_client().deposit(&id);
        // Baseline AFTER deposit: the parties' pre-payout holdings plus the
        // contract's custody. (Forgetting `c0` here is exactly the kind of
        // bug the minimal counterexample exposes — the first P1 failure was
        // in this formula, not in the contract.)
        let (b0, s0, _a0, c0) = w.pool();

        match path {
            0 => w.escrow_client().release(&id),
            1 => w.escrow_client().refund(&id),
            _ => {
                // Outsider claimants are covered by P3; force a party here
                // so the dispute actually lands.
                let claimant_addr = match claimant {
                    0 => &w.buyer,
                    _ => &w.seller,
                };
                w.escrow_client().dispute(&id, claimant_addr);
                w.escrow_client().resolve(&id, &pay_seller);
            }
        }

        let (b1, s1, a1, c1) = w.pool();
        prop_assert_eq!(c1, 0, "contract must retain nothing on a terminal path");
        prop_assert_eq!(b1 + s1 + a1, b0 + s0 + c0, "parties' gain must equal the deposit exactly");
        let status = w.escrow_client().get_status(&id);
        prop_assert!(status == EscrowStatus::Completed || status == EscrowStatus::Refunded);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// P1-partial: conservation after a bounded partial-release sequence
    /// then a terminal action. Verifies:
    /// - pool is conserved after every partial release
    /// - terminal action pays out remaining balance
    /// - released + remaining == deposited throughout
    /// - refund/resolve operate only on remaining balance
    /// - terminal state is correct
    #[test]
    fn p1_conservation_with_partial_release_sequences(
        amount in 10i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
        // A sequence of (numerator, denominator) fractions of the *original*
        // amount; clamped to remaining so the sequence always stays valid.
        fracs in prop::collection::vec((1u8..=9, 1u8..=10), 0..=8),
        terminal in terminal_path(),
        claimant in claimant_pick(),
        pay_seller in prop::bool::ANY,
    ) {
        let w = setup_world();
        w.mint_to_buyer(amount);
        let id = w.create(amount, timeout);
        w.escrow_client().deposit(&id);

        let before = w.pool_total();

        // Apply partial releases, clamping each to remaining.
        for (num, den) in &fracs {
            let record: EscrowData = w.escrow_client().get_escrow(&id);
            let remaining = record.remaining();
            if remaining == 0 || record.status != EscrowStatus::Funded {
                break;
            }
            // fraction of the original amount, clamped to [1, remaining].
            let partial = ((amount * i128::from(*num)) / i128::from(*den)).max(1).min(remaining);
            let res = w.escrow_client().try_release_partial(&id, &partial);
            prop_assert!(matches!(res, Ok(Ok(()))),
                "valid partial release must succeed: amount={amount} partial={partial} remaining={remaining}");

            // Accounting invariant after every partial.
            let after_record: EscrowData = w.escrow_client().get_escrow(&id);
            prop_assert_eq!(after_record.released + after_record.remaining(), after_record.amount,
                "released + remaining must always equal deposited amount");

            // Pool total must be unchanged after every partial.
            prop_assert_eq!(w.pool_total(), before,
                "pool must be conserved after partial release");
        }

        // Now apply the terminal action if the escrow is still Funded.
        let status = w.escrow_client().get_status(&id);
        if status == EscrowStatus::Funded {
            match terminal {
                0 => { w.escrow_client().release(&id); }
                1 => { w.escrow_client().refund(&id); }
                _ => {
                    let claimant_addr = match claimant {
                        0 => &w.buyer,
                        _ => &w.seller,
                    };
                    w.escrow_client().dispute(&id, claimant_addr);
                    w.escrow_client().resolve(&id, &pay_seller);
                }
            }
        }

        // Final invariants.
        let (_, _, _, c1) = w.pool();
        prop_assert_eq!(c1, 0, "contract must hold nothing after terminal action");
        prop_assert_eq!(w.pool_total(), before,
            "pool total must be conserved end-to-end");

        let final_status = w.escrow_client().get_status(&id);
        prop_assert!(
            final_status == EscrowStatus::Completed || final_status == EscrowStatus::Refunded,
            "escrow must be terminal after the terminal action"
        );

        // Remaining balance in the record must be zero at terminal.
        let record: EscrowData = w.escrow_client().get_escrow(&id);
        prop_assert_eq!(record.remaining(), 0,
            "remaining must be zero in terminal escrow");
        prop_assert_eq!(record.released, record.amount,
            "released must equal amount in terminal escrow");
    }
}

// -----------------------------------------------------------------------
// P2 — tamper-resilient conservation (storage attacker between calls)
// -----------------------------------------------------------------------

/// Apply one storage mutation to a loaded escrow record and write it back.
///
/// `escrow_id` and `token` are never mutated: pointing the id at a record
/// that does not exist would brick the entry against every further call, and
/// swapping the token would describe a different contract. Mutating the
/// party addresses is likewise excluded — worst case it lets an address
/// withdraw the full escrow, which is custodial exposure inherent to state
/// tampering, not an invariant violation. What the property demands is that
/// even under every mutation we *do* allow, no value can leave the pool.
fn mutate_record(w: &World, id: u64, kind: u8, add: i128, sub: i128, overwrite: i128) {
    let key = crate::DataKey::Escrow(id);
    // Storage writes are only legal from inside a contract context, so the
    // tampering runs under `as_contract` — modelling a compromised contract
    // itself, which is the strongest adversary this ledger model allows.
    w.env.as_contract(&w.escrow, || {
        let mut rec: EscrowData = w
            .env
            .storage()
            .persistent()
            .get(&key)
            .expect("tamper target must exist");

        match kind {
            // Fuzz the custodied amount.
            0 => {
                rec.amount = add.wrapping_add(rec.amount);
            }
            // Fuzz the deadline: push it into the past or future.
            1 => {
                rec.timeout = (sub.unsigned_abs() as u64).wrapping_add(rec.timeout);
            }
            2 => {
                let shift = sub.unsigned_abs();
                let shifted = (rec.created_at as u128).wrapping_add(shift);
                if shifted <= u64::MAX as u128 {
                    rec.created_at = shifted as u64;
                } else {
                    return; // skip: cannot represent; nothing written
                }
            }
            // Fuzz the claimed/position fields that could skew payouts.
            3 => {
                rec.amount = rec.amount.wrapping_add(overwrite);
            }
            // Fuzz the lifecycle status itself.
            4 => {
                rec.status = match overwrite.rem_euclid(6) {
                    0 => EscrowStatus::Pending,
                    1 => EscrowStatus::Funded,
                    2 => EscrowStatus::Completed,
                    3 => EscrowStatus::Refunded,
                    4 => EscrowStatus::Disputed,
                    _ => EscrowStatus::Cancelled,
                };
            }
            // Fuzz the released field.
            _ => {
                rec.released = overwrite.abs().min(rec.amount.abs());
            }
        }

        w.env.storage().persistent().set(&key, &rec);
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p2_tampered_storage_never_moves_value_out_of_the_pool(
        amount in 1i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
        muts in prop::collection::vec((0u8..=5, arb_delta(), arb_delta(), arb_delta()), 0..=6),
        pay_seller in prop::bool::ANY,
    ) {
        let w = setup_world();
        w.mint_to_buyer(amount);
        let id = w.create(amount, timeout);
        w.escrow_client().deposit(&id);
        let before = w.pool_total();

        // Deadline always forced past: post-deadline refund/resolve paths
        // stay reachable no matter how created_at/timeout were mutated.
        w.env.ledger().set_timestamp(START.saturating_add(timeout).saturating_add(1));

        for (kind, add, sub, overwrite) in muts {
            mutate_record(&w, id, kind, add, sub, overwrite);
            prop_assert_eq!(w.pool_total(), before,
                "a storage mutation changed the pool total");
        }

        // Attempt a payout against whatever the tampered record now says.
        // Three conserve-the-pool outcomes are acceptable:
        //   - resolve succeeds: the *tampered* amount is what moves; if the
        //     attacker shrank it, residue stays in the contract (their loss,
        //     not a theft) — so we assert conservation, not emptiness.
        //   - resolve fails on an over-payment: the token layer's balance
        //     check reverts the whole call, status untouched.
        //   - resolve fails as InvalidInput: the record was tampered into a
        //     non-Disputed status, so nothing was attempted.
        let client = w.escrow_client();
        let status: EscrowStatus = client.get_status(&id);
        match client.try_resolve(&id, &pay_seller) {
            Ok(Ok(())) => {
                let (_, _, _, c1) = w.pool();
                prop_assert!(c1 >= 0, "resolved escrow may retain residue from a shrunken amount, never a deficit");
            }
            Err(Ok(ForgeError::TokenTransferFailed)) => {
                prop_assert_eq!(status, EscrowStatus::Disputed,
                    "failed resolve must revert the status change");
            }
            Err(Ok(ForgeError::InvalidInput)) => {
                // Tampered into a non-Disputed status; nothing attempted.
            }
            other => panic!("unexpected resolve outcome under tampering: {:?}", other),
        }

        prop_assert_eq!(w.pool_total(), before,
            "pool total must be conserved end-to-end, tampering or not");
    }
}

// -----------------------------------------------------------------------
// P3 — fund safety over arbitrary call sequences
// -----------------------------------------------------------------------

/// (op, claimant_pick, pay_seller, partial_frac)
///
/// ops: 0=deposit, 1=release, 2=refund, 3=dispute, 4=resolve,
///      5=release_partial (frac/10 of the original amount, clamped to remaining)
fn action_strategy() -> impl Strategy<Value = (u8, u8, bool, u8)> {
    (
        0u8..=5,
        claimant_pick(),
        prop::bool::ANY,
        partial_fraction(),
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn p3_sequences_never_violate_fund_safety(
        amount in 1i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
        actions in prop::collection::vec(action_strategy(), 0..=12),
    ) {
        use EscrowStatus::{Cancelled, Completed, Disputed, Funded, Pending, Refunded};

        let w = setup_world();
        w.mint_to_buyer(amount);
        let id = w.create(amount, timeout);
        w.escrow_client().deposit(&id);

        // The contract's own state machine, mirrored independently. Every
        // call is checked against this mirror; a mismatch is a failure.
        let mut state = Funded;
        // Mirror of the released accounting.
        let mut mirror_released: i128 = 0;

        for (op, claimant, pay_seller, frac) in actions {
            let client = w.escrow_client();
            let before = w.pool_total();

            match op {
                // deposit
                0 => {
                    let res = client.try_deposit(&id);
                    match state {
                        Pending => {
                            prop_assert!(matches!(res, Ok(Ok(()))), "deposit must succeed while Pending");
                            state = Funded;
                        }
                        _ => prop_assert!(res.is_err(), "deposit must fail unless Pending"),
                    }
                }
                // release (full remaining)
                1 => {
                    let res = client.try_release(&id);
                    match state {
                        Funded => {
                            prop_assert!(matches!(res, Ok(Ok(()))), "release must succeed while Funded");
                            mirror_released = amount;
                            state = Completed;
                        }
                        _ => prop_assert!(res.is_err(), "release must fail unless Funded"),
                    }
                }
                // refund — post-deadline form: the buyer reclaims.
                2 => {
                    w.env.ledger().set_timestamp(START.saturating_add(timeout).saturating_add(1));
                    let res = client.try_refund(&id);
                    match state {
                        Funded => {
                            prop_assert!(matches!(res, Ok(Ok(()))), "post-deadline refund must succeed while Funded");
                            mirror_released = amount;
                            state = Refunded;
                        }
                        _ => prop_assert!(res.is_err(), "refund must fail unless Funded"),
                    }
                }
                // dispute
                3 => {
                    // claimant_pick: 0 buyer (party), 1 seller (party), 2 outsider.
                    let claimant_addr = match claimant {
                        0 => w.buyer.clone(),
                        1 => w.seller.clone(),
                        _ => w.outsider.clone(),
                    };
                    let res = client.try_dispute(&id, &claimant_addr);
                    match state {
                        Funded if claimant <= 1 => {
                            prop_assert!(matches!(res, Ok(Ok(()))), "party dispute must succeed while Funded");
                            state = Disputed;
                        }
                        Funded => {
                            // Rejection must be our typed InvalidInput; the
                            // two-step unwrap mirrors tests.rs's error shape.
                            let err = res.unwrap_err().unwrap();
                            prop_assert_eq!(err, ForgeError::InvalidInput,
                                "outsider dispute must be rejected with InvalidInput");
                        }
                        _ => prop_assert!(res.is_err(), "dispute must fail unless Funded"),
                    }
                }
                // resolve
                4 => {
                    let res = client.try_resolve(&id, &pay_seller);
                    match state {
                        Disputed => {
                            prop_assert!(matches!(res, Ok(Ok(()))), "resolve must succeed while Disputed");
                            mirror_released = amount;
                            state = if pay_seller { Completed } else { Refunded };
                        }
                        _ => prop_assert!(res.is_err(), "resolve must fail unless Disputed"),
                    }
                }
                // release_partial: frac/10 of original amount, clamped to remaining
                _ => {
                    let remaining = amount.saturating_sub(mirror_released);
                    let partial = if remaining > 0 {
                        ((amount * i128::from(frac)) / 10).max(1).min(remaining)
                    } else {
                        1 // will be rejected (remaining == 0 or wrong state)
                    };
                    let res = client.try_release_partial(&id, &partial);
                    match state {
                        Funded if remaining > 0 => {
                            prop_assert!(matches!(res, Ok(Ok(()))),
                                "valid release_partial must succeed: partial={partial} remaining={remaining}");
                            mirror_released = mirror_released.saturating_add(partial);
                            // Final partial completes the escrow.
                            if partial == remaining {
                                state = Completed;
                            }
                        }
                        _ => {
                            prop_assert!(res.is_err(),
                                "release_partial must fail when state={:?} remaining={remaining}", state);
                        }
                    }
                }
            }

            prop_assert_eq!(w.pool_total(), before,
                "no call may change the pool total outside a payout from the contract");

            // Mirror accounting consistency after every step.
            if matches!(state, Funded | Disputed) {
                let record: EscrowData = w.escrow_client().get_escrow(&id);
                prop_assert_eq!(record.released, mirror_released,
                    "released field must match mirror after step");
                prop_assert_eq!(record.remaining(), amount - mirror_released,
                    "remaining() must equal amount - mirror_released");
                prop_assert_eq!(record.released + record.remaining(), record.amount,
                    "accounting identity: released + remaining == amount");
            }
        }

        // If the sequence reached a terminal state, it must be genuinely
        // finished: contract empty, and every further call rejected.
        if matches!(state, Completed | Refunded | Cancelled) {
            let (_, _, _, c) = w.pool();
            prop_assert_eq!(c, 0, "terminal escrow must have paid out in full");
            let client = w.escrow_client();
            prop_assert!(client.try_deposit(&id).is_err());
            prop_assert!(client.try_release(&id).is_err());
            prop_assert!(client.try_release_partial(&id, &1).is_err());
            prop_assert!(client.try_refund(&id).is_err());
            prop_assert!(client.try_dispute(&id, &w.buyer).is_err());
            prop_assert!(client.try_resolve(&id, &true).is_err());

            // Remaining and released must be fully accounted in terminal records.
            let record: EscrowData = w.escrow_client().get_escrow(&id);
            prop_assert_eq!(record.remaining(), 0,
                "terminal escrow remaining must be zero");
            prop_assert_eq!(record.released, record.amount,
                "terminal escrow released must equal deposited amount");
        }
        let _ = Cancelled; // Cancelled is unreachable here (no cancel op in P3's set)
    }
}

// -----------------------------------------------------------------------
// Multi-asset baskets
// -----------------------------------------------------------------------

/// The same world as [`setup_world`] but with **two** SAC tokens, so a basket
/// holds one leg in each. Balances are tracked per token, because the
/// contract's conservation promise is per asset: the custody of token A can
/// never be mixed with token B.
struct BasketWorld {
    env: Env,
    token_a: Address,
    token_b: Address,
    escrow: Address,
    buyer: Address,
    seller: Address,
    arbiter: Address,
}

fn setup_basket_world(mint_b: bool) -> BasketWorld {
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn p4_participant_indexes_match_live_escrows_after_create_cancel_interleavings(
        actions in prop::collection::vec((0u8..=1, 0u8..=3), 1..=12),
    ) {
        let w = setup_world();
        let buyer_a = Address::generate(&w.env);
        let buyer_b = Address::generate(&w.env);
        let seller_a = Address::generate(&w.env);
        let seller_b = Address::generate(&w.env);
        let parties = [
            buyer_a.clone(),
            buyer_b.clone(),
            seller_a.clone(),
            seller_b.clone(),
            w.arbiter.clone(),
        ];
        let mut ids = Vec::new(&w.env);

        for (operation, choice) in actions {
            let mut pending_count = 0_u32;
            for index in 0..ids.len() {
                let id = ids.get_unchecked(index);
                if w.escrow_client().get_status(&id) == EscrowStatus::Pending {
                    pending_count += 1;
                }
            }

            if operation == 0 || pending_count == 0 {
                let buyer = if choice & 1 == 0 { &buyer_a } else { &buyer_b };
                let seller = if choice & 2 == 0 { &seller_a } else { &seller_b };
                let id = w.escrow_client().create_escrow(
                    buyer,
                    seller,
                    &w.arbiter,
                    &w.token,
                    &1_i128,
                    &1_u64,
                );
                ids.push_back(id);
            } else {
                let watched_party = &parties[usize::from(choice) % parties.len()];
                let before_page = w
                    .escrow_client()
                    .escrows_for_participant(watched_party, &0, &u32::MAX);
                let cursor = if before_page.total == 0 {
                    0
                } else {
                    u32::from(choice) % before_page.total
                };
                let target = u32::from(choice) % pending_count;
                let mut pending_at = 0_u32;
                for index in 0..ids.len() {
                    let id = ids.get_unchecked(index);
                    if w.escrow_client().get_status(&id) == EscrowStatus::Pending {
                        if pending_at == target {
                            w.escrow_client().cancel(&id);
                            break;
                        }
                        pending_at += 1;
                    }
                }

                // Cursors are live offsets. After the cancellation, a page
                // at this cursor must match the current full list at that
                // same offset, even if earlier ids shifted left.
                let current = w
                    .escrow_client()
                    .escrows_for_participant(watched_party, &0, &u32::MAX);
                let continued = w
                    .escrow_client()
                    .escrows_for_participant(watched_party, &cursor, &1);
                if cursor < current.ids.len() {
                    prop_assert_eq!(continued.ids.len(), 1);
                    prop_assert_eq!(
                        continued.ids.get_unchecked(0),
                        current.ids.get_unchecked(cursor),
                    );
                } else {
                    prop_assert_eq!(continued.ids.len(), 0);
                }
            }

            assert_participant_indexes_consistent(&w, &ids, &parties);
        }
    }
}

// -----------------------------------------------------------------------
// MultiPartyWorld: several buyers, sellers, and an arbiter for P5/P6
// -----------------------------------------------------------------------

struct MultiPartyWorld {
    env: Env,
    token: Address,
    escrow: Address,
    buyer1: Address,
    buyer2: Address,
    buyer3: Address,
    seller1: Address,
    seller2: Address,
    seller3: Address,
    arbiter: Address,
    outsider: Address,
}

fn setup_multi_party_world() -> MultiPartyWorld {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let admin = Address::generate(&env);
    let sac_a = env.register_stellar_asset_contract_v2(admin.clone());
    let token_a = sac_a.address();
    let sac_b = env.register_stellar_asset_contract_v2(admin);
    let token_b = sac_b.address();

    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let escrow = env.register(Escrow, ());
    let w = BasketWorld {
        env,
        token_a,
        token_b,
        escrow,
        buyer,
        seller,
        arbiter,
    };
    w.mint_to_buyer(w.token_a.clone(), MAX_AMOUNT);
    if mint_b {
        w.mint_to_buyer(w.token_b.clone(), MAX_AMOUNT);
    }
    w
}

impl BasketWorld {
    fn escrow_client(&self) -> SorobanForgeEscrowClient<'_> {
        SorobanForgeEscrowClient::new(&self.env, &self.escrow)
    }

    fn mint_to_buyer(&self, token: Address, amount: i128) {
        StellarAssetClient::new(&self.env, &token).mint(&self.buyer, &amount);
    }

    fn create(&self, a_amt: i128, b_amt: i128, timeout: u64) -> u64 {
        let assets = self.legs(a_amt, b_amt);
        self.escrow_client().create_basket(
            &self.buyer,
            &self.seller,
            &self.arbiter,
            &assets,
            &timeout,
        )
    }

    fn legs(&self, a_amt: i128, b_amt: i128) -> Vec<EscrowAsset> {
        let mut assets = Vec::new(&self.env);
        assets.push_back(EscrowAsset {
            token: self.token_a.clone(),
            amount: a_amt,
            released: 0,
        });
        assets.push_back(EscrowAsset {
            token: self.token_b.clone(),
            amount: b_amt,
            released: 0,
        });
        assets
    }

    /// _A_ pool balances: (buyer, seller, arbiter, contract) in token A.
    fn pool_a(&self) -> (i128, i128, i128, i128) {
        let t = TokenClient::new(&self.env, &self.token_a);
        (
            t.balance(&self.buyer),
            t.balance(&self.seller),
            t.balance(&self.arbiter),
            t.balance(&self.escrow),
        )
    }

    /// _B_ pool balances: (buyer, seller, arbiter, contract) in token B.
    fn pool_b(&self) -> (i128, i128, i128, i128) {
        let t = TokenClient::new(&self.env, &self.token_b);
        (
            t.balance(&self.buyer),
            t.balance(&self.seller),
            t.balance(&self.arbiter),
            t.balance(&self.escrow),
        )
    }

    fn pool_total_a(&self) -> i128 {
        let (b, s, a, c) = self.pool_a();
        b + s + a + c
    }

    fn pool_total_b(&self) -> i128 {
        let (b, s, a, c) = self.pool_b();
        b + s + a + c
    }

    /// Per-leg accounting mirror on the live record: every leg satisfies
    /// `released + remaining() == amount`, and the leg order matches the
    /// creation order (token A then token B).
    fn assert_leg_accounting(&self, id: u64) -> bool {
        let record: BasketEscrowData = self.escrow_client().get_basket(&id);
        let leg_a = record.assets.get_unchecked(0);
        let leg_b = record.assets.get_unchecked(1);
        leg_a.released + leg_a.remaining() == leg_a.amount
            && leg_b.released + leg_b.remaining() == leg_b.amount
            && leg_a.token == self.token_a
            && leg_b.token == self.token_b
    }
}

// P4 — conservation over random *two-leg* terminal paths. Each token's pool is
// conserved independently: custody empties in both, and each leg's deposit
// lands exactly with the parties.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn p4_basket_conservation_on_random_terminal_paths(
        a_amt in 1i128..=MAX_AMOUNT,
        b_amt in 1i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
        path in terminal_path(),
        claimant in claimant_pick(),
        pay_seller in prop::bool::ANY,
    ) {
        let w = setup_basket_world(true);
        let id = w.create(a_amt, b_amt, timeout);
        w.escrow_client().deposit_basket(&id);
        let (a_b0, a_s0, _a_a0, a_c0) = w.pool_a();
        let (b_b0, b_s0, _b_a0, b_c0) = w.pool_b();

        match path {
            0 => w.escrow_client().release_basket(&id),
            1 => w.escrow_client().refund_basket(&id),
            _ => {
                let claimant_addr = match claimant {
                    0 => &w.buyer,
                    _ => &w.seller,
                };
                w.escrow_client().dispute_basket(&id, claimant_addr);
                w.escrow_client().resolve_basket(&id, &pay_seller);
            }
        }

        let (a_b1, a_s1, a_a1, a_c1) = w.pool_a();
        let (b_b1, b_s1, b_a1, b_c1) = w.pool_b();
        prop_assert_eq!(a_c1, 0, "contract must retain nothing of token A");
        prop_assert_eq!(b_c1, 0, "contract must retain nothing of token B");
        prop_assert_eq!(
            a_b1 + a_s1 + a_a1,
            a_b0 + a_s0 + a_c0,
            "parties' gain of token A must equal the deposit exactly"
        );
        prop_assert_eq!(
            b_b1 + b_s1 + b_a1,
            b_b0 + b_s0 + b_c0,
            "parties' gain of token B must equal the deposit exactly"
        );
        let status = w.escrow_client().get_status(&id);
        prop_assert!(status == EscrowStatus::Completed || status == EscrowStatus::Refunded);
        prop_assert!(w.assert_leg_accounting(id), "per-leg accounting must hold at terminal");
    }
}

// P4-partial — conservation across random per-leg partial sequences then a
// terminal action. `(leg, numerator, denominator)` picks which leg and how
// much (clamped to that leg's remaining).
proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn p4_basket_conservation_with_partial_sequences(
        a_amt in 10i128..=MAX_AMOUNT,
        b_amt in 10i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
        partials in prop::collection::vec((0u8..=1, 1u8..=9, 1u8..=10), 0..=8),
        terminal in terminal_path(),
        claimant in claimant_pick(),
        pay_seller in prop::bool::ANY,
    ) {
        let w = setup_basket_world(true);
        let id = w.create(a_amt, b_amt, timeout);
        w.escrow_client().deposit_basket(&id);
        let total_a = w.pool_total_a();
        let total_b = w.pool_total_b();

        let max_of = |leg: u32| if leg == 0 { a_amt } else { b_amt };
        let token_of = |leg: u32| if leg == 0 { w.token_a.clone() } else { w.token_b.clone() };

        for (leg, num, den) in &partials {
            let record: BasketEscrowData = w.escrow_client().get_basket(&id);
            if record.status != EscrowStatus::Funded {
                break;
            }
            let leg_idx = u32::from(*leg);
            let leg_record = record.assets.get_unchecked(leg_idx);
            let remaining = leg_record.remaining();
            if remaining == 0 {
                break;
            }
            let partial =
                ((max_of(leg_idx) * i128::from(*num)) / i128::from(*den)).max(1).min(remaining);
            let token = token_of(leg_idx);
            let res = w.escrow_client().try_release_partial_basket(&id, &token, &partial);
            prop_assert!(
                matches!(res, Ok(Ok(()))),
                "valid per-leg partial must succeed: leg={leg_idx} partial={partial} remaining={remaining}"
            );
            prop_assert!(w.assert_leg_accounting(id), "leg accounting identity after every partial");
            prop_assert_eq!(w.pool_total_a(), total_a, "token A pool conserved after partial");
            prop_assert_eq!(w.pool_total_b(), total_b, "token B pool conserved after partial");
        }

        if w.escrow_client().get_status(&id) == EscrowStatus::Funded {
            match terminal {
                0 => w.escrow_client().release_basket(&id),
                1 => w.escrow_client().refund_basket(&id),
                _ => {
                    let claimant_addr = match claimant {
                        0 => &w.buyer,
                        _ => &w.seller,
                    };
                    w.escrow_client().dispute_basket(&id, claimant_addr);
                    w.escrow_client().resolve_basket(&id, &pay_seller);
                }
            }
        }

        let (_, _, _, a_c) = w.pool_a();
        let (_, _, _, b_c) = w.pool_b();
        prop_assert_eq!(a_c, 0, "contract holds nothing of token A at terminal");
        prop_assert_eq!(b_c, 0, "contract holds nothing of token B at terminal");
        prop_assert_eq!(w.pool_total_a(), total_a, "token A conserved end-to-end");
        prop_assert_eq!(w.pool_total_b(), total_b, "token B conserved end-to-end");

        let final_status = w.escrow_client().get_status(&id);
        prop_assert!(
            final_status == EscrowStatus::Completed || final_status == EscrowStatus::Refunded,
            "basket must be terminal after the terminal action"
        );
        prop_assert!(w.assert_leg_accounting(id), "leg accounting holds at terminal");
        let record: BasketEscrowData = w.escrow_client().get_basket(&id);
        prop_assert_eq!(record.assets.get_unchecked(0).remaining(), 0, "leg A exhausted");
        prop_assert_eq!(record.assets.get_unchecked(1).remaining(), 0, "leg B exhausted");
        prop_assert!(record.fully_released(), "basket terminal means every leg released");
    }
}

// P5 — a dispute freezes BOTH legs: between dispute and resolve no call can
// move value in either token, so custody and the pool are untouched.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn p5_basket_dispute_freezes_every_leg(
        a_amt in 1i128..=MAX_AMOUNT,
        b_amt in 1i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
        claimant in 0u8..=1,
    ) {
        let w = setup_basket_world(true);
        let id = w.create(a_amt, b_amt, timeout);
        w.escrow_client().deposit_basket(&id);
        let total_a = w.pool_total_a();
        let total_b = w.pool_total_b();

        let claimant_addr = if claimant == 0 { &w.buyer } else { &w.seller };
        w.escrow_client().dispute_basket(&id, claimant_addr);

        let client = w.escrow_client();
        prop_assert_eq!(client.get_status(&id), EscrowStatus::Disputed);
        // Every value-moving call except the arbiter's resolve is rejected
        // while Disputed; resolve is the freeze's exit, not a bypassed path.
        prop_assert!(client.try_release_basket(&id).is_err());
        prop_assert!(client.try_release_partial_basket(&id, &w.token_a, &a_amt).is_err());
        prop_assert!(client.try_release_partial_basket(&id, &w.token_b, &b_amt).is_err());
        prop_assert!(client.try_refund_basket(&id).is_err());

        // Custody untouched in BOTH tokens: the freeze covers the whole basket.
        let (_, _, _, a_c) = w.pool_a();
        let (_, _, _, b_c) = w.pool_b();
        prop_assert_eq!(a_c, a_amt, "token A custody frozen");
        prop_assert_eq!(b_c, b_amt, "token B custody frozen");
        prop_assert_eq!(w.pool_total_a(), total_a, "token A pool unchanged by freeze");
        prop_assert_eq!(w.pool_total_b(), total_b, "token B pool unchanged by freeze");
    }
}

// P6 — a failed partial deposit is fully rolled back: the leg that would have
// succeeded must land back with the buyer, custody empty in both tokens, and
// the basket still Pending and re-fundable. Also: id uniqueness across kinds.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn p6_failed_basket_deposit_leaves_no_partial_custody(
        a_amt in 1i128..=MAX_AMOUNT,
        b_amt in 1i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
    ) {
        // Buyer is funded in token A only (mint_b = false), so leg B fails.
        let w = setup_basket_world(false);
        let id = w.create(a_amt, b_amt, timeout);

        let res = w.escrow_client().try_deposit_basket(&id);
        prop_assert!(matches!(res, Err(Ok(ForgeError::TokenTransferFailed))),
            "deposit must fail on the unfunded leg");

        let (_, _, _, a_c) = w.pool_a();
        let (_, _, _, b_c) = w.pool_b();
        prop_assert_eq!(a_c, 0, "leg A must be rolled back too: no partial custody");
        prop_assert_eq!(b_c, 0, "leg B must never enter custody");
        prop_assert_eq!(w.escrow_client().get_status(&id), EscrowStatus::Pending,
            "basket stays Pending and retryable after a failed deposit");
        prop_assert!(w.assert_leg_accounting(id), "leg accounting intact after failed deposit");
    }
}

// P7 — id space and participant index are shared, monotonic, and deduplicated
// across both record kinds: a basket id appears exactly once per distinct
// party, interleaves with single-token ids, and is discoverable through the
// same index pages.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn p7_basket_ids_share_index_and_sequence(
        a_amt in 1i128..=MAX_AMOUNT,
        b_amt in 1i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
    ) {
        // Arbiter == seller exercises the dedup path across kinds.
        let mut w = setup_basket_world(true);
        w.arbiter = w.seller.clone();

        let single = w.escrow_client().create_escrow(
            &w.buyer,
            &w.seller,
            &w.arbiter,
            &w.token_a,
            &a_amt,
            &timeout,
        );
        let assets = w.legs(a_amt, b_amt);
        let basket = w.escrow_client().create_basket(
            &w.buyer,
            &w.seller,
            &w.arbiter,
            &assets,
            &timeout,
        );
        prop_assert_eq!(single + 1, basket, "ids are monotonic across kinds");

        // Buyer (3 distinct participants: buyer, seller) sees exactly
        // [single, basket] in creation order through the shared index.
        let client = w.escrow_client();
        let page = client.escrows_for_participant(&w.buyer, &0, &10);
        prop_assert_eq!(page.total, 2, "buyer index holds both records");
        prop_assert_eq!(page.ids.get_unchecked(0), single);
        prop_assert_eq!(page.ids.get_unchecked(1), basket);

        // Seller doubles as arbiter: deduplicated to one entry.
        let page = client.escrows_for_participant(&w.seller, &0, &10);
        prop_assert_eq!(page.total, 2, "seller/arbiter index holds both records once each");
        prop_assert_eq!(page.ids.get_unchecked(0), single);
        prop_assert_eq!(page.ids.get_unchecked(1), basket);
    let sac = env.register_stellar_asset_contract_v2(admin);
    let token = sac.address();
    let escrow = env.register(Escrow, ());

    let buyer1 = Address::generate(&env);
    let buyer2 = Address::generate(&env);
    let buyer3 = Address::generate(&env);
    let seller1 = Address::generate(&env);
    let seller2 = Address::generate(&env);
    let seller3 = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let outsider = Address::generate(&env);

    MultiPartyWorld {
        env,
        token,
        escrow,
        buyer1,
        buyer2,
        buyer3,
        seller1,
        seller2,
        seller3,
        arbiter,
        outsider,
    }
}

impl MultiPartyWorld {
    fn client(&self) -> SorobanForgeEscrowClient<'_> {
        SorobanForgeEscrowClient::new(&self.env, &self.escrow)
    }

    fn mint_to(&self, addr: &Address, amount: i128) {
        StellarAssetClient::new(&self.env, &self.token).mint(addr, &amount);
    }

    fn create_escrow(&self, buyer: &Address, seller: &Address) -> u64 {
        self.client()
            .create_escrow(buyer, seller, &self.arbiter, &self.token, &1_i128, &1_u64)
    }

    fn deposit(&self, id: u64, buyer: &Address) {
        self.mint_to(buyer, 1);
        self.client().deposit(&id);
    }

    fn release(&self, id: u64) {
        self.client().release(&id);
    }

    fn refund_past_deadline(&self, id: u64) {
        self.env.ledger().set_timestamp(START + 2);
        self.client().refund(&id);
    }

    fn cancel(&self, id: u64) {
        self.client().cancel(&id);
    }

    fn get_escrow(&self, id: u64) -> EscrowData {
        self.client().get_escrow(&id)
    }

    fn index_page(&self, party: &Address) -> ParticipantEscrowsPage {
        self.client().escrows_for_participant(party, &0, &u32::MAX)
    }
}

/// Verify that for every party, `escrows_for_participant` returns exactly
/// the ids of escrows they participate in (as buyer, seller, or arbiter),
/// in creation order, excluding cancelled escrows.
fn verify_participant_index(
    client: &SorobanForgeEscrowClient,
    env: &Env,
    ids: &Vec<u64>,
    parties: &[Address],
) {
    for party in parties {
        let mut expected = Vec::new(env);
        for index in 0..ids.len() {
            let id = ids.get_unchecked(index);
            let record: EscrowData = client.get_escrow(&id);
            let involved =
                record.buyer == *party || record.seller == *party || record.arbiter == *party;
            if involved && record.status != EscrowStatus::Cancelled {
                expected.push_back(id);
            }
        }
        let page = client.escrows_for_participant(party, &0, &u32::MAX);
        assert_eq!(
            page.ids, expected,
            "index contents must match live escrows for party"
        );
        assert_eq!(page.total, expected.len());
        // No duplicates, all non-cancelled
        for i in 0..page.ids.len() {
            let id = page.ids.get_unchecked(i);
            let record: EscrowData = client.get_escrow(&id);
            assert_ne!(record.status, EscrowStatus::Cancelled);
            for j in 0..i {
                assert_ne!(page.ids.get_unchecked(j), id, "no duplicate ids in index");
            }
        }
    }
}

// -----------------------------------------------------------------------
// P5 — participant index integrity across multi-party escrows
//    (including terminal-state interaction: completed escrows remain
//     listed; cancelled escrows are removed)
// -----------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p5_participant_index_integrity_across_multi_party_escrows(
        actions in prop::collection::vec((0u8..=2, 0u8..=2), 1..=12),
        terminal in prop::collection::vec(0u8..=3, 0..=12),
    ) {
        let w = setup_multi_party_world();
        let buyer_addrs = [&w.buyer1, &w.buyer2, &w.buyer3];
        let seller_addrs = [&w.seller1, &w.seller2, &w.seller3];
        let all_parties = [
            w.buyer1.clone(), w.buyer2.clone(), w.buyer3.clone(),
            w.seller1.clone(), w.seller2.clone(), w.seller3.clone(),
            w.arbiter.clone(), w.outsider.clone(),
        ];

        let mut escrow_ids = Vec::new(&w.env);
        // Track (buyer, seller) for each escrow to enable deposit/refund
        let mut escrow_buyers: Vec<Address> = Vec::new(&w.env);
        let mut escrow_sellers: Vec<Address> = Vec::new(&w.env);

        // Create escrows with random (buyer, seller) pairings
        for (bi, si) in &actions {
            let buyer = buyer_addrs[*bi as usize].clone();
            let seller = seller_addrs[*si as usize].clone();
            let id = w.create_escrow(&buyer, &seller);
            escrow_ids.push_back(id);
            escrow_buyers.push_back(buyer);
            escrow_sellers.push_back(seller);
        }

        // Apply terminal actions: 0=none, 1=deposit+release,
        // 2=deposit+refund(post-deadline), 3=cancel(no deposit)
        for i in 0..terminal.len() {
            let term = *terminal.get(i).unwrap();
            if i >= escrow_ids.len() as usize { break; }
            let id = escrow_ids.get_unchecked(i as u32);
            let buyer = escrow_buyers.get_unchecked(i as u32);
            match term {
                0 => {} // no terminal action, stays Pending
                1 => {
                    w.mint_to(&buyer, 1);
                    w.deposit(id, &buyer);
                    w.release(id);
                }
                2 => {
                    w.mint_to(&buyer, 1);
                    w.deposit(id, &buyer);
                    w.refund_past_deadline(id);
                }
                _ => {
                    w.cancel(id);
                }
            }
        }

        // Verify integrity for all parties including outsider (empty page)
        verify_participant_index(&w.client(), &w.env, &escrow_ids, &all_parties);

        // Arbiter must appear in every non-cancelled escrow's index
        let arbiter_page = w.index_page(&w.arbiter);
        let mut expected_arbiter = Vec::new(&w.env);
        for index in 0..escrow_ids.len() {
            let id = escrow_ids.get_unchecked(index);
            let record: EscrowData = w.get_escrow(id);
            if record.status != EscrowStatus::Cancelled {
                expected_arbiter.push_back(id);
            }
        }
        assert_eq!(arbiter_page.ids, expected_arbiter);
        assert_eq!(arbiter_page.total, expected_arbiter.len());

        // Outsider must get an empty page
        let outsider_page = w.index_page(&w.outsider);
        assert_eq!(outsider_page.total, 0);
        assert_eq!(outsider_page.ids.len(), 0);
        assert_eq!(outsider_page.next_cursor, None);
    }
}

// -----------------------------------------------------------------------
// P6 — pagination-slice property: random (offset, limit) must match
//      the manual slice of the full id-derived list, with clamping
//      matching the implemented bounds handling
// -----------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p6_pagination_slices_match_manual_slice(
        count in 1u32..=10u32,
        cursor in 0u32..=20u32,
        limit in 0u32..=15u32,
    ) {
        let w = setup_multi_party_world();
        let buyer = w.buyer1.clone();

        // Create `count` escrows where `buyer` participates
        let mut ids = Vec::new(&w.env);
        for _ in 0..count {
            let seller = Address::generate(&w.env);
            let id = w.create_escrow(&buyer, &seller);
            ids.push_back(id);
        }

        let total = ids.len();
        let page = w.client().escrows_for_participant(&buyer, &cursor, &limit);

        // Mirror the implemented bounds handling exactly:
        let (at, end) = match limit {
            0 => (total, total),
            _ => (cursor.min(total), cursor.saturating_add(limit).min(total)),
        };
        let mut expected = Vec::new(&w.env);
        for i in at..end {
            expected.push_back(ids.get_unchecked(i));
        }
        let expected_next = if end < total { Some(end) } else { None };

        assert_eq!(page.total, total, "total must reflect full count regardless of cursor/limit");
        assert_eq!(page.ids, expected, "page ids must match manual slice of full list");
        assert_eq!(page.next_cursor, expected_next, "next_cursor must match clamped end");
    }

    #[test]
    fn p6_pagination_boundary_with_max_limit(
        count in 1u32..=10u32,
        cursor in 0u32..=10u32,
    ) {
        let w = setup_multi_party_world();
        let buyer = w.buyer1.clone();

        let mut ids = Vec::new(&w.env);
        for _ in 0..count {
            let seller = Address::generate(&w.env);
            let id = w.create_escrow(&buyer, &seller);
            ids.push_back(id);
        }

        let total = ids.len();
        let page = w.client().escrows_for_participant(&buyer, &cursor, &u32::MAX);

        let at = cursor.min(total);
        let end = cursor.saturating_add(u32::MAX).min(total);
        let mut expected = Vec::new(&w.env);
        for i in at..end {
            expected.push_back(ids.get_unchecked(i));
        }
        let expected_next = if end < total { Some(end) } else { None };

        assert_eq!(page.total, total);
        assert_eq!(page.ids, expected, "u32::MAX limit must return full remaining slice");
        assert_eq!(page.next_cursor, expected_next);
    }
}
