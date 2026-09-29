use super::*;
use soroban_forge_test_utils::TestAccounts;
use soroban_sdk::testutils::{Address as _, Events as _, Ledger as _};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::Env;

const START: u64 = 1_000_000;
const CLIFF: u64 = 1_000;
const DURATION: u64 = 4_000;
const TOTAL: i128 = 10_000;

/// Build a fresh env with mocked auths, a registered contract, and named
/// accounts. The generated client borrows the env, so it cannot be
/// returned from a helper.
#[macro_export]
macro_rules! setup {
    () => {{
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(START);

        // Real Stellar Asset Contract — the same fixture escrow uses.
        let admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(admin);
        let token = sac.address();
        let token_admin = StellarAssetClient::new(&env, &token);
        let token_client = TokenClient::new(&env, &token);
        let contract_id = env.register(Vesting, ());
        let client = SorobanForgeVestingClient::new(&env, &contract_id);
        let accounts = TestAccounts::generate(&env);
        // Fund the schedule contract up front with the full allocation,
        // the way a deployment is topped up before schedules run.
        token_admin.mint(&contract_id, &TOTAL);
        (env, token, token_client, contract_id, client, accounts)
    }};
}

// NOTE: In soroban-sdk 27.0.6, entrypoint `require_auth` checks are verified
// via `env.mock_auths` (enforce mode). An unauthorized caller or a signature
// with mismatched arguments aborts the invocation with `Err(Err(InvokeError::Abort))`.
// Contract self-authorization for token settlement is implicit in the host:
// the executing contract's own transfer is auto-approved and requires no
// separate user signature frame.

fn create(client: &SorobanForgeVestingClient<'_>, token: &Address, accounts: &TestAccounts) -> u64 {
    client.create_schedule(&accounts.user1, token, &TOTAL, &CLIFF, &DURATION)
}

#[test]
fn create_schedule_succeeds_and_is_locked() {
    let (_env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    assert_eq!(client.get_status(&id), VestingStatus::Locked);
    assert_eq!(client.claimable(&id), 0);
}

#[test]
fn lifecycle_events_cover_creation_claim_and_silent_zero_claim() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let before_create = env.events().all().len();
    let id = create(&client, &token, &accounts);
    assert_eq!(env.events().all().len(), before_create + 1);

    let after_create = env.events().all().len();
    assert_eq!(client.claim(&id), 0);
    assert_eq!(env.events().all().len(), after_create);

    env.ledger().set_timestamp(START + DURATION);
    let before_claim = env.events().all().len();
    assert_eq!(client.claim(&id), TOTAL);
    assert!(env.events().all().len() > before_claim);
}

#[test]
fn create_schedule_assigns_distinct_ids() {
    let (_env, token, _tc, _cid, client, accounts) = setup!();
    let id1 = create(&client, &token, &accounts);
    let id2 = create(&client, &token, &accounts);
    assert_ne!(id1, id2);
}

#[test]
fn create_schedule_without_cliff_starts_vesting() {
    let (_env, _token, _tc, _cid, client, accounts) = setup!();
    let id = client.create_schedule(
        &accounts.user1,
        &accounts.validator,
        &TOTAL,
        &0_u64,
        &DURATION,
    );
    assert_eq!(client.get_status(&id), VestingStatus::Vesting);
}

#[test]
fn create_schedule_rejects_zero_total() {
    let (_env, _token, _tc, _cid, client, accounts) = setup!();
    let err = client
        .try_create_schedule(
            &accounts.user1,
            &accounts.validator,
            &0_i128,
            &CLIFF,
            &DURATION,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
}

#[test]
fn create_schedule_rejects_zero_duration() {
    let (_env, _token, _tc, _cid, client, accounts) = setup!();
    let err = client
        .try_create_schedule(&accounts.user1, &accounts.validator, &TOTAL, &CLIFF, &0_u64)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
}

#[test]
fn create_schedule_rejects_cliff_after_duration() {
    let (_env, _token, _tc, _cid, client, accounts) = setup!();
    let err = client
        .try_create_schedule(
            &accounts.user1,
            &accounts.validator,
            &TOTAL,
            &5_000_u64,
            &4_000_u64,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
}

#[test]
fn claimable_before_cliff_is_zero() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    // Halfway between start and the cliff.
    env.ledger().set_timestamp(START + CLIFF / 2);
    assert_eq!(client.claimable(&id), 0);
}

#[test]
fn claimable_at_cliff_is_zero() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    env.ledger().set_timestamp(START + CLIFF);
    assert_eq!(client.claimable(&id), 0);
}

