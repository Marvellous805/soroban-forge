//! Randomized invariant suite (proptest).
//!
//! The hand-written unit suite pins behaviour on known, fixed values; this
//! module systematically tests the contract's fund-safety, arithmetic conservation,
//! and monotonicity invariants over large spaces of generated inputs.
//!
//! Six property invariants are exercised:
//!
//! **P1 — Total Conservation & Residue Safety.** For random amounts, cliffs,
//! and durations: across arbitrary sequences of timestamps and interleaved claims,
//! `claimed + remaining_claimable <= total_amount` holds at every point in time.
//! Rounding residue from floor-division never leaks tokens or causes payouts to
//! exceed `total_amount`. Upon full duration elapsed and final claim, the contract
//! retains 0 tokens and the beneficiary holds `total_amount` exactly.
//!
//! **P2 — Monotonicity.** For arbitrary monotonically non-decreasing timestamp
//! progressions: `vested(t)` is strictly monotonic non-decreasing ($t_a \le t_b \implies \text{vested}(t_a) \le \text{vested}(t_b)$),
//! cumulative `claimed` never decreases, and `claimable` increases monotonically
//! over time until `duration` (in the absence of claims).
//!
//! **P3 — Arbitrary Action Sequences.** For random sequences of time advancements
//! and claim attempts: `claim()` returns exactly the newly vested amount or `0`,
//! zero-amount claims make no token transfers, and total pool tokens (`beneficiary + contract`)
//! are conserved identically.
//!
//! **P4 — Tamper-Resilient Conservation.** An attacker attempting to mutate stored
//! schedule parameters (amount, claimed, start, cliff, duration) between calls can
//! never move value out of the pool beyond the token contract's balance.
//!
//! **P5 — Tranche Conservation.** For random unlock tables (including TGE
//! and never-unlocking `u64::MAX` tranches) and random claim sequences:
//! `claimed` never exceeds the table total, the contract's custody balance
//! decreases by exactly what the beneficiary gains, repeated claims never
//! overpay, the final claim transitions the schedule to `Completed` when no
//! unreachable tranche remains, and a failed transfer leaves stored state
//! untouched.
//!
//! **P6 — Tranche Claimable Monotonic With Exact Sums.** For random unlock
//! tables sampled at increasing timestamps: `claimable` is non-decreasing
//! over time and equals the exact partial sum of unlocked-but-unclaimed
//! tranches — 0 before the first offset, exact step sums between offsets,
//! and the full unlockable total at/after the last reachable offset.
//!
//! Runs are deterministic with reproducible seeds. Override the case count with
//! `PROPTEST_CASES=n cargo test -p soroban-forge-vesting props`.

use crate::{SorobanForgeVestingClient, Tranche, Vesting, VestingSchedule, VestingStatus};
use proptest::prelude::*;
use std::vec::Vec;
use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{Address, Env};

const START: u64 = 1_000_000;
const MAX_AMOUNT: i128 = 1_000_000_000_000;
const MAX_DURATION: u64 = 10 * 365 * 24 * 3600; // 10 years in seconds
const MAX_TRANCHES: u32 = 32;

// Clock reading beyond every reachable tranche offset. Deliberately not
// `u64::MAX` so `start + offset` arithmetic in product code cannot overflow;
// `u64::MAX - START` elapsed still excludes a `u64::MAX` tranche.
const FAR_FUTURE: u64 = u64::MAX / 2;

// -----------------------------------------------------------------------
// World: one fresh env, SAC, vesting contract, and accounts per case
// -----------------------------------------------------------------------

struct World {
    env: Env,
    token: Address,
    vesting: Address,
    beneficiary: Address,
}

fn setup_world() -> World {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin);
    let token = sac.address();

    let vesting = env.register(Vesting, ());

    World {
        token,
        vesting,
        beneficiary: Address::generate(&env),
        env,
    }
}

