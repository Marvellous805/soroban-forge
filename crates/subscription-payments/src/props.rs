//! Randomized invariant suite (proptest) for subscription payments.
//!
//! Exercised properties:
//! 1. Monotonicity: `last_charged` is monotonic and advances by exactly one period
//!    per successful charge.
//! 2. No early bill: `charge` returns `0` and changes nothing before a full period
//!    has elapsed.
//! 3. Terminal safety: a `Cancelled` subscription never bills.

use crate::{
    SorobanForgeSubscriptionPaymentsClient, SubscriptionPayments, SubscriptionStatus,
    MAX_CATCHUP_PERIODS, MAX_RETRIES,
};
use proptest::prelude::*;
use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env};

const START: u64 = 1_000_000;
const MAX_AMOUNT: i128 = 1_000_000_000_000;
const MAX_PERIOD: u64 = 10 * 365 * 24 * 3600;

struct World {
    env: Env,
    token: Address,
    contract_id: Address,
    subscriber: Address,
    provider: Address,
}

fn setup_world(mint_amount: i128) -> World {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    env.ledger().set_timestamp(START);

    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin);
    let token = sac.address();

    let subscriber = Address::generate(&env);
    let provider = Address::generate(&env);

    StellarAssetClient::new(&env, &token).mint(&subscriber, &mint_amount);

    let contract_id = env.register(SubscriptionPayments, ());

    World {
        env,
        token,
        contract_id,
        subscriber,
        provider,
    }
}

