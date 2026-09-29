//! Tests for the plan registry entrypoints added in issue #116.
//!
//! Coverage matrix
//! ───────────────
//! `create_plan`
//!   - happy path: fields stored correctly, flat and with quotas
//!   - sequential ids starting at 1
//!   - rejects amount ≤ 0
//!   - rejects period == 0
//!   - rejects quota with zero bucket_units
//!   - rejects quota with negative overage_price
//!   - rejects duplicate metrics in quotas list
//!   - requires provider authorization
//!
//! `plan_count`
//!   - zero before any plan
//!   - increments per `create_plan`, not per `subscribe`
//!
//! `get_plan`
//!   - returns stored plan
//!   - NotFound for unknown id
//!   - NotFound for id 0
//!
//! `subscribe_to_plan`
//!   - creates Active subscription with plan's exact terms
//!   - copies plan quotas into subscription
//!   - subscription id is sequential across all creation paths
//!   - multiple subscriptions to same plan are distinct records
//!   - populates subscriber and provider indexes
//!   - NotFound for unknown plan id
//!   - requires subscriber authorization
//!
//! Plan ids and subscription ids are from independent counters
//! Existing `subscribe` (explicit terms) coexists unchanged

use super::*;
use soroban_forge_test_utils::TestAccounts;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env};

const START: u64 = 1_000_000;
const PERIOD: u64 = 1_000;
const AMOUNT: i128 = 500;

/// Minimal harness: env + registered contract + SAC token + named accounts.
/// No subscription is created up-front; plan tests build their own state.
macro_rules! setup {
    () => {{
        let env = Env::default();
        env.mock_all_auths_allowing_non_root_auth();
        env.ledger().set_timestamp(START);

        let admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(admin);
        let token = sac.address();
        let token_admin = StellarAssetClient::new(&env, &token);

        let contract_id = env.register(SubscriptionPayments, ());
        let client = SorobanForgeSubscriptionPaymentsClient::new(&env, &contract_id);
        let accounts = TestAccounts::generate(&env);

        token_admin.mint(&accounts.user1, &100_000_i128);

        (env, token, token_admin, contract_id, client, accounts)
    }};
}

// ── create_plan happy path ────────────────────────────────────────────────────

#[test]
fn create_plan_returns_sequential_ids_starting_at_one() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();

    let id1 = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );
    let id2 = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );
    let id3 = client.create_plan(
        &accounts.arbiter,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );

    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);
}

#[test]
fn create_plan_stores_all_fields_correctly() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    let id = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );

    let plan = client.get_plan(&id);
    assert_eq!(plan.plan_id, id);
    assert_eq!(plan.provider, accounts.validator);
    assert_eq!(plan.token, token);
    assert_eq!(plan.amount, AMOUNT);
    assert_eq!(plan.period, PERIOD);
    assert!(plan.quotas.is_empty());
}

#[test]
fn create_plan_with_quotas_stores_them() {
    let (env, token, _ta, _cid, client, accounts) = setup!();
    let metric = soroban_sdk::symbol_short!("api");
    let quota = MetricQuota {
        metric: metric.clone(),
        included_units: 1_000,
        overage_price: 5,
        bucket_units: 100,
        max_overage_units: Some(10_000),
    };
    let mut quotas = Vec::new(&env);
    quotas.push_back(quota);

    let id = client.create_plan(&accounts.validator, &token, &AMOUNT, &PERIOD, &quotas);
    let plan = client.get_plan(&id);

    assert_eq!(plan.quotas.len(), 1);
    let sq = plan.quotas.get_unchecked(0);
    assert_eq!(sq.metric, metric);
    assert_eq!(sq.included_units, 1_000);
    assert_eq!(sq.overage_price, 5);
    assert_eq!(sq.bucket_units, 100);
    assert_eq!(sq.max_overage_units, Some(10_000));
}

// ── create_plan validation (checked before auth) ──────────────────────────────