impl World {
    fn vesting_client(&self) -> SorobanForgeVestingClient<'_> {
        SorobanForgeVestingClient::new(&self.env, &self.vesting)
    }

    fn token_client(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.token)
    }

    fn mint_to_contract(&self, amount: i128) {
        StellarAssetClient::new(&self.env, &self.token).mint(&self.vesting, &amount);
    }

    fn create(&self, amount: i128, cliff: u64, duration: u64) -> u64 {
        self.vesting_client().create_schedule(
            &self.beneficiary,
            &self.token,
            &amount,
            &cliff,
            &duration,
        )
    }

    /// Create a tranche schedule, funding custody with the table total first
    /// (same mint-then-create shape as the linear fixture helpers).
    fn create_tranche(&self, tranches: &soroban_sdk::Vec<Tranche>, total_amount: i128) -> u64 {
        self.mint_to_contract(total_amount);
        self.vesting_client()
            .create_tranche_schedule(&self.beneficiary, &self.token, tranches)
    }

    /// Balances of (beneficiary, contract) — the whole pool.
    fn pool(&self) -> (i128, i128) {
        let t = self.token_client();
        (t.balance(&self.beneficiary), t.balance(&self.vesting))
    }

    /// The conserved total: everything held by the contract and the beneficiary.
    fn pool_total(&self) -> i128 {
        let (b, c) = self.pool();
        b + c
    }
}

// -----------------------------------------------------------------------
// Strategies
// -----------------------------------------------------------------------

fn arb_schedule_params() -> impl Strategy<Value = (i128, u64, u64)> {
    (1i128..=MAX_AMOUNT, 1u64..=MAX_DURATION)
        .prop_flat_map(|(amount, duration)| (Just(amount), 0u64..=duration, Just(duration)))
}

/// Build a host `Vec<Tranche>` from a raw `(unlock_at, amount)` table.
///
/// Strategies produce plain tuples because proptest strategies run outside
/// any `Env`; host objects like `soroban_sdk::Vec` must be allocated in the
/// case's own environment, which only exists once the test body starts.
fn to_tranche_vec(env: &Env, table: &[(u64, i128)]) -> soroban_sdk::Vec<Tranche> {
    let mut tranches = soroban_sdk::Vec::new(env);
    for (unlock_at, amount) in table {
        tranches.push_back(Tranche {
            unlock_at: *unlock_at,
            amount: *amount,
        });
    }
    tranches
}

/// Sum of every tranche amount, including never-unlocking ones.
fn table_total(table: &[(u64, i128)]) -> i128 {
    table.iter().map(|(_, amount)| *amount).sum()
}

/// Sum of tranches that can actually unlock (everything but `u64::MAX`).
fn unlockable_total(table: &[(u64, i128)]) -> i128 {
    table
        .iter()
        .filter(|(unlock_at, _)| *unlock_at != u64::MAX)
        .map(|(_, amount)| *amount)
        .sum()
}

/// Exact mirror of `tranche_unlocked`: cumulative sum over tranches whose
/// offset has elapsed at `elapsed` seconds after `start`.
fn expected_unlocked_at(table: &[(u64, i128)], elapsed: u64) -> i128 {
    table
        .iter()
        .filter(|(unlock_at, _)| *unlock_at <= elapsed)
        .map(|(_, amount)| *amount)
        .sum()
}

/// Exact mirror of `tranche_claimable` (unlocked minus claimed). The contract
/// uses checked subtraction, so this is exact, not saturating.
fn expected_claimable_at(table: &[(u64, i128)], elapsed: u64, claimed: i128) -> i128 {
    expected_unlocked_at(table, elapsed) - claimed
}

/// Generate `n` strictly increasing offsets in `lo..=hi` with random gaps.
///
/// Sorted uniform draws already yield exponentially-distributed gaps; entries
/// that collide are bumped forward, which keeps the sequence strictly
/// increasing without collapsing the step function into uniform steps
/// (uniform gaps would make the exact-sum property trivially easy).
fn strictly_increasing_offsets(n: usize, lo: u64, hi: u64) -> impl Strategy<Value = Vec<u64>> {
    prop::collection::vec(lo..=hi, n).prop_map(|mut offsets| {
        offsets.sort_unstable();
        for i in 1..offsets.len() {
            if offsets[i] <= offsets[i - 1] {
                offsets[i] = offsets[i - 1].saturating_add(1);
            }
        }
        offsets
    })
}