#[test]
fn claim_before_cliff_returns_zero() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    env.ledger().set_timestamp(START + CLIFF / 2);
    assert_eq!(client.claim(&id), 0);
}

#[test]
fn claimable_midway_is_half() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    // Halfway through the vesting window (cliff .. duration).
    env.ledger()
        .set_timestamp(START + CLIFF + (DURATION - CLIFF) / 2);
    assert_eq!(client.claimable(&id), TOTAL / 2);
}

#[test]
fn claimable_at_duration_is_full() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    env.ledger().set_timestamp(START + DURATION);
    assert_eq!(client.claimable(&id), TOTAL);
}

#[test]
fn claimable_after_duration_is_full() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    env.ledger().set_timestamp(START + DURATION + 1);
    assert_eq!(client.claimable(&id), TOTAL);
}

#[test]
fn claim_pays_exact_amount() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    env.ledger()
        .set_timestamp(START + CLIFF + (DURATION - CLIFF) / 2);
    assert_eq!(client.claim(&id), TOTAL / 2);
}

#[test]
fn repeated_claims_never_overpay_or_underpay() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);

    // Claim half at the midway point.
    env.ledger()
        .set_timestamp(START + CLIFF + (DURATION - CLIFF) / 2);
    assert_eq!(client.claim(&id), TOTAL / 2);
    assert_eq!(client.claimable(&id), 0);

    // Advance past the end; the remaining half becomes claimable.
    env.ledger().set_timestamp(START + DURATION + 100);
    assert_eq!(client.claim(&id), TOTAL - TOTAL / 2);
    assert_eq!(client.claimable(&id), 0);

    // A further claim is a no-op.
    assert_eq!(client.claim(&id), 0);
    assert_eq!(client.get_status(&id), VestingStatus::Completed);
}

#[test]
fn claim_after_end_completes_status() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    env.ledger().set_timestamp(START + DURATION + 1);
    assert_eq!(client.get_status(&id), VestingStatus::Vesting);
    assert_eq!(client.claim(&id), TOTAL);
    assert_eq!(client.get_status(&id), VestingStatus::Completed);
}

#[test]
fn claim_without_cliff_vests_from_start() {
    let (env, _token, _tc, _cid, client, accounts) = setup!();
    let id = client.create_schedule(
        &accounts.user1,
        &accounts.validator,
        &TOTAL,
        &0_u64,
        &DURATION,
    );
    env.ledger().set_timestamp(START + DURATION / 2);
    assert_eq!(client.claimable(&id), TOTAL / 2);
}

#[test]
fn cliff_equals_duration_vests_at_once() {
    let (env, _token, _tc, _cid, client, accounts) = setup!();
    let id = client.create_schedule(
        &accounts.user1,
        &accounts.validator,
        &TOTAL,
        &DURATION,
        &DURATION,
    );
    env.ledger().set_timestamp(START + DURATION - 1);
    assert_eq!(client.claimable(&id), 0);
    env.ledger().set_timestamp(START + DURATION);
    assert_eq!(client.claimable(&id), TOTAL);
}

#[test]
fn claim_missing_schedule_is_not_found() {
    let (_env, _token, _tc, _cid, client, _accounts) = setup!();
    let err = client.try_claim(&999).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::NotFound);
}

#[test]
fn claimable_missing_schedule_is_not_found() {
    let (_env, _token, _tc, _cid, client, _accounts) = setup!();
    let err = client.try_claimable(&999).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::NotFound);
}

