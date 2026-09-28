//! Metered-usage suite for subscription payments: exact overage math, period
//! boundaries, caps, and rollover.
//!
//! Layered like the crate's other suites:
//! - **Pure derivation** — [`period_amount`] and its helpers, with no
//!   `Env`-backed state, exercised directly at bucket/cap/overflow
//!   boundaries and against the worked example in the contract docs.
//! - **Contract behaviour** — declaration, recording, settlement, period
//!   rollover, and the failure-isolation guarantee that a failed transfer
//!   leaves every meter intact so the retry bills the same amount again.
//!
//! Negative authorization lives in `authz.rs` and randomized invariants in
//! `props.rs`, matching the crate's split.

use crate::{
    billable_buckets, metric_overage, period_amount, usage_units, validate_quotas, MetricQuota,
    SorobanForgeSubscriptionPaymentsClient, SubscriptionPayments, UsageRecord, MAX_QUOTAS,
};
use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::testutils::{Address as _, Events as _, Ledger as _};
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::xdr::{ContractEventBody, Int128Parts, ScSymbol, ScVal};
use soroban_sdk::{Address, Env, Symbol, Vec};
use std::format;

const START: u64 = 1_000_000;
const PERIOD: u64 = 1_000;
const AMOUNT: i128 = 250;
const SUB_ID: u64 = 1;

/// The metered dimensions used across the suite.
const CALLS: &str = "api_calls";
const BYTES: &str = "storage_bytes";

/// Build a quota: `included_units` free, `overage_price` per started
/// `bucket_units`, optionally capped.
fn quota(
    env: &Env,
    metric: &str,
    included_units: u64,
    overage_price: i128,
    bucket_units: u64,
    max_overage_units: Option<u64>,
) -> MetricQuota {
    MetricQuota {
        metric: Symbol::new(env, metric),
        included_units,
        overage_price,
        bucket_units,
        max_overage_units,
    }
}

/// Two-metric plan: 10k calls included then 25 per 1k; 1 MB included then 10
/// per 100k, capped at 5 MB of overage.
fn calls_and_bytes(env: &Env) -> Vec<MetricQuota> {
    Vec::from_slice(
        env,
        &[
            quota(env, CALLS, 10_000, 25, 1_000, None),
            quota(env, BYTES, 1_000_000, 10, 100_000, Some(5_000_000)),
        ],
    )
}

/// Open-period meter list for a subscription, as the derivation reads it.
fn usage(env: &Env, recorded: &[(&str, u64)]) -> Vec<UsageRecord> {
    let mut records = Vec::new(env);
    for (metric, units) in recorded {
        records.push_back(UsageRecord {
            subscription_id: SUB_ID,
            metric: Symbol::new(env, metric),
            units: *units,
            period_start: START,
        });
    }
    records
}

macro_rules! setup {
    () => {{
        let env = Env::default();
        env.mock_all_auths_allowing_non_root_auth();
        env.ledger().set_timestamp(START);

        let admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(admin);
        let token = sac.address();
        let token_admin = StellarAssetClient::new(&env, &token);
        let token_client = soroban_sdk::token::Client::new(&env, &token);

        let contract_id = env.register(SubscriptionPayments, ());
        let client = SorobanForgeSubscriptionPaymentsClient::new(&env, &contract_id);
        let accounts = soroban_forge_test_utils::TestAccounts::generate(&env);

        token_admin.mint(&accounts.user1, &10_000_000_i128);
        let subscription_id = client.subscribe(
            &accounts.user1,
            &accounts.validator,
            &token,
            &AMOUNT,
            &PERIOD,
        );
        (env, token, token_client, client, accounts, subscription_id)
    }};
}

/// Subscribe, declare the quota list built by `$quotas(&env)`, and return the
/// fixture. The closure runs against the fixture's own `Env`, because a
/// `soroban_sdk::Vec` is an env-tagged object and cannot cross envs.
macro_rules! setup_with_quotas {
    ($quotas:expr) => {{
        let (env, token, token_client, client, accounts, subscription_id) = setup!();
        client.set_quotas(&subscription_id, &($quotas)(&env));
        (env, token, token_client, client, accounts, subscription_id)
    }};
}

// ---------------------------------------------------------------------------
// Pure derivation: buckets and caps
// ---------------------------------------------------------------------------

#[test]
fn billable_buckets_round_up_to_started_buckets() {
    // A started bucket is billed in full: 1 unit into a 1k bucket is a full
    // bucket, and the 1001st unit starts the second.
    assert_eq!(billable_buckets(1, None, 1_000), 1);
    assert_eq!(billable_buckets(1_000, None, 1_000), 1);
    assert_eq!(billable_buckets(1_001, None, 1_000), 2);
    assert_eq!(billable_buckets(2_000, None, 1_000), 2);
    assert_eq!(billable_buckets(2_001, None, 1_000), 3);
    // Exact division never rounds up into a phantom bucket.
    assert_eq!(billable_buckets(3_000, None, 1_000), 3);
    // Bucket sizes other than 1k behave the same way.
    assert_eq!(billable_buckets(99, None, 100), 1);
    assert_eq!(billable_buckets(100, None, 100), 1);
    assert_eq!(billable_buckets(101, None, 100), 2);
    // Unit buckets bill one bucket per unit.
    assert_eq!(billable_buckets(7, None, 1), 7);
    // Nothing over the line bills nothing.
    assert_eq!(billable_buckets(0, None, 1_000), 0);
}