/// Positive per-tranche amounts summing to at most `cap * n`.
fn positive_amounts(n: usize, per_tranche_cap: i128) -> impl Strategy<Value = Vec<i128>> {
    prop::collection::vec(1i128..=per_tranche_cap, n)
}

/// Random tranche table: 1..=MAX_TRANCHES entries, every amount > 0,
/// strictly increasing offsets, random gaps between offsets.
///
/// Weighted mix of interesting shapes:
/// - plain random tables,
/// - a TGE first tranche (`unlock_at == 0`),
/// - a never-unlocking tail tranche (`unlock_at == u64::MAX`),
/// - both a TGE head and a `u64::MAX` tail,
/// - near-`i128::MAX` cumulative totals.
fn arb_tranche_table() -> impl Strategy<Value = Vec<(u64, i128)>> {
    prop_oneof![
        4 => (1u32..=MAX_TRANCHES).prop_flat_map(|n| {
            let cap = MAX_AMOUNT / n as i128;
            strictly_increasing_offsets(n as usize, 0, MAX_DURATION).prop_flat_map(move |offsets| {
                positive_amounts(n as usize, cap)
                    .prop_map(move |amounts| offsets.into_iter().zip(amounts).collect())
            })
        }),
        2 => (1u32..=MAX_TRANCHES).prop_flat_map(|n| {
            let cap = MAX_AMOUNT / n as i128;
            strictly_increasing_offsets(n as usize - 1, 1, MAX_DURATION).prop_flat_map(move |offsets| {
                positive_amounts(n as usize, cap).prop_map(move |amounts| {
                    let mut table = vec![(0u64, amounts[0])];
                    for (i, offset) in offsets.into_iter().enumerate() {
                        table.push((offset, amounts[i + 1]));
                    }
                    table
                })
            })
        }),
        2 => (2u32..=MAX_TRANCHES).prop_flat_map(|n| {
            let cap = MAX_AMOUNT / n as i128;
            strictly_increasing_offsets(n as usize - 1, 0, MAX_DURATION).prop_flat_map(move |offsets| {
                positive_amounts(n as usize, cap).prop_map(move |amounts| {
                    let mut table: Vec<(u64, i128)> =
                        offsets.into_iter().zip(amounts.iter().cloned()).collect();
                    table.push((u64::MAX, *amounts.last().unwrap()));
                    table
                })
            })
        }),
        1 => (3u32..=MAX_TRANCHES).prop_flat_map(|n| {
            let cap = MAX_AMOUNT / n as i128;
            strictly_increasing_offsets(n as usize - 2, 1, MAX_DURATION).prop_flat_map(move |offsets| {
                positive_amounts(n as usize, cap).prop_map(move |amounts| {
                    let mut table = vec![(0u64, amounts[0])];
                    for (i, offset) in offsets.into_iter().enumerate() {
                        table.push((offset, amounts[i + 1]));
                    }
                    table.push((u64::MAX, amounts[amounts.len() - 1]));
                    table
                })
            })
        }),
        1 => (1u32..=MAX_TRANCHES).prop_flat_map(|n| {
            let cap = i128::MAX / n as i128;
            strictly_increasing_offsets(n as usize, 0, MAX_DURATION).prop_flat_map(move |offsets| {
                positive_amounts(n as usize, cap)
                    .prop_map(move |amounts| offsets.into_iter().zip(amounts).collect())
            })
        }),
    ]
}