#[test]
fn create_plan_rejects_zero_amount() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    let err = client
        .try_create_plan(
            &accounts.validator,
            &token,
            &0_i128,
            &PERIOD,
            &Vec::new(&client.env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    // No partial record written.
    assert_eq!(client.plan_count(), 0);
}

#[test]
fn create_plan_rejects_negative_amount() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    let err = client
        .try_create_plan(
            &accounts.validator,
            &token,
            &(-100_i128),
            &PERIOD,
            &Vec::new(&client.env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(client.plan_count(), 0);
}

#[test]
fn create_plan_rejects_zero_period() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    let err = client
        .try_create_plan(
            &accounts.validator,
            &token,
            &AMOUNT,
            &0_u64,
            &Vec::new(&client.env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(client.plan_count(), 0);
}

#[test]
fn create_plan_rejects_quota_with_zero_bucket_units() {
    let (env, token, _ta, _cid, client, accounts) = setup!();
    let bad_quota = MetricQuota {
        metric: soroban_sdk::symbol_short!("calls"),
        included_units: 0,
        overage_price: 1,
        bucket_units: 0, // invalid
        max_overage_units: None,
    };
    let mut quotas = Vec::new(&env);
    quotas.push_back(bad_quota);
    let err = client
        .try_create_plan(&accounts.validator, &token, &AMOUNT, &PERIOD, &quotas)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(client.plan_count(), 0);
}

#[test]
fn create_plan_rejects_negative_overage_price() {
    let (env, token, _ta, _cid, client, accounts) = setup!();
    let bad_quota = MetricQuota {
        metric: soroban_sdk::symbol_short!("calls"),
        included_units: 0,
        overage_price: -1, // invalid
        bucket_units: 100,
        max_overage_units: None,
    };
    let mut quotas = Vec::new(&env);
    quotas.push_back(bad_quota);
    let err = client
        .try_create_plan(&accounts.validator, &token, &AMOUNT, &PERIOD, &quotas)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
}

#[test]
fn create_plan_rejects_duplicate_metrics_in_quotas() {
    let (env, token, _ta, _cid, client, accounts) = setup!();
    let q = MetricQuota {
        metric: soroban_sdk::symbol_short!("calls"),
        included_units: 0,
        overage_price: 1,
        bucket_units: 1,
        max_overage_units: None,
    };
    let mut quotas = Vec::new(&env);
    quotas.push_back(q.clone());
    quotas.push_back(q); // duplicate metric
    let err = client
        .try_create_plan(&accounts.validator, &token, &AMOUNT, &PERIOD, &quotas)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
}

#[test]
fn create_plan_requires_provider_auth() {
    // Use a completely fresh env with no mocked auths so require_auth is enforced.
    let env = Env::default();
    env.ledger().set_timestamp(START);
    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin);
    let token = sac.address();
    let contract_id = env.register(SubscriptionPayments, ());
    let client = SorobanForgeSubscriptionPaymentsClient::new(&env, &contract_id);
    let provider = Address::generate(&env);

    // No auths mocked — the host will reject the call when require_auth fires.
    let result = client.try_create_plan(&provider, &token, &AMOUNT, &PERIOD, &Vec::new(&env));
    assert!(result.is_err());
}

// ── plan_count ────────────────────────────────────────────────────────────────

#[test]
fn plan_count_starts_at_zero() {
    let (_env, _token, _ta, _cid, client, _accounts) = setup!();
    assert_eq!(client.plan_count(), 0);
}

#[test]
fn plan_count_increments_per_create_plan() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );
    assert_eq!(client.plan_count(), 1);
    client.create_plan(
        &accounts.arbiter,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );
    assert_eq!(client.plan_count(), 2);
}

#[test]
fn plan_count_not_affected_by_subscribe() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    client.subscribe(
        &accounts.user1,
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
    );
    assert_eq!(client.plan_count(), 0);
    assert_eq!(client.get_subscription_count(), 1);
}

// ── get_plan error paths ──────────────────────────────────────────────────────

#[test]
fn get_plan_returns_not_found_for_unknown_id() {
    let (_env, _token, _ta, _cid, client, _accounts) = setup!();
    let err = client.try_get_plan(&999).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::NotFound);
}

#[test]
fn get_plan_returns_not_found_for_id_zero() {
    let (_env, _token, _ta, _cid, client, _accounts) = setup!();
    let err = client.try_get_plan(&0).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::NotFound);
}

// ── plan ids and subscription ids are independent counters ────────────────────

#[test]
fn plan_ids_and_subscription_ids_are_from_separate_counters() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    // Create two plans first — plan ids 1 and 2.
    let plan_id1 = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );
    let plan_id2 = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );
    assert_eq!(plan_id1, 1);
    assert_eq!(plan_id2, 2);

    // Subscription ids start their own counter at 1.
    let sub_id1 =
        client.subscribe(&accounts.user1, &accounts.validator, &token, &AMOUNT, &PERIOD);
    let sub_id2 = client.subscribe_to_plan(&plan_id1, &accounts.user1);
    assert_eq!(sub_id1, 1);
    assert_eq!(sub_id2, 2);

    // Both counters tracked independently.
    assert_eq!(client.plan_count(), 2);
    assert_eq!(client.get_subscription_count(), 2);
}

// ── subscribe_to_plan happy path ─────────────────────────────────────────────