#[test]
fn billable_buckets_clamps_to_cap_before_bucketing() {
    // A cap bounds billable overage units, so the bill is at most
    // ceil(cap / bucket) buckets even when the cap is not bucket-aligned.
    assert_eq!(billable_buckets(1_500, Some(1_500), 1_000), 2);
    assert_eq!(billable_buckets(u64::MAX, Some(1_500), 1_000), 2);
    // A cap below the bucket size still bills one started bucket: the cap
    // bounds units, and billing is per started bucket.
    assert_eq!(billable_buckets(5_000, Some(1), 1_000), 1);
    // A zero cap bills nothing at all.
    assert_eq!(billable_buckets(u64::MAX, Some(0), 1_000), 0);
    // A cap above the usage is a no-op.
    assert_eq!(billable_buckets(2_500, Some(1_000_000), 1_000), 3);
    // Without a cap, usage itself is the bound.
    assert_eq!(
        billable_buckets(u64::MAX, None, 1_000),
        u64::MAX.div_ceil(1_000)
    );
}

#[test]
fn billable_buckets_is_total_at_the_u64_boundary() {
    // Nothing wraps at the top of the range: the bucket *count* is derived by
    // division, so no `units * bucket` product can overflow.
    assert_eq!(billable_buckets(u64::MAX, None, 1), u64::MAX);
    assert_eq!(
        billable_buckets(u64::MAX, None, 1_000),
        u64::MAX.div_ceil(1_000)
    );
    assert_eq!(
        billable_buckets(u64::MAX, Some(u64::MAX), 1_000),
        u64::MAX.div_ceil(1_000)
    );
    // A zero bucket size bills nothing rather than dividing by zero.
    assert_eq!(billable_buckets(1_000, None, 0), 0);
    assert_eq!(billable_buckets(u64::MAX, Some(u64::MAX), 0), 0);
}

// ---------------------------------------------------------------------------
// Pure derivation: per-metric overage
// ---------------------------------------------------------------------------

#[test]
fn metric_overage_leaves_included_units_free() {
    let env = Env::default();
    let q = quota(&env, CALLS, 10_000, 25, 1_000, None);
    // Under and exactly at the inclusion: no overage at all.
    assert_eq!(metric_overage(0, &q), Ok(0));
    assert_eq!(metric_overage(9_999, &q), Ok(0));
    assert_eq!(metric_overage(10_000, &q), Ok(0));
    // One unit past the inclusion starts a bucket.
    assert_eq!(metric_overage(10_001, &q), Ok(25));
    assert_eq!(metric_overage(11_000, &q), Ok(25));
    assert_eq!(metric_overage(11_001, &q), Ok(50));
}

#[test]
fn metric_overage_is_per_bucket_not_per_unit() {
    let env = Env::default();
    let q = quota(&env, CALLS, 1_000, 7, 1_000, None);
    // 2_500 - 1_000 = 1_500 overage units => 2 buckets => 14, not 10.5.
    assert_eq!(metric_overage(2_500, &q), Ok(14));
    // Fractional prices are impossible by construction: the price is per
    // bucket, so no unit of precision is lost to division.
    let single = quota(&env, CALLS, 0, 1, 1_000, None);
    assert_eq!(metric_overage(999, &single), Ok(1));
}

#[test]
fn metric_overage_applies_the_cap() {
    let env = Env::default();
    let q = quota(&env, BYTES, 0, 10, 100_000, Some(5_000_000));
    assert_eq!(metric_overage(5_000_000, &q), Ok(500));
    // Past the cap the bill is pinned: 50 buckets of 100k, not more.
    assert_eq!(metric_overage(5_000_001, &q), Ok(500));
    assert_eq!(metric_overage(6_000_000, &q), Ok(500));
    assert_eq!(metric_overage(u64::MAX, &q), Ok(500));
    // A zero cap is a hard ceiling: usage is metered but never billed.
    let hard_stop = quota(&env, CALLS, 0, 25, 1_000, Some(0));
    assert_eq!(metric_overage(u64::MAX, &hard_stop), Ok(0));
}

#[test]
fn metric_overage_of_a_free_or_priceless_metric_is_zero() {
    let env = Env::default();
    assert_eq!(
        metric_overage(u64::MAX, &quota(&env, CALLS, 0, 0, 1_000, None)),
        Ok(0)
    );
    assert_eq!(
        metric_overage(0, &quota(&env, CALLS, 0, i128::MAX, 1, None)),
        Ok(0)
    );
}