// -----------------------------------------------------------------------
// P1 — Total Conservation & Residue Safety
// -----------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p1_total_conservation_and_residue_safety(
        (total_amount, cliff, duration) in arb_schedule_params(),
        mut time_offsets in prop::collection::vec(0u64..=(MAX_DURATION * 2), 1..=8),
    ) {
        time_offsets.sort_unstable();

        let w = setup_world();
        w.mint_to_contract(total_amount);
        let id = w.create(total_amount, cliff, duration);

        let initial_pool = w.pool_total();
        prop_assert_eq!(initial_pool, total_amount, "initial pool must equal total_amount");

        let mut cumulative_claimed = 0i128;

        for offset in time_offsets {
            let current_time = START.saturating_add(offset);
            w.env.ledger().set_timestamp(current_time);

            let claimable = w.vesting_client().claimable(&id);
            prop_assert!(claimable >= 0, "claimable amount must never be negative");
            prop_assert!(
                cumulative_claimed + claimable <= total_amount,
                "claimed ({}) + claimable ({}) must not exceed total_amount ({})",
                cumulative_claimed,
                claimable,
                total_amount
            );

            let claimed_now = w.vesting_client().claim(&id);
            prop_assert_eq!(claimed_now, claimable, "claim() must return the exact claimable amount");
            cumulative_claimed += claimed_now;

            let (beneficiary_balance, contract_balance) = w.pool();
            prop_assert_eq!(
                beneficiary_balance,
                cumulative_claimed,
                "beneficiary balance must match cumulative claimed exactly"
            );
            prop_assert_eq!(
                contract_balance,
                total_amount - cumulative_claimed,
                "contract balance must match remaining unvested/unclaimed tokens exactly"
            );
            prop_assert_eq!(
                w.pool_total(),
                total_amount,
                "pool total must be invariant across claims"
            );
        }

        // Advance to full duration and perform final claim to verify residue safety
        w.env.ledger().set_timestamp(START.saturating_add(duration).saturating_add(1));
        let final_claimable = w.vesting_client().claimable(&id);
        let final_claimed = w.vesting_client().claim(&id);
        prop_assert_eq!(final_claimed, final_claimable);
        cumulative_claimed += final_claimed;

        let (final_b, final_c) = w.pool();
        prop_assert_eq!(cumulative_claimed, total_amount, "all tokens must be claimable at completion");
        prop_assert_eq!(final_b, total_amount, "beneficiary must receive exact total_amount with 0 token loss");
        prop_assert_eq!(final_c, 0, "contract balance must be drained to exactly 0");
        prop_assert_eq!(
            w.vesting_client().get_status(&id),
            VestingStatus::Completed,
            "status must be Completed after full claim"
        );
    }
}

// -----------------------------------------------------------------------
// P2 — Monotonicity Invariants
// -----------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p2_monotonicity_over_time(
        (total_amount, cliff, duration) in arb_schedule_params(),
        mut offsets in prop::collection::vec(0u64..=(MAX_DURATION * 2), 2..=10),
    ) {
        offsets.sort_unstable();

        let w = setup_world();
        w.mint_to_contract(total_amount);
        let id = w.create(total_amount, cliff, duration);

        let mut prev_claimable = 0i128;
        let mut prev_time = 0u64;

        for offset in offsets {
            let current_time = START.saturating_add(offset);
            w.env.ledger().set_timestamp(current_time);

            let claimable = w.vesting_client().claimable(&id);

            if offset < cliff {
                prop_assert_eq!(claimable, 0, "before cliff, claimable must be 0");
                prop_assert_eq!(
                    w.vesting_client().get_status(&id),
                    VestingStatus::Locked,
                    "before cliff, status must be Locked"
                );
            } else {
                prop_assert!(
                    claimable >= prev_claimable,
                    "claimable ({}) at t={} must be >= previous claimable ({}) at t={}",
                    claimable,
                    current_time,
                    prev_claimable,
                    prev_time
                );
                if offset >= duration {
                    prop_assert_eq!(
                        claimable,
                        total_amount,
                        "at or after duration, claimable must equal total_amount"
                    );
                }
            }

            prev_claimable = claimable;
            prev_time = current_time;
        }
    }
}