impl World {
    fn client(&self) -> SorobanForgeSubscriptionPaymentsClient<'_> {
        SorobanForgeSubscriptionPaymentsClient::new(&self.env, &self.contract_id)
    }

    fn subscribe(&self, amount: i128, period: u64) -> u64 {
        self.client().subscribe(
            &self.subscriber,
            &self.provider,
            &self.token,
            &amount,
            &period,
        )
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn prop_last_charged_is_monotonic_and_advances_by_one_period(
        amount in 1i128..=100_000_i128,
        period in 1u64..=MAX_PERIOD,
        charges_count in 1u32..=10_u32,
    ) {
        let mint_total = amount.saturating_mul(charges_count as i128);
        let w = setup_world(mint_total);
        let id = w.subscribe(amount, period);

        let mut prev_last_charged = START;

        for i in 1..=charges_count {
            let target_time = START + period * (i as u64);
            w.env.ledger().set_timestamp(target_time);

            let billed = w.client().charge(&id);
            prop_assert_eq!(billed, amount);

            let sub = w.client().get_subscription(&id);
            prop_assert!(sub.last_charged > prev_last_charged, "last_charged must be strictly monotonic");
            prop_assert_eq!(sub.last_charged, START + period * (i as u64), "last_charged must advance by one period per charge");
            prev_last_charged = sub.last_charged;
        }
    }

    #[test]
    fn prop_charge_returns_zero_before_period_elapses(
        amount in 1i128..=MAX_AMOUNT,
        period in 2u64..=MAX_PERIOD,
        elapsed_delta in 1u64..=MAX_PERIOD,
    ) {
        let delta = elapsed_delta % period;
        if delta == 0 {
            return Ok(());
        }

        let w = setup_world(amount * 10);
        let id = w.subscribe(amount, period);

        w.env.ledger().set_timestamp(START + delta);

        let billed = w.client().charge(&id);
        prop_assert_eq!(billed, 0, "charge must return 0 before period elapses");

        let sub = w.client().get_subscription(&id);
        prop_assert_eq!(sub.last_charged, START, "last_charged must remain unchanged when charging early");
    }

    #[test]
    fn prop_cancelled_subscription_never_bills(
        amount in 1i128..=MAX_AMOUNT,
        period in 1u64..=MAX_PERIOD,
        cancel_delay in 0u64..=MAX_PERIOD,
        charge_delay in 1u64..=MAX_PERIOD,
    ) {
        let w = setup_world(amount * 10);
        let id = w.subscribe(amount, period);

        w.env.ledger().set_timestamp(START + cancel_delay);
        w.client().cancel(&id);

        let sub_after_cancel = w.client().get_subscription(&id);
        prop_assert_eq!(sub_after_cancel.status, SubscriptionStatus::Cancelled);

        w.env.ledger().set_timestamp(START + cancel_delay + charge_delay);
        let res = w.client().try_charge(&id);
        prop_assert!(res.is_err(), "charge on a cancelled subscription must return an error");
        let err = res.unwrap_err().unwrap();
        prop_assert_eq!(err, ForgeError::InvalidInput, "cancelled subscription must fail with InvalidInput");

        let sub_after_failed_charge = w.client().get_subscription(&id);
        prop_assert_eq!(sub_after_failed_charge.status, SubscriptionStatus::Cancelled);
    }

    #[test]
    fn prop_failed_charges_advance_retry_state_without_moving_balances(
        amount in 1i128..=100_000_i128,
        balance_percent in 0u32..100_u32,
        period in 1u64..=MAX_PERIOD,
        failure_count in 1u32..=MAX_RETRIES,
    ) {
        let initial_balance = amount * balance_percent as i128 / 100;
        let w = setup_world(initial_balance);
        let id = w.subscribe(amount, period);
        let token_client = StellarAssetClient::new(&w.env, &w.token);
        let provider_balance = token_client.balance(&w.provider);
        w.env.ledger().set_timestamp(START + period);

        let mut previous_failed_attempts = 0;
        for failure in 1..=failure_count {
            prop_assert_eq!(w.client().charge(&id), 0);

            let sub = w.client().get_subscription(&id);
            let status = sub.status.clone();
            prop_assert!(sub.failed_attempts >= previous_failed_attempts);
            prop_assert_eq!(sub.failed_attempts, failure);
            prop_assert_eq!(
                status.clone(),
                if failure == MAX_RETRIES {
                    SubscriptionStatus::Cancelled
                } else {
                    SubscriptionStatus::PastDue
                }
            );
            prop_assert_eq!(sub.last_charged, START);
            prop_assert_eq!(token_client.balance(&w.subscriber), initial_balance);
            prop_assert_eq!(token_client.balance(&w.provider), provider_balance);
            previous_failed_attempts = sub.failed_attempts;

            if status == SubscriptionStatus::PastDue {
                let before = sub;
                let catchup = w.client().try_charge_catchup(&id, &1);
                let err = catchup.unwrap_err().unwrap();
                prop_assert_eq!(err, ForgeError::InvalidInput);
                prop_assert_eq!(w.client().get_subscription(&id), before);
            }
        }
    }

    #[test]
    fn prop_catchup_bills_only_due_periods_within_the_hard_cap(
        amount in 1i128..=100_000_i128,
        period in 1u64..=MAX_PERIOD,
        elapsed_periods in 0u32..=(MAX_CATCHUP_PERIODS * 2),
        elapsed_remainder in 0u64..=MAX_PERIOD,
        max_periods in 0u32..=(MAX_CATCHUP_PERIODS * 2),
    ) {
        let elapsed_seconds = period * elapsed_periods as u64 + elapsed_remainder % period;
        if elapsed_seconds == 0 {
            return Ok(());
        }

        let mint_amount = amount * MAX_CATCHUP_PERIODS as i128;
        let w = setup_world(mint_amount);
        let id = w.subscribe(amount, period);
        let elapsed_whole_periods = (elapsed_seconds / period) as u32;
        w.env.ledger().set_timestamp(START + elapsed_seconds);
        let token_client = StellarAssetClient::new(&w.env, &w.token);
        let expected_periods = core::cmp::min(elapsed_whole_periods, max_periods);
        let before = w.client().get_subscription(&id);

        if max_periods > MAX_CATCHUP_PERIODS {
            let result = w.client().try_charge_catchup(&id, &max_periods);
            let err = result.unwrap_err().unwrap();
            prop_assert_eq!(err, ForgeError::InvalidInput);
            prop_assert_eq!(w.client().get_subscription(&id), before);
            prop_assert_eq!(token_client.balance(&w.subscriber), mint_amount);
            prop_assert_eq!(token_client.balance(&w.provider), 0);
        } else {
            let billed = w.client().charge_catchup(&id, &max_periods);
            prop_assert!(expected_periods <= elapsed_whole_periods);
            prop_assert!(expected_periods <= max_periods);
            prop_assert!(expected_periods <= MAX_CATCHUP_PERIODS);
            prop_assert_eq!(billed, amount * expected_periods as i128);

            let after = w.client().get_subscription(&id);
            prop_assert_eq!(
                after.last_charged,
                START + period * expected_periods as u64
            );
            prop_assert_eq!(token_client.balance(&w.subscriber), mint_amount - billed);
            prop_assert_eq!(token_client.balance(&w.provider), billed);
        }
    }

    #[test]
    fn prop_pause_resume_conserves_due_date_and_never_charges_while_paused(
        amount in 1i128..=100_000_i128,
        period in 10_000u64..=MAX_PERIOD,
        pause_duration in 1u64..=MAX_PERIOD,
        between_cycles in 0u64..=100_u64,
        cycles in 1u32..=10_u32,
    ) {
        let mint_amount = amount * (cycles as i128 + 1);
        let w = setup_world(mint_amount);
        let id = w.subscribe(amount, period);
        let token_client = StellarAssetClient::new(&w.env, &w.token);
        let mut now = START;
        let mut expected_last_charged = START;

        for _ in 0..cycles {
            w.env.ledger().set_timestamp(now);
            w.client().pause(&id);
            let balance_before = token_client.balance(&w.subscriber);

            now += pause_duration;
            w.env.ledger().set_timestamp(now);
            let result = w.client().try_charge(&id);
            prop_assert!(result.is_err(), "charge must fail while paused");
            prop_assert_eq!(result.unwrap_err().unwrap(), ForgeError::InvalidInput);
            let paused = w.client().get_subscription(&id);
            prop_assert_eq!(paused.status, SubscriptionStatus::Paused);
            prop_assert_eq!(paused.last_charged, expected_last_charged);
            prop_assert_eq!(token_client.balance(&w.subscriber), balance_before);

            w.client().resume(&id);
            expected_last_charged += pause_duration;
            let resumed = w.client().get_subscription(&id);
            prop_assert_eq!(resumed.status, SubscriptionStatus::Active);
            prop_assert_eq!(resumed.last_charged, expected_last_charged);
            now += between_cycles;
        }

        let next_due = expected_last_charged + period;
        prop_assert!(now < next_due);
        w.env.ledger().set_timestamp(now);
        prop_assert_eq!(w.client().charge(&id), 0);
        prop_assert_eq!(w.client().get_subscription(&id).last_charged, expected_last_charged);

        w.env.ledger().set_timestamp(next_due);
        prop_assert_eq!(w.client().charge(&id), amount);
        prop_assert_eq!(w.client().get_subscription(&id).last_charged, next_due);
    }
}