#[test]
fn get_status_missing_schedule_is_not_found() {
    let (_env, _token, _tc, _cid, client, _accounts) = setup!();
    let err = client.try_get_status(&999).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::NotFound);
}

#[test]
fn claimable_overflow_is_reported() {
    let (env, _token, _tc, _cid, client, accounts) = setup!();
    // A huge total with a non-trivial elapsed time overflows the
    // intermediate `total * elapsed` product.
    let id = client.create_schedule(
        &accounts.user1,
        &accounts.validator,
        &i128::MAX,
        &0_u64,
        &1_000_u64,
    );
    env.ledger().set_timestamp(START + 500);
    let err = client.try_claimable(&id).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::ArithmeticOverflow);
}

// -------------------------------------------------------------------
// Settlement: claim pays real SEP-41 tokens
// -------------------------------------------------------------------

#[test]
fn claim_transfers_vested_tokens_to_beneficiary() {
    let (env, token, tc, contract_id, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);

    // Halfway through the vesting window: 5_000 claimable.
    env.ledger()
        .set_timestamp(START + CLIFF + (DURATION - CLIFF) / 2);
    assert_eq!(client.claim(&id), TOTAL / 2);

    // Tokens actually moved, and the schedule recorded the claim.
    assert_eq!(tc.balance(&accounts.user1), TOTAL / 2);
    assert_eq!(tc.balance(&contract_id), TOTAL - TOTAL / 2);
    assert_eq!(client.claimable(&id), 0);
    assert_eq!(client.get_status(&id), VestingStatus::Vesting);
}

#[test]
fn final_claim_settles_remainder_and_completes() {
    let (env, token, tc, contract_id, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);

    env.ledger()
        .set_timestamp(START + CLIFF + (DURATION - CLIFF) / 2);
    assert_eq!(client.claim(&id), TOTAL / 2);

    env.ledger().set_timestamp(START + DURATION + 100);
    assert_eq!(client.claim(&id), TOTAL - TOTAL / 2);

    // Full allocation paid out; contract drained; schedule completed.
    assert_eq!(tc.balance(&accounts.user1), TOTAL);
    assert_eq!(tc.balance(&contract_id), 0);
    assert_eq!(client.get_status(&id), VestingStatus::Completed);
    assert_eq!(client.claimable(&id), 0);

    // A repeated claim after completion is a silent no-op: no transfer.
    assert_eq!(client.claim(&id), 0);
    assert_eq!(tc.balance(&accounts.user1), TOTAL);
    assert_eq!(tc.balance(&contract_id), 0);
}

#[test]
fn repeated_claims_settle_token_balances_each_time() {
    let (env, token, tc, contract_id, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);

    // Three separate claims across the window; each moves exactly the
    // newly claimable amount and nothing more.
    env.ledger().set_timestamp(START + CLIFF + 1_000);
    assert_eq!(client.claim(&id), 3_333);
    assert_eq!(tc.balance(&accounts.user1), 3_333);

    env.ledger().set_timestamp(START + CLIFF + 2_000);
    assert_eq!(client.claim(&id), 3_333);
    assert_eq!(tc.balance(&accounts.user1), 6_666);

    env.ledger().set_timestamp(START + DURATION + 1);
    assert_eq!(client.claim(&id), 3_334);
    assert_eq!(tc.balance(&accounts.user1), TOTAL);
    assert_eq!(tc.balance(&contract_id), 0);
    assert_eq!(client.get_status(&id), VestingStatus::Completed);
}

#[test]
fn failed_transfer_leaves_claim_unchanged() {
    let (env, token, tc, contract_id, client, accounts) = setup!();
    // Schedule promises double what the contract actually holds.
    let id = client.create_schedule(&accounts.user1, &token, &(TOTAL * 2), &0_u64, &DURATION);
    env.ledger().set_timestamp(START + DURATION + 1);
    assert_eq!(client.claimable(&id), TOTAL * 2);

    let err = client.try_claim(&id).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::TokenTransferFailed);

    // Nothing moved, nothing recorded: balances and schedule unchanged.
    assert_eq!(tc.balance(&accounts.user1), 0);
    assert_eq!(tc.balance(&contract_id), TOTAL);
    assert_eq!(client.claimable(&id), TOTAL * 2);
    assert_eq!(client.get_status(&id), VestingStatus::Vesting);
}