// -----------------------------------------------------------------------
// P3 — Arbitrary Interleaved Action Sequences
// -----------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p3_interleaved_action_sequences(
        (total_amount, cliff, duration) in arb_schedule_params(),
        mut actions in prop::collection::vec((0u64..=(MAX_DURATION * 2), prop::bool::ANY), 1..=12),
    ) {
        actions.sort_by_key(|(offset, _)| *offset);

        let w = setup_world();
        w.mint_to_contract(total_amount);
        let id = w.create(total_amount, cliff, duration);

        let mut cumulative_claimed = 0i128;

        for (offset, do_claim) in actions {
            let current_time = START.saturating_add(offset);
            w.env.ledger().set_timestamp(current_time);

            let pool_before = w.pool_total();
            let claimable = w.vesting_client().claimable(&id);

            if do_claim {
                let claimed = w.vesting_client().claim(&id);
                prop_assert_eq!(claimed, claimable, "claim() must pay exact claimable amount");
                cumulative_claimed += claimed;
            }

            let pool_after = w.pool_total();
            prop_assert_eq!(pool_before, pool_after, "pool total must be conserved across action");
            prop_assert_eq!(pool_after, total_amount, "pool total must equal total_amount");

            let (b_bal, c_bal) = w.pool();
            prop_assert_eq!(b_bal, cumulative_claimed);
            prop_assert_eq!(c_bal, total_amount - cumulative_claimed);
        }
    }
}

// -----------------------------------------------------------------------
// P4 — Tamper-Resilient Conservation
// -----------------------------------------------------------------------