#[test]
fn metric_overage_is_overflow_safe() {
    let env = Env::default();
    // The largest representable bill is exact, not an error.
    let max_price = quota(&env, CALLS, 0, i128::MAX, 1, None);
    assert_eq!(metric_overage(1, &max_price), Ok(i128::MAX));
    // One bucket more is not representable and must fail loudly rather than
    // wrap into a negative (or wrong) bill.
    assert_eq!(
        metric_overage(2, &max_price),
        Err(ForgeError::ArithmeticOverflow)
    );
    let big_price = quota(&env, CALLS, 0, i128::MAX / 2, 1, None);
    assert_eq!(metric_overage(2, &big_price), Ok(i128::MAX - 1));
    assert_eq!(
        metric_overage(4, &big_price),
        Err(ForgeError::ArithmeticOverflow)
    );
    // Maximum units with a maximum bucket count cannot overflow the cast.
    let max_units = quota(&env, CALLS, 0, 1, 1, None);
    assert_eq!(
        metric_overage(u64::MAX, &max_units),
        Ok(i128::from(u64::MAX))
    );
    // A price that makes the maximal bucket count itself unrepresentable.
    let huge_price = quota(&env, CALLS, 0, i128::MAX / (u64::MAX as i128) + 1, 1, None);
    assert_eq!(
        metric_overage(u64::MAX, &huge_price),
        Err(ForgeError::ArithmeticOverflow)
    );
}

// ---------------------------------------------------------------------------
// Pure derivation: the period amount
// ---------------------------------------------------------------------------

#[test]
fn period_amount_without_quotas_is_exactly_the_base() {
    let env = Env::default();
    // The flat flow: no quotas means the base, whatever the meters say.
    assert_eq!(
        period_amount(AMOUNT, &Vec::new(&env), &usage(&env, &[])),
        Ok(AMOUNT)
    );
    assert_eq!(
        period_amount(AMOUNT, &Vec::new(&env), &usage(&env, &[(CALLS, 999_999)])),
        Ok(AMOUNT)
    );
}

#[test]
fn period_amount_sums_every_declared_metric() {
    let env = Env::default();
    let quotas = calls_and_bytes(&env);
    let recorded = usage(&env, &[(CALLS, 12_000), (BYTES, 1_400_000)]);
    // base 250 + 2 buckets * 25 (calls) + 4 buckets * 10 (bytes) = 340.
    assert_eq!(period_amount(250, &quotas, &recorded), Ok(340));
}

#[test]
fn period_amount_ignores_usage_of_undeclared_metrics() {
    let env = Env::default();
    let quotas = Vec::from_slice(&env, &[quota(&env, CALLS, 10_000, 25, 1_000, None)]);
    let recorded = usage(&env, &[(CALLS, 11_000), (BYTES, 5_000_000)]);
    // Only the declared metric bills: 250 + 1 bucket * 25.
    assert_eq!(period_amount(250, &quotas, &recorded), Ok(275));
    // A declared-but-unused metric contributes nothing.
    assert_eq!(
        period_amount(
            250,
            &calls_and_bytes(&env),
            &usage(&env, &[(CALLS, 10_000)])
        ),
        Ok(250)
    );
}

#[test]
fn period_amount_matches_the_documented_worked_example() {
    let env = Env::default();
    // base 1_000; 10k calls included then 25 per 1k (uncapped);
    // 1 MB included then 10 per 100k, capped at 5 MB of overage.
    let quotas = calls_and_bytes(&env);

    // Period 1: 12k calls (2 buckets), no storage: 1_000 + 50.
    assert_eq!(
        period_amount(1_000, &quotas, &usage(&env, &[(CALLS, 12_000)])),
        Ok(1_050)
    );
    // Period 2: exactly the included calls (free), 1.4 MB (4 buckets): 1_040.
    assert_eq!(
        period_amount(
            1_000,
            &quotas,
            &usage(&env, &[(CALLS, 10_000), (BYTES, 1_400_000)])
        ),
        Ok(1_040)
    );
    // Period 3: 10.5k calls (a half bucket, billed in full = 25) and 6 MB
    // (overage pinned at the 5 MB cap = 50 buckets = 500): 1_525.
    assert_eq!(
        period_amount(
            1_000,
            &quotas,
            &usage(&env, &[(CALLS, 10_500), (BYTES, 6_000_000)])
        ),
        Ok(1_525)
    );
    // Three periods: three bases plus 50 + 40 + 525 of exactly-derived
    // overage. Every figure is an integer product — nothing was rounded
    // along the way and no remainder is carried between periods.
    let three_periods = 1_050 + 1_040 + 1_525;
    assert_eq!(three_periods, 3 * 1_000 + 50 + 40 + 525);
    assert_eq!(three_periods, 3_615);
}