#[test]
fn zero_claim_issues_no_token_transfer() {
    let (env, token, tc, contract_id, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);

    // Before the cliff: returns 0, moves nothing.
    env.ledger().set_timestamp(START + CLIFF / 2);
    assert_eq!(client.claim(&id), 0);
    assert_eq!(tc.balance(&accounts.user1), 0);
    assert_eq!(tc.balance(&contract_id), TOTAL);

    // A second immediate claim right after a payout is also a no-op.
    env.ledger()
        .set_timestamp(START + CLIFF + (DURATION - CLIFF) / 2);
    assert_eq!(client.claim(&id), TOTAL / 2);
    assert_eq!(client.claim(&id), 0);
    assert_eq!(tc.balance(&accounts.user1), TOTAL / 2);
    assert_eq!(tc.balance(&contract_id), TOTAL - TOTAL / 2);
}

#[test]
fn floor_division_residue_stays_claimable_until_final_claim() {
    let (env, token, tc, contract_id, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);

    // 1_000 / 3_000 through the window:
    // 10_000 * 1_000 / 3_000 = 3_333 (floored) — one stroop of residue
    // stays behind, and the final claim pays it out in full.
    env.ledger().set_timestamp(START + CLIFF + 1_000);
    let first = client.claim(&id);
    assert_eq!(first, 3_333);

    env.ledger().set_timestamp(START + DURATION + 1);
    let rest = client.claim(&id);
    assert_eq!(first + rest, TOTAL);
    assert_eq!(tc.balance(&accounts.user1), TOTAL);
    assert_eq!(tc.balance(&contract_id), 0);
}

// -------------------------------------------------------------------
// Boundary, Cliff == Duration, and Timing Tests
// -------------------------------------------------------------------

#[test]
fn boundary_cliff_and_duration_step_progression() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);

    // At creation start:
    env.ledger().set_timestamp(START);
    assert_eq!(client.get_status(&id), VestingStatus::Locked);
    assert_eq!(client.claimable(&id), 0);

    // 1 second before cliff:
    env.ledger().set_timestamp(START + CLIFF - 1);
    assert_eq!(client.get_status(&id), VestingStatus::Locked);
    assert_eq!(client.claimable(&id), 0);

    // At cliff boundary:
    env.ledger().set_timestamp(START + CLIFF);
    assert_eq!(client.get_status(&id), VestingStatus::Vesting);
    assert_eq!(client.claimable(&id), 0);

    // 1 second after cliff: floor(10_000 * 1 / 3_000) = 3
    env.ledger().set_timestamp(START + CLIFF + 1);
    assert_eq!(client.get_status(&id), VestingStatus::Vesting);
    assert_eq!(client.claimable(&id), 3);

    // 1 second before full duration: floor(10_000 * 2_999 / 3_000) = 9_996
    env.ledger().set_timestamp(START + DURATION - 1);
    assert_eq!(client.get_status(&id), VestingStatus::Vesting);
    assert_eq!(client.claimable(&id), 9_996);

    // Exactly at duration:
    env.ledger().set_timestamp(START + DURATION);
    assert_eq!(client.get_status(&id), VestingStatus::Vesting);
    assert_eq!(client.claimable(&id), TOTAL);

    // After duration:
    env.ledger().set_timestamp(START + DURATION + 10_000);
    assert_eq!(client.get_status(&id), VestingStatus::Vesting);
    assert_eq!(client.claimable(&id), TOTAL);
}