fn mutate_vesting_record(w: &World, id: u64, kind: u8, delta: i128) {
    let key = crate::DataKey::Schedule(id);
    w.env.as_contract(&w.vesting, || {
        if let Some(mut rec) = w.env.storage().instance().get::<_, VestingSchedule>(&key) {
            match kind {
                // Fuzz total_amount
                0 => {
                    rec.total_amount = rec.total_amount.wrapping_add(delta).max(1);
                }
                // Fuzz claimed
                1 => {
                    rec.claimed = rec.claimed.wrapping_add(delta).max(0);
                }
                // Fuzz cliff
                2 => {
                    rec.cliff = (rec.cliff as i128).wrapping_add(delta).max(0) as u64;
                }
                // Fuzz duration
                3 => {
                    rec.duration = (rec.duration as i128).wrapping_add(delta).max(1) as u64;
                }
                _ => {}
            }
            w.env.storage().instance().set(&key, &rec);
        }
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p4_tampered_storage_never_violates_pool_conservation(
        (total_amount, cliff, duration) in arb_schedule_params(),
        mutations in prop::collection::vec((0u8..=3, -10_000i128..=10_000i128), 0..=4),
    ) {
        let w = setup_world();
        w.mint_to_contract(total_amount);
        let id = w.create(total_amount, cliff, duration);

        for (kind, delta) in mutations {
            mutate_vesting_record(&w, id, kind, delta);
        }

        w.env.ledger().set_timestamp(START.saturating_add(duration).saturating_add(10));

        let client = w.vesting_client();
        let pool_before = w.pool_total();

        match client.try_claim(&id) {
            Ok(Ok(_paid)) => {
                let (b, c) = w.pool();
                prop_assert!(c >= 0, "contract balance must not be negative");
                prop_assert!(b >= 0, "beneficiary balance must not be negative");
                prop_assert_eq!(b + c, pool_before, "pool total must be conserved on successful claim");
            }
            Err(Ok(ForgeError::TokenTransferFailed)) => {
                // Over-payment prevented by token balance check; pool unchanged
                prop_assert_eq!(w.pool_total(), pool_before);
            }
            Err(Ok(ForgeError::ArithmeticOverflow)) => {
                // Arithmetic overflow prevented; pool unchanged
                prop_assert_eq!(w.pool_total(), pool_before);
            }
            Err(Ok(ForgeError::NotFound)) => {}
            other => panic!("unexpected outcome under tampering: {:?}", other),
        }
    }
}

// -----------------------------------------------------------------------
// P5 — Tranche Conservation
// -----------------------------------------------------------------------
//
// Over random tranche tables and random claim sequences:
// - claimed never exceeds total_amount
// - contract custody balance decreases by exactly what the beneficiary gains
// - repeated claims never overpay
// - final claim transitions the schedule to Completed (when no u64::MAX
//   tranche withholds part of the total)
// - failed transfers leave state untouched

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p5_tranche_conservation(
        table in arb_tranche_table(),
        // Claim offsets share the unlock offsets' range so claims land
        // between unlocks (partial claims) instead of clumping after the
        // full unlock; the final drain below covers the everything-at-once
        // pattern.
        mut claim_offsets in prop::collection::vec(0u64..=MAX_DURATION, 1..=8),
    ) {
        claim_offsets.sort_unstable();

        let w = setup_world();
        let total_amount = table_total(&table);
        let id = w.create_tranche(&to_tranche_vec(&w.env, &table), total_amount);

        let initial_pool = w.pool_total();
        prop_assert_eq!(initial_pool, total_amount, "initial pool must equal total_amount");

        let mut cumulative_claimed = 0i128;

        for offset in claim_offsets {
            let current_time = START.saturating_add(offset);
            w.env.ledger().set_timestamp(current_time);

            let claimable = w.vesting_client().claimable(&id);
            prop_assert!(claimable >= 0, "claimable amount must never be negative");

            // Exact partial sum: unlocked-but-unclaimed at this instant.
            let elapsed = current_time - START;
            prop_assert_eq!(
                claimable,
                expected_claimable_at(&table, elapsed, cumulative_claimed),
                "claimable must equal the exact unlocked-minus-claimed sum at elapsed={}",
                elapsed
            );

            prop_assert!(
                cumulative_claimed + claimable <= total_amount,
                "claimed ({}) + claimable ({}) must not exceed total_amount ({})",
                cumulative_claimed,
                claimable,
                total_amount
            );

            let claimed_now = w.vesting_client().claim(&id);
            prop_assert_eq!(claimed_now, claimable, "claim() must return the exact claimable amount");
            cumulative_claimed += claimed_now;

            let (beneficiary_balance, contract_balance) = w.pool();
            prop_assert_eq!(
                beneficiary_balance,
                cumulative_claimed,
                "beneficiary balance must match cumulative claimed exactly"
            );
            prop_assert_eq!(
                contract_balance,
                total_amount - cumulative_claimed,
                "contract custody must have decreased by exactly what the beneficiary gained"
            );
            prop_assert_eq!(
                w.pool_total(),
                total_amount,
                "pool total must be invariant across claims"
            );

            // A repeated claim at the same instant must never overpay.
            let repeat = w.vesting_client().claim(&id);
            prop_assert_eq!(repeat, 0, "repeated claim at the same instant must pay 0");
            let (b_repeat, c_repeat) = w.pool();
            prop_assert_eq!(b_repeat, cumulative_claimed, "repeat claim must not move beneficiary balance");
            prop_assert_eq!(c_repeat, total_amount - cumulative_claimed, "repeat claim must not move contract balance");
        }

        // At the last reachable offset (inclusive boundary), everything
        // unlockable is claimable.
        let last_reachable = table
            .iter()
            .rev()
            .find(|(unlock_at, _)| *unlock_at != u64::MAX)
            .map(|(unlock_at, _)| *unlock_at)
            .expect("strategy never produces an all-u64::MAX table");
        w.env
            .ledger()
            .set_timestamp(START.saturating_add(last_reachable));
        prop_assert_eq!(
            w.vesting_client().claimable(&id),
            unlockable_total(&table) - cumulative_claimed,
            "at the last reachable offset, the full unlockable remainder must be claimable"
        );

        // One final claim of everything at a far-future instant: reachable
        // tranches drain; a u64::MAX tranche never unlocks and stays in
        // custody.
        w.env.ledger().set_timestamp(FAR_FUTURE);
        let final_claimable = w.vesting_client().claimable(&id);
        prop_assert_eq!(
            final_claimable,
            unlockable_total(&table) - cumulative_claimed,
            "far-future claimable must be exactly the unlockable remainder"
        );
        let final_claimed = w.vesting_client().claim(&id);
        prop_assert_eq!(final_claimed, final_claimable);
        cumulative_claimed += final_claimed;

        let (final_b, final_c) = w.pool();
        prop_assert_eq!(
            final_b,
            unlockable_total(&table),
            "beneficiary must hold exactly the unlockable total"
        );
        prop_assert_eq!(
            final_c,
            total_amount - unlockable_total(&table),
            "contract must retain exactly the never-unlocking tranche amounts"
        );
        prop_assert_eq!(w.pool_total(), total_amount, "pool total must be conserved at the end");

        // Lifecycle: Completed only when claimed == total_amount; a schedule
        // with an unclaimed u64::MAX tranche stays Vesting forever.
        if unlockable_total(&table) == total_amount {
            prop_assert_eq!(
                w.vesting_client().get_status(&id),
                VestingStatus::Completed,
                "fully-claimed schedule must transition to Completed on the final claim"
            );
        } else {
            prop_assert_eq!(
                w.vesting_client().get_status(&id),
                VestingStatus::Vesting,
                "a schedule with an unclaimed u64::MAX tranche must remain Vesting"
            );
        }

        // The stored record agrees with the tokens that actually moved.
        let stored = w.vesting_client().get_tranche_schedule(&id);
        prop_assert_eq!(stored.claimed, cumulative_claimed, "stored claimed must equal cumulative paid");
        prop_assert_eq!(stored.total_amount, total_amount, "stored total must equal the table sum");

        // Failed transfers leave state untouched: custody now holds exactly
        // the never-unlocking remainder, so a TGE schedule asking for one
        // token more than the contract owns must fail the SEP-41 transfer
        // (same bucketing as P4) without touching either record.
        let pool_before_probe = w.pool_total();
        let probe_amount = total_amount - unlockable_total(&table) + 1;
        let probe_table = [(0u64, probe_amount)];
        let probe_id = w
            .vesting_client()
            .create_tranche_schedule(&w.beneficiary, &w.token, &to_tranche_vec(&w.env, &probe_table));
        let probe_before = w.vesting_client().get_tranche_schedule(&probe_id);

        let probe_result = w.vesting_client().try_claim(&probe_id);
        prop_assert!(
            matches!(probe_result, Err(Ok(ForgeError::TokenTransferFailed))),
            "over-custody claim must fail with TokenTransferFailed, got {:?}",
            probe_result
        );
        prop_assert_eq!(w.pool_total(), pool_before_probe, "failed transfer must not move the pool");
        let probe_after = w.vesting_client().get_tranche_schedule(&probe_id);
        prop_assert_eq!(probe_after.claimed, probe_before.claimed, "failed transfer must not touch claimed");
        prop_assert_eq!(probe_after.status, probe_before.status, "failed transfer must not touch status");
    }
}

// -----------------------------------------------------------------------
// P6 — Tranche Claimable Monotonic With Exact Sums
// -----------------------------------------------------------------------
//
// For random tranche tables:
// - claimable is non-decreasing over randomly increasing timestamps
// - claimable equals the exact partial sum of unlocked-but-unclaimed tranches
//   at sampled points:
//   * 0 before the first offset
//   * exact step sums between offsets
//   * the full unlockable total at/after the last reachable offset

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p6_tranche_claimable_monotonic_with_exact_sums(
        table in arb_tranche_table(),
        // Sample timestamps share the unlock offsets' range so samples hit
        // the step function's plateaus and edges, not just its final state.
        mut sample_offsets in prop::collection::vec(0u64..=MAX_DURATION, 3..=10),
    ) {
        sample_offsets.sort_unstable();

        let first_reachable = table
            .first()
            .filter(|(unlock_at, _)| *unlock_at != u64::MAX)
            .map(|(unlock_at, _)| *unlock_at)
            .expect("strategy never produces an all-u64::MAX table");

        let w = setup_world();
        let total_amount = table_total(&table);
        let id = w.create_tranche(&to_tranche_vec(&w.env, &table), total_amount);

        let mut baseline = 0i128;
        let mut cumulative_claimed = 0i128;
        let mut prev_elapsed = 0u64;

        for (index, offset) in sample_offsets.iter().enumerate() {
            let current_time = START.saturating_add(*offset);
            w.env.ledger().set_timestamp(current_time);
            let elapsed = current_time - START;

            let claimable = w.vesting_client().claimable(&id);

            // Exact partial sum of unlocked-but-unclaimed tranches.
            prop_assert_eq!(
                claimable,
                expected_claimable_at(&table, elapsed, cumulative_claimed),
                "claimable must equal the exact partial sum at elapsed={}",
                elapsed
            );

            // Monotonicity holds between samples (no claims in between);
            // after a claim the baseline resets to the post-claim value.
            prop_assert!(
                claimable >= baseline,
                "claimable ({}) at elapsed={} must be >= previous baseline ({}) at elapsed={}",
                claimable,
                elapsed,
                baseline,
                prev_elapsed
            );

            // Strictly before the first offset nothing is claimable.
            if elapsed < first_reachable {
                prop_assert_eq!(claimable, 0, "claimable must be 0 before the first offset");
            }

            baseline = claimable;

            // Interleave one claim every other sample so the "minus claimed"
            // term is exercised; claim() must pay the exact view.
            if index % 2 == 1 {
                let claimed_now = w.vesting_client().claim(&id);
                prop_assert_eq!(claimed_now, claimable, "claim() must pay the exact claimable view");
                cumulative_claimed += claimed_now;
                prop_assert_eq!(
                    w.vesting_client().claimable(&id),
                    0,
                    "after claiming everything unlocked so far, claimable must be 0"
                );
                baseline = 0;
            }

            prev_elapsed = elapsed;
        }

        // The three canonical sample points, observed on a fresh schedule so
        // `claimed == 0` and the sums are the raw step function.
        let w2 = setup_world();
        let id2 = w2.create_tranche(&to_tranche_vec(&w2.env, &table), total_amount);

        // Point 1: before the first offset (or at creation time for a TGE
        // head, where elapsed 0 IS the unlock instant).
        if first_reachable == 0 {
            prop_assert_eq!(
                w2.vesting_client().claimable(&id2),
                table[0].1,
                "at a TGE first offset, claimable at the creation instant must equal the TGE amount"
            );
        } else {
            w2.env.ledger().set_timestamp(START);
            prop_assert_eq!(
                w2.vesting_client().claimable(&id2),
                0,
                "claimable must be 0 strictly before the first offset"
            );
        }

        // Point 2: strictly between two offsets — the exact step sum of every
        // tranche unlocked so far (only meaningful when a gap exists).
        let reachable: std::vec::Vec<u64> = table
            .iter()
            .filter(|(unlock_at, _)| *unlock_at != u64::MAX)
            .map(|(unlock_at, _)| *unlock_at)
            .collect();
        if let Some(pair) = reachable.windows(2).find(|pair| pair[1] - pair[0] >= 2) {
            let mid = pair[0] + (pair[1] - pair[0]) / 2;
            w2.env.ledger().set_timestamp(START.saturating_add(mid));
            prop_assert_eq!(
                w2.vesting_client().claimable(&id2),
                expected_unlocked_at(&table, mid),
                "claimable strictly between offsets must equal the exact step sum at elapsed={}",
                mid
            );
            assert!(pair[0] < mid && mid < pair[1], "sample point must lie strictly between offsets");
        }

        // Point 3: at the last reachable offset (inclusive boundary) — the
        // full unlockable total, u64::MAX tail excluded.
        let last_reachable = reachable[reachable.len() - 1];
        w2.env
            .ledger()
            .set_timestamp(START.saturating_add(last_reachable));
        prop_assert_eq!(
            w2.vesting_client().claimable(&id2),
            unlockable_total(&table),
            "claimable at the last reachable offset must be the full unlockable total"
        );
    }
}