#[test]
fn period_amount_is_overflow_safe() {
    let env = Env::default();
    let quotas = Vec::from_slice(&env, &[quota(&env, CALLS, 0, i128::MAX, 1, None)]);
    // Base plus an unrepresentable overage is an error, not a wrapped bill.
    assert_eq!(
        period_amount(1, &quotas, &usage(&env, &[(CALLS, 4)])),
        Err(ForgeError::ArithmeticOverflow)
    );
    // The largest representable sum is exact.
    let exact = Vec::from_slice(&env, &[quota(&env, CALLS, 0, i128::MAX, 1, None)]);
    assert_eq!(
        period_amount(0, &exact, &usage(&env, &[(CALLS, 1)])),
        Ok(i128::MAX)
    );
    // A negative base is the caller's business, but the sum stays checked: it
    // is exact here, and it fails loudly rather than wrapping the other way.
    let negative = Vec::from_slice(&env, &[quota(&env, CALLS, 0, i128::MIN, 1, None)]);
    assert_eq!(
        period_amount(-1, &negative, &usage(&env, &[(CALLS, 1)])),
        Err(ForgeError::ArithmeticOverflow)
    );
}

#[test]
fn usage_units_defaults_to_zero_for_an_unused_metric() {
    let env = Env::default();
    let recorded = usage(&env, &[(CALLS, 12_000)]);
    assert_eq!(usage_units(&recorded, &Symbol::new(&env, CALLS)), 12_000);
    assert_eq!(usage_units(&recorded, &Symbol::new(&env, BYTES)), 0);
    assert_eq!(usage_units(&usage(&env, &[]), &Symbol::new(&env, CALLS)), 0);
}

// ---------------------------------------------------------------------------
// Quota validation
// ---------------------------------------------------------------------------

#[test]
fn validate_quotas_accepts_empty_and_well_formed_lists() {
    let env = Env::default();
    assert_eq!(validate_quotas(&Vec::new(&env)), Ok(()));
    assert_eq!(validate_quotas(&calls_and_bytes(&env)), Ok(()));
    // A zero included-units quota is a pure overage plan.
    assert_eq!(
        validate_quotas(&Vec::from_slice(&env, &[quota(&env, CALLS, 0, 1, 1, None)])),
        Ok(())
    );
    // A zero cap is a hard ceiling, not a malformed quota.
    assert_eq!(
        validate_quotas(&Vec::from_slice(
            &env,
            &[quota(&env, CALLS, 0, 1, 1, Some(0))]
        )),
        Ok(())
    );
}

#[test]
fn validate_quotas_rejects_malformed_entries() {
    let env = Env::default();
    // A zero bucket size would divide by zero.
    assert_eq!(
        validate_quotas(&Vec::from_slice(
            &env,
            &[quota(&env, CALLS, 0, 25, 0, None)]
        )),
        Err(ForgeError::InvalidInput)
    );
    // A negative price would bill the subscriber for usage.
    assert_eq!(
        validate_quotas(&Vec::from_slice(
            &env,
            &[quota(&env, CALLS, 0, -1, 1_000, None)]
        )),
        Err(ForgeError::InvalidInput)
    );
    // One bad entry poisons the whole list, not just its own position.
    assert_eq!(
        validate_quotas(&Vec::from_slice(
            &env,
            &[
                quota(&env, CALLS, 0, 25, 1_000, None),
                quota(&env, BYTES, 0, 10, 0, None),
            ]
        )),
        Err(ForgeError::InvalidInput)
    );
}

#[test]
fn validate_quotas_rejects_duplicate_metrics() {
    let env = Env::default();
    // A duplicated metric would make the per-metric sum order-dependent, so
    // the exactness of the bill would depend on declaration order.
    assert_eq!(
        validate_quotas(&Vec::from_slice(
            &env,
            &[
                quota(&env, CALLS, 0, 25, 1_000, None),
                quota(&env, CALLS, 0, 10, 100, None),
            ]
        )),
        Err(ForgeError::InvalidInput)
    );
    // A duplicated metric is rejected regardless of where it appears.
    assert_eq!(
        validate_quotas(&Vec::from_slice(
            &env,
            &[
                quota(&env, CALLS, 0, 25, 1_000, None),
                quota(&env, BYTES, 0, 10, 100, None),
                quota(&env, CALLS, 0, 10, 100, None),
            ]
        )),
        Err(ForgeError::InvalidInput)
    );
}

#[test]
fn validate_quotas_bounds_the_list_length() {
    let env = Env::default();
    let mut at_max = Vec::new(&env);
    for index in 0..MAX_QUOTAS {
        at_max.push_back(quota(&env, &format!("metric_{index}"), 0, 1, 1, None));
    }
    assert_eq!(at_max.len(), MAX_QUOTAS);
    assert_eq!(validate_quotas(&at_max), Ok(()));

    at_max.push_back(quota(&env, "one_too_many", 0, 1, 1, None));
    assert_eq!(validate_quotas(&at_max), Err(ForgeError::InvalidInput));
}

// ---------------------------------------------------------------------------
// Quota declaration
// ---------------------------------------------------------------------------