#[test]
fn interleaved_partial_claims_across_arbitrary_timestamps() {
    let (env, token, tc, contract_id, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);

    // Five sequential claim points:
    // Period is 3_000 seconds, total is 10_000.
    // Step 1: elapsed 300 -> 10_000 * 300 / 3_000 = 1_000.
    env.ledger().set_timestamp(START + CLIFF + 300);
    assert_eq!(client.claim(&id), 1_000);
    assert_eq!(tc.balance(&accounts.user1), 1_000);
    assert_eq!(client.claimable(&id), 0);

    // Step 2: elapsed 750 -> 10_000 * 750 / 3_000 = 2_500 vested. Claimable = 2_500 - 1_000 = 1_500.
    env.ledger().set_timestamp(START + CLIFF + 750);
    assert_eq!(client.claim(&id), 1_500);
    assert_eq!(tc.balance(&accounts.user1), 2_500);
    assert_eq!(client.claimable(&id), 0);

    // Step 3: elapsed 1_500 -> 10_000 * 1_500 / 3_000 = 5_000 vested. Claimable = 5_000 - 2_500 = 2_500.
    env.ledger().set_timestamp(START + CLIFF + 1_500);
    assert_eq!(client.claim(&id), 2_500);
    assert_eq!(tc.balance(&accounts.user1), 5_000);
    assert_eq!(client.claimable(&id), 0);

    // Step 4: elapsed 2_250 -> 10_000 * 2_250 / 3_000 = 7_500 vested. Claimable = 7_500 - 5_000 = 2_500.
    env.ledger().set_timestamp(START + CLIFF + 2_250);
    assert_eq!(client.claim(&id), 2_500);
    assert_eq!(tc.balance(&accounts.user1), 7_500);
    assert_eq!(client.claimable(&id), 0);

    // Step 5: elapsed 3_000 (end of duration) -> full 10_000 vested. Remainder = 2_500.
    env.ledger().set_timestamp(START + DURATION);
    assert_eq!(client.claim(&id), 2_500);
    assert_eq!(tc.balance(&accounts.user1), TOTAL);
    assert_eq!(tc.balance(&contract_id), 0);
    assert_eq!(client.get_status(&id), VestingStatus::Completed);
    assert_eq!(client.claimable(&id), 0);
}

#[test]
fn repeated_zero_claims_at_and_before_cliff() {
    let (env, token, tc, contract_id, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);

    // Repeated claims before cliff:
    env.ledger().set_timestamp(START + 100);
    assert_eq!(client.claim(&id), 0);
    assert_eq!(client.claim(&id), 0);
    assert_eq!(client.claim(&id), 0);
    assert_eq!(tc.balance(&accounts.user1), 0);
    assert_eq!(tc.balance(&contract_id), TOTAL);

    // Repeated claims at cliff:
    env.ledger().set_timestamp(START + CLIFF);
    assert_eq!(client.claim(&id), 0);
    assert_eq!(client.claim(&id), 0);
    assert_eq!(tc.balance(&accounts.user1), 0);
    assert_eq!(tc.balance(&contract_id), TOTAL);
}

#[test]
fn timestamp_u64_max_edge_case() {
    let (env, token, tc, contract_id, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);

    // Ledger timestamp at maximum u64 value:
    env.ledger().set_timestamp(u64::MAX);
    assert_eq!(client.claimable(&id), TOTAL);
    assert_eq!(client.claim(&id), TOTAL);
    assert_eq!(tc.balance(&accounts.user1), TOTAL);
    assert_eq!(tc.balance(&contract_id), 0);
    assert_eq!(client.get_status(&id), VestingStatus::Completed);
}

#[test]
fn total_amount_i128_max_at_duration_succeeds() {
    let (env, _token, _tc, _cid, client, accounts) = setup!();
    let id = client.create_schedule(
        &accounts.user1,
        &accounts.validator,
        &i128::MAX,
        &0_u64,
        &1_000_u64,
    );
    // At or after duration, vested_amount returns total_amount directly
    // without computing total_amount * elapsed / period, so it does not overflow.
    env.ledger().set_timestamp(START + 1_000);
    assert_eq!(client.claimable(&id), i128::MAX);

    env.ledger().set_timestamp(START + 2_000);
    assert_eq!(client.claimable(&id), i128::MAX);
}