#[test]
fn subscribe_to_plan_creates_active_subscription_with_plan_terms() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    let plan_id = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );
    let sub_id = client.subscribe_to_plan(&plan_id, &accounts.user1);

    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.subscriber, accounts.user1);
    assert_eq!(sub.provider, accounts.validator);
    assert_eq!(sub.token, token);
    assert_eq!(sub.amount, AMOUNT);
    assert_eq!(sub.period, PERIOD);
    assert_eq!(sub.status, SubscriptionStatus::Active);
    assert_eq!(sub.last_charged, START);
    assert_eq!(sub.paused_at, None);
    assert_eq!(sub.failed_attempts, 0);
    assert!(sub.quotas.is_empty());
}

#[test]
fn subscribe_to_plan_copies_quotas_verbatim() {
    let (env, token, _ta, _cid, client, accounts) = setup!();
    let metric = soroban_sdk::symbol_short!("bytes");
    let quota = MetricQuota {
        metric: metric.clone(),
        included_units: 500,
        overage_price: 10,
        bucket_units: 50,
        max_overage_units: Some(5_000),
    };
    let mut quotas = Vec::new(&env);
    quotas.push_back(quota);

    let plan_id = client.create_plan(&accounts.validator, &token, &AMOUNT, &PERIOD, &quotas);
    let sub_id = client.subscribe_to_plan(&plan_id, &accounts.user1);

    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.quotas.len(), 1);
    let sq = sub.quotas.get_unchecked(0);
    assert_eq!(sq.metric, metric);
    assert_eq!(sq.included_units, 500);
    assert_eq!(sq.overage_price, 10);
    assert_eq!(sq.bucket_units, 50);
    assert_eq!(sq.max_overage_units, Some(5_000));
}

#[test]
fn subscribe_to_plan_subscription_id_is_sequential_across_all_paths() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    let plan_id = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );

    let id1 = client.subscribe(&accounts.user1, &accounts.validator, &token, &AMOUNT, &PERIOD);
    let id2 = client.subscribe_to_plan(&plan_id, &accounts.user2);
    let id3 = client.subscribe_to_plan(&plan_id, &accounts.user3);
    let id4 = client.subscribe(&accounts.user1, &accounts.arbiter, &token, &AMOUNT, &PERIOD);

    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);
    assert_eq!(id4, 4);
    assert_eq!(client.get_subscription_count(), 4);
}

#[test]
fn multiple_subscriptions_to_same_plan_are_distinct_records() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    let plan_id = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );

    // Same subscriber, same plan — two distinct subscriptions.
    let sub_id1 = client.subscribe_to_plan(&plan_id, &accounts.user1);
    let sub_id2 = client.subscribe_to_plan(&plan_id, &accounts.user1);

    assert_ne!(sub_id1, sub_id2);

    let sub1 = client.get_subscription(&sub_id1);
    let sub2 = client.get_subscription(&sub_id2);
    assert_ne!(sub1.subscription_id, sub2.subscription_id);

    // Both carry identical plan terms.
    assert_eq!(sub1.amount, sub2.amount);
    assert_eq!(sub1.period, sub2.period);
    assert_eq!(sub1.provider, sub2.provider);
    assert_eq!(sub1.token, sub2.token);

    // Both appear in the subscriber index.
    assert_eq!(
        client
            .subscriptions_for_subscriber(&accounts.user1, &0, &10)
            .len(),
        2
    );

    // Both appear in the provider index.
    assert_eq!(
        client
            .subscriptions_for_provider(&accounts.validator, &0, &10)
            .len(),
        2
    );
}

#[test]
fn subscribe_to_plan_populates_both_indexes() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    let plan_id = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );
    client.subscribe_to_plan(&plan_id, &accounts.user1);
    client.subscribe_to_plan(&plan_id, &accounts.user2);

    assert_eq!(
        client
            .subscriptions_for_provider(&accounts.validator, &0, &10)
            .len(),
        2
    );
    assert_eq!(
        client
            .subscriptions_for_subscriber(&accounts.user1, &0, &10)
            .len(),
        1
    );
    assert_eq!(
        client
            .subscriptions_for_subscriber(&accounts.user2, &0, &10)
            .len(),
        1
    );
}

// ── subscribe_to_plan error paths ────────────────────────────────────────────

#[test]
fn subscribe_to_plan_returns_not_found_for_unknown_plan_id() {
    let (_env, _token, _ta, _cid, client, accounts) = setup!();
    let err = client
        .try_subscribe_to_plan(&999, &accounts.user1)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::NotFound);
    assert_eq!(client.get_subscription_count(), 0);
}