#[test]
fn set_quotas_stores_the_declaration_on_the_subscription() {
    let (_env, _token, _tc, client, _accounts, subscription_id) = setup!();
    let quotas = calls_and_bytes(&client.env);
    client.set_quotas(&subscription_id, &quotas);

    let subscription = client.get_subscription(&subscription_id);
    assert_eq!(subscription.quotas, quotas);
    assert_eq!(client.quote_period(&subscription_id), AMOUNT);
}

#[test]
fn subscriptions_start_flat() {
    let (_env, _token, _tc, client, _accounts, subscription_id) = setup!();
    let subscription = client.get_subscription(&subscription_id);
    assert!(subscription.quotas.is_empty());
    assert_eq!(client.quote_period(&subscription_id), AMOUNT);
}

#[test]
fn set_quotas_to_an_empty_list_returns_the_subscription_to_flat_pricing() {
    let (_env, _token, _tc, client, _accounts, subscription_id) =
        setup_with_quotas!(calls_and_bytes);
    let env = client.env.clone();
    client.set_quotas(&subscription_id, &Vec::new(&env));

    assert!(client.get_subscription(&subscription_id).quotas.is_empty());
    assert_eq!(client.quote_period(&subscription_id), AMOUNT);
}

#[test]
fn set_quotas_rejects_malformed_declarations() {
    let (_env, _token, _tc, client, _accounts, subscription_id) = setup!();
    let env = client.env.clone();
    let before = client.get_subscription(&subscription_id);

    for malformed in [
        Vec::from_slice(&env, &[quota(&env, CALLS, 0, 25, 0, None)]),
        Vec::from_slice(&env, &[quota(&env, CALLS, 0, -25, 1_000, None)]),
        Vec::from_slice(
            &env,
            &[
                quota(&env, CALLS, 0, 25, 1_000, None),
                quota(&env, CALLS, 0, 10, 100, None),
            ],
        ),
    ] {
        assert_eq!(
            client
                .try_set_quotas(&subscription_id, &malformed)
                .unwrap_err()
                .unwrap(),
            ForgeError::InvalidInput
        );
    }

    // No partial declaration survived any rejected call.
    assert_eq!(client.get_subscription(&subscription_id), before);
}

#[test]
fn set_quotas_rejects_more_than_the_maximum_declared_metrics() {
    let (_env, _token, _tc, client, _accounts, subscription_id) = setup!();
    let env = client.env.clone();
    let mut too_many = Vec::new(&env);
    for index in 0..=MAX_QUOTAS {
        too_many.push_back(quota(&env, &format!("metric_{index}"), 0, 1, 1, None));
    }
    assert_eq!(
        client
            .try_set_quotas(&subscription_id, &too_many)
            .unwrap_err()
            .unwrap(),
        ForgeError::InvalidInput
    );
    assert!(client.get_subscription(&subscription_id).quotas.is_empty());
}

#[test]
fn set_quotas_is_rejected_once_the_open_period_has_usage() {
    let (env, _token, _tc, client, _accounts, subscription_id) = setup!();
    let symbol = Symbol::new(&env, CALLS);
    client.set_quotas(&subscription_id, &calls_and_bytes(&env));
    client.record_usage(&subscription_id, &symbol, &12_000);

    // The subscriber may not reprice units they have already had metered.
    let repriced = Vec::from_slice(&env, &[quota(&env, CALLS, 0, 1_000_000, 1, None)]);
    assert_eq!(
        client
            .try_set_quotas(&subscription_id, &repriced)
            .unwrap_err()
            .unwrap(),
        ForgeError::InvalidInput
    );
    assert_eq!(client.get_usage(&subscription_id, &symbol).units, 12_000);
    assert_eq!(client.quote_period(&subscription_id), 250 + 50);
}

#[test]
fn set_quotas_is_allowed_again_after_the_period_rolls_over() {
    let (env, _token, _tc, client, _accounts, subscription_id) =
        setup_with_quotas!(calls_and_bytes);
    let symbol = Symbol::new(&env, CALLS);
    client.record_usage(&subscription_id, &symbol, &12_000);

    env.ledger().set_timestamp(START + PERIOD);
    client.charge(&subscription_id);

    // The new period has no usage, so the terms can be changed for it.
    let repriced = Vec::from_slice(&env, &[quota(&env, CALLS, 0, 1, 1, None)]);
    client.set_quotas(&subscription_id, &repriced);
    assert_eq!(client.get_subscription(&subscription_id).quotas, repriced);
}

#[test]
fn set_quotas_is_rejected_when_the_subscription_is_not_active() {
    let (env, _token, _tc, client, _accounts, subscription_id) = setup!();
    let quotas = calls_and_bytes(&env);

    client.pause(&subscription_id);
    assert_eq!(
        client
            .try_set_quotas(&subscription_id, &quotas)
            .unwrap_err()
            .unwrap(),
        ForgeError::InvalidInput
    );
    client.resume(&subscription_id);
    client.cancel(&subscription_id);
    assert_eq!(
        client
            .try_set_quotas(&subscription_id, &quotas)
            .unwrap_err()
            .unwrap(),
        ForgeError::InvalidInput
    );
    assert!(client.get_subscription(&subscription_id).quotas.is_empty());
}