#[test]
fn start_plus_duration_u64_overflow_reported() {
    let (env, _token, _tc, _cid, client, accounts) = setup!();
    // A schedule whose duration would cause start + duration to overflow u64.
    let id = client.create_schedule(
        &accounts.user1,
        &accounts.validator,
        &TOTAL,
        &0_u64,
        &u64::MAX,
    );
    env.ledger().set_timestamp(START + 100);
    let err = client.try_claimable(&id).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::ArithmeticOverflow);

    let claim_err = client.try_claim(&id).unwrap_err().unwrap();
    assert_eq!(claim_err, ForgeError::ArithmeticOverflow);
}

#[test]
fn monotonic_id_counter_overflow_reported() {
    let (env, token, _tc, contract_id, client, accounts) = setup!();
    // Set Count key to u64::MAX directly in contract storage:
    env.as_contract(&contract_id, || {
        env.storage().instance().set(&DataKey::Count, &u64::MAX);
    });

    let err = client
        .try_create_schedule(&accounts.user1, &token, &TOTAL, &CLIFF, &DURATION)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::ArithmeticOverflow);
}

// ---------------------------------------------------------------------------
// `get_schedule` record view (issue #125)
// ---------------------------------------------------------------------------

/// The view returns the complete stored record for an existing id — every
/// creation parameter round-trips, and the initial state matches what
/// `claimable`/`get_status` read.
#[test]
fn get_schedule_returns_the_full_stored_record() {
    let (_env, token, _tc, _cid, client, accounts) = setup!();
    let id = client.create_schedule(&accounts.user1, &token, &TOTAL, &CLIFF, &DURATION);

    let schedule = client.get_schedule(&id);
    assert_eq!(schedule.beneficiary, accounts.user1);
    assert_eq!(schedule.token, token);
    assert_eq!(schedule.total_amount, TOTAL);
    assert_eq!(schedule.start, START);
    assert_eq!(schedule.cliff, CLIFF);
    assert_eq!(schedule.duration, DURATION);
    assert_eq!(schedule.claimed, 0);
    assert_eq!(schedule.status, VestingStatus::Locked);
    assert_eq!(client.claimable(&id), 0);
    assert_eq!(client.get_status(&id), VestingStatus::Locked);
}

#[test]
fn get_schedule_unknown_id_returns_not_found() {
    let (_env, _token, _tc, _cid, client, _accounts) = setup!();
    let err = client.try_get_schedule(&7_u64).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::NotFound);
}

/// The two kinds partition one id space: a tranche id is `NotFound` in the
/// linear view (and vice versa, per `get_tranche_schedule`).
#[test]
fn get_schedule_tranche_id_returns_not_found() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let tranches = soroban_sdk::Vec::from_array(
        &env,
        [
            Tranche {
                unlock_at: 0,
                amount: 2_000,
            },
            Tranche {
                unlock_at: 1_000,
                amount: 8_000,
            },
        ],
    );
    let id = client.create_tranche_schedule(&accounts.user1, &token, &tranches);

    let err = client.try_get_schedule(&id).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::NotFound);
    // The tranche view does return that record.
    let tranche = client.get_tranche_schedule(&id);
    assert_eq!(tranche.total_amount, 10_000);
}