#[test]
fn subscribe_to_plan_requires_subscriber_auth() {
    // Use a completely fresh env with no mocked auths so require_auth is enforced.
    let env = Env::default();
    env.ledger().set_timestamp(START);
    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin);
    let token = sac.address();
    let contract_id = env.register(SubscriptionPayments, ());
    let provider = Address::generate(&env);
    let subscriber = Address::generate(&env);

    // Create the plan with mocked auths on the same env (mock_all affects env, not per-call).
    env.mock_all_auths_allowing_non_root_auth();
    let client = SorobanForgeSubscriptionPaymentsClient::new(&env, &contract_id);
    let plan_id =
        client.create_plan(&provider, &token, &AMOUNT, &PERIOD, &Vec::new(&env));

    // Create a brand new env with no mocking — only a plan lookup, then require_auth.
    let env2 = Env::default();
    env2.ledger().set_timestamp(START);
    // Re-register in the new env to get a clean contract.
    let admin2 = Address::generate(&env2);
    let sac2 = env2.register_stellar_asset_contract_v2(admin2);
    let token2 = sac2.address();
    let contract_id2 = env2.register(SubscriptionPayments, ());
    let provider2 = Address::generate(&env2);
    let subscriber2 = Address::generate(&env2);
    // Create the plan in env2 with mocking...
    env2.mock_all_auths_allowing_non_root_auth();
    let client2 = SorobanForgeSubscriptionPaymentsClient::new(&env2, &contract_id2);
    let plan_id2 =
        client2.create_plan(&provider2, &token2, &AMOUNT, &PERIOD, &Vec::new(&env2));
    // Now try subscribe_to_plan with an explicit auth that doesn't include subscriber2.
    // We use env2 mock_all so this will succeed — rewrite the test to assert the
    // authorization tree contains the subscriber.
    let _ = plan_id;
    let _ = plan_id2;
    let _ = subscriber;
    let _ = subscriber2;

    // Verify that subscribe_to_plan records the subscriber in the auth tree.
    let sub_id = client2.subscribe_to_plan(&plan_id2, &subscriber2);
    let auths = env2.auths();
    // At least one authorization must be for subscriber2.
    let subscriber_authorized = auths.iter().any(|(addr, _)| *addr == subscriber2);
    assert!(subscriber_authorized, "subscribe_to_plan must require subscriber auth");
    let sub = client2.get_subscription(&sub_id);
    assert_eq!(sub.subscriber, subscriber2);
}

// ── subscription from plan can be charged ────────────────────────────────────

#[test]
fn subscription_from_plan_can_be_charged_after_period() {
    let (env, token, _ta, _cid, client, accounts) = setup!();
    let plan_id = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );
    let sub_id = client.subscribe_to_plan(&plan_id, &accounts.user1);

    env.ledger().set_timestamp(START + PERIOD);
    let billed = client.charge(&sub_id);
    assert_eq!(billed, AMOUNT);

    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.last_charged, START + PERIOD);
    assert_eq!(sub.status, SubscriptionStatus::Active);
}

#[test]
fn subscription_from_plan_charge_before_period_returns_zero() {
    let (env, token, _ta, _cid, client, accounts) = setup!();
    let plan_id = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );
    let sub_id = client.subscribe_to_plan(&plan_id, &accounts.user1);

    env.ledger().set_timestamp(START + PERIOD - 1);
    assert_eq!(client.charge(&sub_id), 0);
    assert_eq!(client.get_subscription(&sub_id).last_charged, START);
}

// ── terms match plan exactly ─────────────────────────────────────────────────

#[test]
fn subscription_terms_match_plan_terms_exactly() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    let plan_id = client.create_plan(
        &accounts.validator,
        &token,
        &12_345_i128,
        &7_200_u64,
        &Vec::new(&client.env),
    );
    let plan = client.get_plan(&plan_id);
    let sub_id = client.subscribe_to_plan(&plan_id, &accounts.user1);
    let sub = client.get_subscription(&sub_id);

    assert_eq!(sub.provider, plan.provider);
    assert_eq!(sub.token, plan.token);
    assert_eq!(sub.amount, plan.amount);
    assert_eq!(sub.period, plan.period);
    assert_eq!(sub.quotas.len(), plan.quotas.len());
}

// ── existing subscribe coexists unchanged ────────────────────────────────────

#[test]
fn explicit_subscribe_still_works_alongside_plans() {
    let (_env, token, _ta, _cid, client, accounts) = setup!();
    let plan_id = client.create_plan(
        &accounts.validator,
        &token,
        &AMOUNT,
        &PERIOD,
        &Vec::new(&client.env),
    );

    let sub_via_plan =
        client.subscribe_to_plan(&plan_id, &accounts.user1);
    let sub_explicit =
        client.subscribe(&accounts.user2, &accounts.validator, &token, &50_i128, &500_u64);

    let sp = client.get_subscription(&sub_via_plan);
    let se = client.get_subscription(&sub_explicit);
    assert_eq!(sp.amount, AMOUNT);
    assert_eq!(sp.period, PERIOD);
    assert_eq!(se.amount, 50);
    assert_eq!(se.period, 500);
    assert_eq!(client.get_subscription_count(), 2);
}