#[test]
fn set_quotas_for_an_unknown_subscription_is_not_found() {
    let (env, _token, _tc, client, _accounts, _id) = setup!();
    let quotas = calls_and_bytes(&env);
    assert_eq!(
        client.try_set_quotas(&999, &quotas).unwrap_err().unwrap(),
        ForgeError::NotFound
    );
}

// ---------------------------------------------------------------------------
// Usage recording
// ---------------------------------------------------------------------------

#[test]
fn record_usage_accumulates_raw_units_within_the_open_period() {
    let (env, _token, _tc, client, _accounts, subscription_id) =
        setup_with_quotas!(calls_and_bytes);
    let calls = Symbol::new(&env, CALLS);
    let bytes = Symbol::new(&env, BYTES);

    client.record_usage(&subscription_id, &calls, &4_000);
    client.record_usage(&subscription_id, &calls, &500);
    client.record_usage(&subscription_id, &bytes, &1_000_000);

    // Raw units, per metric — not buckets: the derivation, not the recorder,
    // decides what a unit is worth.
    assert_eq!(client.get_usage(&subscription_id, &calls).units, 4_500);
    assert_eq!(client.get_usage(&subscription_id, &bytes).units, 1_000_000);
    assert_eq!(
        client.get_usage(&subscription_id, &calls).period_start,
        START
    );
    assert_eq!(
        client.get_usage(&subscription_id, &calls).subscription_id,
        subscription_id
    );
    // 4_500 calls are within the 10k inclusion, so only the base is due.
    assert_eq!(client.quote_period(&subscription_id), AMOUNT);
}

#[test]
fn record_usage_rejects_undeclared_metrics() {
    let (env, _token, _tc, client, _accounts, subscription_id) =
        setup_with_quotas!(calls_and_bytes);
    // The plan prices calls and bytes; nothing else may be metered.
    let err = client
        .try_record_usage(&subscription_id, &Symbol::new(&env, "requests"), &1)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(
        client
            .get_usage(&subscription_id, &Symbol::new(&env, "requests"))
            .units,
        0
    );
}

#[test]
fn record_usage_on_a_flat_subscription_is_rejected() {
    let (env, _token, _tc, client, _accounts, subscription_id) = setup!();
    // No quotas means no priced dimension, so a flat subscriber can never be
    // metered: the flow is exactly the pre-metering contract.
    let err = client
        .try_record_usage(&subscription_id, &Symbol::new(&env, CALLS), &1_000)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(client.quote_period(&subscription_id), AMOUNT);
}

#[test]
fn record_usage_rejects_zero_units() {
    let (env, _token, _tc, client, _accounts, subscription_id) =
        setup_with_quotas!(calls_and_bytes);
    let err = client
        .try_record_usage(&subscription_id, &Symbol::new(&env, CALLS), &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(
        client
            .get_usage(&subscription_id, &Symbol::new(&env, CALLS))
            .units,
        0
    );
}

#[test]
fn record_usage_for_an_unknown_subscription_is_not_found() {
    let (env, _token, _tc, client, _accounts, _id) = setup!();
    let err = client
        .try_record_usage(&999, &Symbol::new(&env, CALLS), &1)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::NotFound);
}

#[test]
fn record_usage_is_rejected_once_the_subscription_leaves_active() {
    let (env, _token, _tc, client, _accounts, subscription_id) =
        setup_with_quotas!(calls_and_bytes);
    let calls = Symbol::new(&env, CALLS);

    // Paused: nothing is served, so nothing is metered.
    client.pause(&subscription_id);
    assert_eq!(
        client
            .try_record_usage(&subscription_id, &calls, &1_000)
            .unwrap_err()
            .unwrap(),
        ForgeError::InvalidInput
    );

    // Cancelled: terminal.
    client.resume(&subscription_id);
    client.cancel(&subscription_id);
    assert_eq!(
        client
            .try_record_usage(&subscription_id, &calls, &1_000)
            .unwrap_err()
            .unwrap(),
        ForgeError::InvalidInput
    );

    // PastDue: the meter freezes at the amount that was attempted, which is
    // what makes the arrears retry identical to the attempt that failed.
    let broke = Address::generate(&env);
    StellarAssetClient::new(&env, &client.get_subscription(&subscription_id).token)
        .mint(&broke, &10_i128);
    let broke_id = client.subscribe(
        &broke,
        &client.get_subscription(&subscription_id).provider,
        &client.get_subscription(&subscription_id).token,
        &AMOUNT,
        &PERIOD,
    );
    client.set_quotas(&broke_id, &calls_and_bytes(&env));
    client.record_usage(&broke_id, &calls, &1_000);
    env.ledger().set_timestamp(START + PERIOD);
    assert_eq!(client.charge(&broke_id), 0);
    assert_eq!(
        client
            .try_record_usage(&broke_id, &calls, &1_000)
            .unwrap_err()
            .unwrap(),
        ForgeError::InvalidInput
    );
    assert_eq!(client.get_usage(&broke_id, &calls).units, 1_000);
}