/// `claimed` in the record advances with completed claims: the view and the
/// claim path read the same storage, and the stored status follows the
/// settled state.
#[test]
fn get_schedule_reflects_claims() {
    let (env, token, tc, contract_id, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    assert_eq!(client.get_schedule(&id).claimed, 0);

    // Halfway through the ramp: floor(TOTAL * 2_000 / 3_000) vests.
    env.ledger().set_timestamp(START + CLIFF + DURATION / 2);
    let payout = client.claim(&id);
    assert_eq!(payout, 6_666);

    let schedule = client.get_schedule(&id);
    assert_eq!(schedule.claimed, 6_666);
    assert_eq!(schedule.status, VestingStatus::Vesting);
    assert_eq!(tc.balance(&accounts.user1), 6_666);
    assert_eq!(tc.balance(&contract_id), TOTAL - 6_666);

    // After the final claim the record is Completed with everything claimed.
    env.ledger().set_timestamp(START + DURATION);
    assert_eq!(client.claim(&id), TOTAL - 6_666);
    let schedule = client.get_schedule(&id);
    assert_eq!(schedule.claimed, TOTAL);
    assert_eq!(schedule.status, VestingStatus::Completed);
}

/// The view is read-only: repeated reads return the same record and the id
/// counter (the only cross-call state) never moves.
#[test]
fn get_schedule_is_read_only() {
    let (env, token, _tc, contract_id, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    let before = client.get_schedule(&id);
    let count_before: u64 = env.as_contract(&contract_id, || {
        env.storage().instance().get(&DataKey::Count).unwrap_or(0)
    });
    assert_eq!(count_before, 1);

    let again = client.get_schedule(&id);
    assert_eq!(again.claimed, before.claimed);
    assert_eq!(again.start, before.start);
    assert_eq!(again.total_amount, before.total_amount);

    let count_after: u64 = env.as_contract(&contract_id, || {
        env.storage().instance().get(&DataKey::Count).unwrap_or(0)
    });
    assert_eq!(count_after, count_before);

    // A read against an empty id leaves nothing behind either.
    let err = client.try_get_schedule(&99_u64).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::NotFound);
    let count_final: u64 = env.as_contract(&contract_id, || {
        env.storage().instance().get(&DataKey::Count).unwrap_or(0)
    });
    assert_eq!(count_final, count_before);
}

/// No authorization anywhere in the view's path: it succeeds under a blank
/// envelope, like `claimable` and `get_status`.
#[test]
fn get_schedule_requires_no_auth() {
    let (env, token, _tc, _cid, client, accounts) = setup!();
    let id = create(&client, &token, &accounts);
    env.set_auths(&[]);
    let schedule = client.get_schedule(&id);
    assert_eq!(schedule.beneficiary, accounts.user1);
}

// --- Tranche Vesting Validation & Unit Tests ---

#[test]
fn create_tranche_schedule_rejects_empty_table() {
    let (env, _, token, _, client, _) = setup!();
    env.mock_all_auths();
    let beneficiary = Address::generate(&env);

    let tranches = soroban_sdk::Vec::new(&env);
    let res = client.try_create_tranche_schedule(&beneficiary, &token.address, &tranches);
    assert_eq!(res.unwrap_err().unwrap(), ForgeError::InvalidInput);
}

#[test]
fn create_tranche_schedule_rejects_too_many_tranches() {
    let (env, _, token, _, client, _) = setup!();
    env.mock_all_auths();
    let beneficiary = Address::generate(&env);

    let mut tranches = soroban_sdk::Vec::new(&env);
    for i in 0..=32 {
        tranches.push_back(Tranche {
            unlock_at: i * 100,
            amount: 100,
        });
    }

    let res = client.try_create_tranche_schedule(&beneficiary, &token.address, &tranches);
    assert_eq!(res.unwrap_err().unwrap(), ForgeError::InvalidInput);
}

#[test]
fn create_tranche_schedule_rejects_non_positive_amount() {
    let (env, _, token, _, client, _) = setup!();
    env.mock_all_auths();
    let beneficiary = Address::generate(&env);

    let mut tranches = soroban_sdk::Vec::new(&env);
    tranches.push_back(Tranche {
        unlock_at: 100,
        amount: 0,
    });

    let res = client.try_create_tranche_schedule(&beneficiary, &token.address, &tranches);
    assert_eq!(res.unwrap_err().unwrap(), ForgeError::InvalidInput);
}

#[test]
fn create_tranche_schedule_rejects_non_increasing_offsets() {
    let (env, _, token, _, client, _) = setup!();
    env.mock_all_auths();
    let beneficiary = Address::generate(&env);

    let mut equal_offsets = soroban_sdk::Vec::new(&env);
    equal_offsets.push_back(Tranche {
        unlock_at: 100,
        amount: 500,
    });
    equal_offsets.push_back(Tranche {
        unlock_at: 100,
        amount: 500,
    });
    let res = client.try_create_tranche_schedule(&beneficiary, &token.address, &equal_offsets);
    assert_eq!(res.unwrap_err().unwrap(), ForgeError::InvalidInput);

    let mut decreasing = soroban_sdk::Vec::new(&env);
    decreasing.push_back(Tranche {
        unlock_at: 200,
        amount: 500,
    });
    decreasing.push_back(Tranche {
        unlock_at: 100,
        amount: 500,
    });
    let res2 = client.try_create_tranche_schedule(&beneficiary, &token.address, &decreasing);
    assert_eq!(res2.unwrap_err().unwrap(), ForgeError::InvalidInput);
}

#[test]
fn create_tranche_schedule_rejects_arithmetic_overflow() {
    let (env, _, token, _, client, _) = setup!();
    env.mock_all_auths();
    let beneficiary = Address::generate(&env);

    let mut tranches = soroban_sdk::Vec::new(&env);
    tranches.push_back(Tranche {
        unlock_at: 100,
        amount: i128::MAX,
    });
    tranches.push_back(Tranche {
        unlock_at: 200,
        amount: 1,
    });

    let res = client.try_create_tranche_schedule(&beneficiary, &token.address, &tranches);
    assert_eq!(res.unwrap_err().unwrap(), ForgeError::ArithmeticOverflow);
}

#[test]
fn tranche_tge_and_max_u64_behavior() {
    let (env, _, token, _, client, _) = setup!();
    env.mock_all_auths();
    let beneficiary = Address::generate(&env);

    let mut tranches = soroban_sdk::Vec::new(&env);
    tranches.push_back(Tranche {
        unlock_at: 0,
        amount: 1_000,
    });
    tranches.push_back(Tranche {
        unlock_at: u64::MAX,
        amount: 9_000,
    });

    let id = client.create_tranche_schedule(&beneficiary, &token.address, &tranches);

    assert_eq!(client.claimable(&id), 1_000);

    env.ledger().set_timestamp(u64::MAX / 2);
    assert_eq!(client.claimable(&id), 1_000);
}

#[test]
fn tranche_claim_step_function_and_settlement() {
    let (env, _, token, _, client, _) = setup!();
    env.mock_all_auths();
    let beneficiary = Address::generate(&env);

    let mut tranches = soroban_sdk::Vec::new(&env);
    tranches.push_back(Tranche {
        unlock_at: 100,
        amount: 2_000,
    });
    tranches.push_back(Tranche {
        unlock_at: 200,
        amount: 3_000,
    });

    let id = client.create_tranche_schedule(&beneficiary, &token.address, &tranches);

    // Before first tranche
    env.ledger().set_timestamp(START + 50);
    assert_eq!(client.claimable(&id), 0);
    assert_eq!(client.claim(&id), 0);

    // At first tranche unlock (claims 2,000)
    env.ledger().set_timestamp(START + 100);
    assert_eq!(client.claimable(&id), 2_000);
    assert_eq!(client.claim(&id), 2_000);

    // Between tranches (since 2,000 was already claimed, 0 remain unclaimed until next unlock)
    env.ledger().set_timestamp(START + 150);
    assert_eq!(client.claimable(&id), 0);
    assert_eq!(client.claim(&id), 0);

    // At second tranche unlock (total vested = 5,000; 2,000 claimed -> 3,000 claimable)
    env.ledger().set_timestamp(START + 200);
    assert_eq!(client.claimable(&id), 3_000);
    assert_eq!(client.claim(&id), 3_000);

    let status = client.get_status(&id);
    assert_eq!(status, VestingStatus::Completed);
}

#[test]
fn linear_and_tranche_coexistence() {
    let (env, _, token, _, client, _) = setup!();
    let beneficiary1 = Address::generate(&env);
    let beneficiary2 = Address::generate(&env);

    let id_linear = client.create_schedule(&beneficiary1, &token.address, &10_000, &100, &200);

    let mut tranches = soroban_sdk::Vec::new(&env);
    tranches.push_back(Tranche {
        unlock_at: 100,
        amount: 5_000,
    });
    let id_tranche = client.create_tranche_schedule(&beneficiary2, &token.address, &tranches);

    assert_ne!(id_linear, id_tranche);
    assert!(client.try_get_tranche_schedule(&id_linear).is_err());
}