#[test]
fn record_usage_is_overflow_safe() {
    let (env, token, _tc, client, accounts, subscription_id) = setup_with_quotas!(calls_and_bytes);
    let calls = Symbol::new(&env, CALLS);
    // The maximal bill is enormous, so the subscriber needs to be able to pay
    // it for the settlement half of this test to mean anything.
    StellarAssetClient::new(&env, &token).mint(&accounts.user1, &1_000_000_000_000_000_000);

    // The counter is `u64`; recording past its top is an error, not a wrap
    // that would silently under-bill the period.
    client.record_usage(&subscription_id, &calls, &u64::MAX);
    let err = client
        .try_record_usage(&subscription_id, &calls, &1)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::ArithmeticOverflow);
    assert_eq!(client.get_usage(&subscription_id, &calls).units, u64::MAX);
    // The first (maximal) record is intact, so the period still settles. The
    // bucket count comes from the units *past* the inclusion, then the price
    // is per bucket: 18_446_744_073_709_542 buckets of 25, on top of the base.
    assert_eq!(
        client.quote_period(&subscription_id),
        461_168_601_842_738_800
    );
    env.ledger().set_timestamp(START + PERIOD);
    let charged = client.charge(&subscription_id);
    assert_eq!(charged, 461_168_601_842_738_800);
    assert_eq!(
        token_balance(&client, &client.get_subscription(&subscription_id).provider),
        charged
    );
}

#[test]
fn record_usage_past_a_clamped_cap_is_still_metered_verbatim() {
    let (env, _token, _tc, client, _accounts, subscription_id) =
        setup_with_quotas!(calls_and_bytes);
    let bytes = Symbol::new(&env, BYTES);
    client.record_usage(&subscription_id, &bytes, &6_000_000);

    // The meter is the honest unit count; the cap only clamps the bill.
    assert_eq!(client.get_usage(&subscription_id, &bytes).units, 6_000_000);
    assert_eq!(client.quote_period(&subscription_id), AMOUNT + 500);
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[test]
fn usage_recorded_event_carries_the_running_period_total() {
    let (env, _token, _tc, client, _accounts, subscription_id) =
        setup_with_quotas!(calls_and_bytes);
    let calls = Symbol::new(&env, CALLS);
    let topics = std::vec![
        ScVal::U64(subscription_id),
        ScVal::Symbol(ScSymbol::try_from(CALLS).unwrap()),
    ];

    // An indexer rebuilds a period's overage from these events alone, so each
    // must carry both the increment and the running total for the window. The
    // event log holds the latest invocation, hence one assertion per call.
    client.record_usage(&subscription_id, &calls, &4_000);
    let first = events_named(&env, &client.address, "usage_recorded");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, topics);
    assert_eq!(data_field(&first[0].1, "units"), ScVal::U64(4_000));
    assert_eq!(data_field(&first[0].1, "period_units"), ScVal::U64(4_000));
    assert_eq!(data_field(&first[0].1, "period_start"), ScVal::U64(START));

    client.record_usage(&subscription_id, &calls, &500);
    let second = events_named(&env, &client.address, "usage_recorded");
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].0, topics);
    assert_eq!(
        data_field(&second[0].1, "units"),
        ScVal::U64(500),
        "the increment, not the total"
    );
    assert_eq!(data_field(&second[0].1, "period_units"), ScVal::U64(4_500));
    assert_eq!(data_field(&second[0].1, "period_start"), ScVal::U64(START));
}

#[test]
fn charged_event_reports_the_derived_amount_not_the_base() {
    let (env, _token, _tc, client, _accounts, subscription_id) =
        setup_with_quotas!(calls_and_bytes);
    client.record_usage(&subscription_id, &Symbol::new(&env, CALLS), &11_001);

    // 11_001 calls is 1_001 past the 10k inclusion, so two started 1k buckets
    // at 25 each, on top of the base.
    let expected = AMOUNT + 50;
    assert_eq!(client.quote_period(&subscription_id), expected);
    env.ledger().set_timestamp(START + PERIOD);
    assert_eq!(client.charge(&subscription_id), expected);

    let charged = events_named(&env, &client.address, "charged");
    assert_eq!(charged.len(), 1);
    assert_eq!(
        charged[0].0,
        std::vec![ScVal::U64(subscription_id)],
        "the subscription is the only topic"
    );
    assert_eq!(
        data_field(&charged[0].1, "amount"),
        sc_i128(expected),
        "the event must price the period, not just the base"
    );
    assert_eq!(
        data_field(&charged[0].1, "last_charged"),
        ScVal::U64(START + PERIOD)
    );
}

#[test]
fn quotas_set_event_publishes_the_declared_terms() {
    let (env, _token, _tc, client, _accounts, subscription_id) = setup!();
    client.set_quotas(&subscription_id, &calls_and_bytes(&env));

    let declared = events_named(&env, &client.address, "quotas_set");
    assert_eq!(declared.len(), 1);
    assert_eq!(declared[0].0, std::vec![ScVal::U64(subscription_id)]);

    // The whole plan travels with the event, so an indexer learns the terms it
    // needs to price overage without replaying the declaration.
    let published = data_field(&declared[0].1, "quotas");
    let ScVal::Vec(Some(metrics)) = &published else {
        panic!("`quotas` is not a vec: {published:?}");
    };
    assert_eq!(metrics.0.len(), 2);
    let calls = &metrics.0[0];
    assert_eq!(
        data_field(calls, "metric"),
        ScVal::Symbol(ScSymbol::try_from(CALLS).unwrap())
    );
    assert_eq!(data_field(calls, "included_units"), ScVal::U64(10_000));
    assert_eq!(data_field(calls, "overage_price"), sc_i128(25));
    assert_eq!(data_field(calls, "bucket_units"), ScVal::U64(1_000));
    assert_eq!(
        data_field(calls, "max_overage_units"),
        ScVal::Void,
        "an absent cap is published as `None`"
    );
    let bytes = &metrics.0[1];
    assert_eq!(
        data_field(bytes, "metric"),
        ScVal::Symbol(ScSymbol::try_from(BYTES).unwrap())
    );
    assert_eq!(data_field(bytes, "included_units"), ScVal::U64(1_000_000));
    assert_eq!(data_field(bytes, "overage_price"), sc_i128(10));
    assert_eq!(data_field(bytes, "bucket_units"), ScVal::U64(100_000));
    assert_eq!(
        data_field(bytes, "max_overage_units"),
        ScVal::U64(5_000_000)
    );
}

#[test]
fn a_failed_settlement_publishes_no_metering_events() {
    let (env, token, _tc, client, accounts, _subscription_id) = setup!();
    // The subscriber holds exactly the base, so any overage makes the transfer
    // fail and roll the whole invocation back.
    let broke = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&broke, &AMOUNT);
    let broke_id = client.subscribe(&broke, &accounts.validator, &token, &AMOUNT, &PERIOD);
    client.set_quotas(&broke_id, &calls_and_bytes(&env));
    client.record_usage(&broke_id, &Symbol::new(&env, CALLS), &11_001);

    env.ledger().set_timestamp(START + PERIOD);
    assert_eq!(client.charge(&broke_id), 0);

    // Nothing was published, and every meter survived the attempt, so the
    // arrears retry re-derives the identical bill.
    assert!(events_named(&env, &client.address, "charged").is_empty());
    let calls = Symbol::new(&env, CALLS);
    assert_eq!(client.get_usage(&broke_id, &calls).units, 11_001);
    assert_eq!(client.quote_period(&broke_id), AMOUNT + 50);
}

/// Every event this contract published under `name` in the latest invocation,
/// as its remaining topics and its raw XDR data — the shape an indexer
/// consumes, minus the name.
fn events_named(
    env: &Env,
    contract: &Address,
    name: &str,
) -> std::vec::Vec<(std::vec::Vec<ScVal>, ScVal)> {
    let name = ScVal::Symbol(ScSymbol::try_from(name).unwrap());
    env.events()
        .all()
        .filter_by_contract(contract)
        .events()
        .iter()
        .filter_map(|event| {
            let ContractEventBody::V0(body) = &event.body;
            if body.topics.first() != Some(&name) {
                return None;
            }
            Some((body.topics[1..].to_vec(), body.data.clone()))
        })
        .collect()
}

/// Read one field out of a contract event's data, which the SDK encodes as an
/// `ScVal::Map` keyed by the field names as symbols.
fn data_field(data: &ScVal, field: &str) -> ScVal {
    let ScVal::Map(Some(entries)) = data else {
        panic!("event data is not a map: {data:?}");
    };
    let key = ScVal::Symbol(ScSymbol::try_from(field).unwrap());
    entries
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.val.clone())
        .unwrap_or_else(|| panic!("event data has no `{field}` field: {data:?}"))
}

/// An `i128` in the XDR form the wire uses, which is a 128-bit split pair
/// rather than a single scalar.
fn sc_i128(value: i128) -> ScVal {
    ScVal::I128(Int128Parts {
        hi: (value >> 64) as i64,
        lo: value as u64,
    })
}

fn token_balance(client: &SorobanForgeSubscriptionPaymentsClient<'_>, address: &Address) -> i128 {
    soroban_sdk::token::Client::new(&client.env, &token_address(client)).balance(address)
}

fn token_address(client: &SorobanForgeSubscriptionPaymentsClient<'_>) -> Address {
    client.get_subscription(&SUB_ID).token
}
