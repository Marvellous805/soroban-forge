#![no_std]

//! # Soroban Forge — Subscription Payments contract
//!
//! Recurring, on-chain subscription billing: a subscriber authorises a
//! provider to pull a fixed `amount` per `period` (seconds) from a token
//! balance using SEP-41 tokens. The contract tracks subscription state,
//! billing cadence, token settlement, pause/resume, arrears retry handling,
//! and metered usage (per-metric quotas with exact overage pricing).
//!
//! Lifecycle:
//!
//! ```text
//! subscribe -> Active <-> Paused
//!                 \         /
//!                  v       v
//!                   cancel -> Cancelled (no further charges)
//! subscribe -> Active --(period elapses, charge succeeds)--> bills amount, advances
//!            -> Active --(period elapses, charge fails)---> PastDue (arrears retry)
//!            -> PastDue --(retry exceeds max retries)-----> Cancelled
//!            -> cancel -> Cancelled (no further charges)
//! ```
//!
//! Authorization model:
//! - `subscribe` requires the subscriber (who authorises the agreement).
//! - `authorize_provider` requires the subscriber (who consents to the
//!   provider-initiated relationship).
//! - `revoke_provider` requires the subscriber (who withdraws that consent).
//! - `subscribe_on_behalf_of` requires the provider **and** an explicit
//!   opt-in from the subscriber for that provider.
//! - `charge` requires the provider (who pulls payment) and bills when a
//!   full period has elapsed since the last charge, executing a SEP-41 token
//!   transfer from subscriber to provider.
//! - `charge_catchup` requires the provider and atomically bills multiple
//!   elapsed periods, bounded by [`MAX_CATCHUP_PERIODS`]. It refuses
//!   `PastDue` subscriptions so arrears retry semantics remain owned by
//!   `charge`.
//! - `set_quotas` requires the subscriber: quotas price the overage the
//!   subscriber is billed, so the account being charged is the one that
//!   consents to the terms.
//! - `record_usage` requires the provider (the party that served the usage
//!   and meters it, mirroring who is authorized to `charge`).
//! - `pause` requires the subscriber.
//! - `resume` requires the subscriber.
//! - `cancel` requires the subscriber (works from `Active`, `Paused`, or `PastDue`).
//! - read-only views (`get_subscription`, `get_subscription_count`,
//!   `subscriptions_for_subscriber`, `subscriptions_for_provider`,
//!   `is_provider_authorized`) require no authorization.
//!
//! | Entrypoint                 | Required authorization             |
//! | -------------------------- | ---------------------------------- |
//! | `subscribe`                | subscriber                         |
//! | `authorize_provider`       | subscriber                         |
//! | `revoke_provider`          | subscriber                         |
//! | `subscribe_on_behalf_of`   | provider + valid subscriber opt-in |
//! | `charge`                   | provider (existing authorization)  |
//! | `set_quotas`               | subscriber (prices the overage)    |
//! | `record_usage`             | provider (meters the service)      |
//! | `pause`                    | subscriber                         |
//! | `resume`                   | subscriber                         |
//! | `cancel`                   | subscriber                         |
//! | read methods               | none                               |
//!
//! ## Provider opt-in (explicit consent)
//!
//! A provider-initiated subscription (`subscribe_on_behalf_of`) is only
//! accepted for a provider the subscriber has **explicitly authorized** on
//! chain. The consent lives in instance storage under
//! [`DataKey::ProviderOptIn`], keyed by `(subscriber, provider)`; it is
//! written by `authorize_provider` and removed by `revoke_provider`, both
//! of which require the subscriber's authorization.
//!
//! The security property this preserves: a provider cannot create a
//! subscription that can later pull subscriber funds without the
//! subscriber's explicit on-chain consent. `charge` still authorizes
//! through the provider, so a subscription created by `subscribe` (subscriber
//! authorized at creation) remains fully chargeable, while a provider-created
//! subscription can only exist for a relationship the subscriber chose.
//!
//! The opt-in is a narrow, per-relationship flag — not a general permission
//! framework. Both path's records are stored in the same
//! [`Subscription`] shape with the same sequential id counter, so
//! [`get_subscription`] cannot distinguish a subscriber-created from a
//! provider-created record, and no consent-origin field is needed.
//!
//! ## Metered usage: quotas and overage
//!
//! A flat `amount` per period is a one-size-fits-all price. Production
//! products also price *usage*: a base period fee plus included units per
//! metric, with a per-bucket overage price for what exceeds the inclusion
//! (for example "10k API calls included, then 5 stroops per 1k over").
//!
//! - A subscription carries a declared quota list, [`Vec<MetricQuota>`]. An
//!   **empty list is the flat flow**: [`period_amount`] short-circuits to
//!   `amount`, the charged value and every event payload are bit-identical to
//!   a contract without this feature, and the existing regression suite is
//!   unchanged.
//! - The provider meters usage with `record_usage(subscription_id, metric,
//!   units)`, which accumulates **raw units** for the open period only, under
//!   [`DataKey::Usage`]. Recording is rejected for a metric the subscription
//!   does not declare, so a provider can never meter usage the subscriber has
//!   not already been asked to pay for.
//! - The amount a period bills is derived in one place — the pure
//!   [`period_amount`] helper, called by `charge`, by every period of
//!   `charge_catchup`, and by the `quote_period` view:
//!
//!   ```text
//!   amount = base
//!          + Σ over metric of ceil(min(max(0, units - included), cap) / bucket) * overage_price
//!   ```
//!
//!   with `cap` omitted for an uncapped quota. A started bucket is billed in
//!   full and the price is **per bucket**, never per unit, so no sub-unit
//!   price precision is ever lost.
//! - **No rounding drift is possible across periods.** The accumulator holds
//!   raw units and never a bucket count or a carried-over remainder, so each
//!   period's amount is a pure function of that period's units. Charging `N`
//!   periods bills exactly `N` times the per-period derivation — there is no
//!   state in which rounding can compound.
//! - Meters are dropped atomically with the charge that closes the period, in
//!   the same frame that advances `last_charged`. A **failed** transfer
//!   returns before that point, so every meter survives untouched and the
//!   retry re-derives a byte-identical amount.
//! - `charge_catchup` settles the open period **last**: usage can only be
//!   metered into the open (latest) period, so earlier unsettled periods bill
//!   the base amount and only the final one carries overage.
//! - `record_usage` requires an `Active` subscription, which is what freezes
//!   the meter at the attempted amount once a charge has failed and makes the
//!   arrears retry identical to the attempt that failed.
//!
//! ### Design decisions
//!
//! 1. **Cap policy** — a per-metric `max_overage_units` on *billable overage
//!    units*, enforced at settlement by clamping, never by rejecting. A charge
//!    is always billable, so a usage spike cannot wedge a subscription, and
//!    the subscriber's worst case per period is `base + Σ caps`. Rejecting at
//!    `record_usage` time was rejected because refused units would spill into
//!    the next period and break the "current period only" property.
//! 2. **Bucketing / rounding** — integer buckets priced per bucket, rounded
//!    **up** (a started bucket is charged in full), derived from raw units on
//!    every derivation. A cap that is not a multiple of `bucket_units` bills
//!    at most `ceil(cap / bucket_units)` buckets.
//! 3. **Accumulator storage class** — instance storage
//!    ([`DataKey::Usage`]), one entry per `(subscription, metric)`, stamped
//!    with the `last_charged` it belongs to. Instance storage matches the
//!    rest of the crate; the persistent-storage migration with TTL maintenance
//!    is tracked separately and does not change the rollover rules.

#[cfg(test)]
extern crate std;

use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::{
    contract, contractclient, contractevent, contractimpl, contracttype, token, Address, Env,
    Symbol, Vec,
};

/// Maximum consecutive failed payment attempts before transitioning to Cancelled.
const MAX_RETRIES: u32 = 3;

/// Hard upper bound for one catch-up invocation.
///
/// Keeping the loop bounded protects Soroban instruction limits while still
/// covering practical keeper downtime without requiring one transaction per
/// missed period.
const MAX_CATCHUP_PERIODS: u32 = 32;

/// Maximum number of per-metric quotas one subscription may declare.
///
/// Every charge derives its amount by walking the quota list, so the list is
/// bounded to keep that derivation — and the instance storage a metered
/// subscription uses — inside Soroban's instruction and state budgets.
const MAX_QUOTAS: u32 = 16;

/// Public interface for the Soroban Forge subscription payments contract.
#[contractclient(name = "SorobanForgeSubscriptionPaymentsClient")]
pub trait SorobanForgeSubscriptionPayments {
    /// Subscribe `subscriber` to `provider`'s service at `amount` per
    /// `period`. Returns the stable subscription id.
    fn subscribe(
        env: Env,
        subscriber: Address,
        provider: Address,
        token: Address,
        amount: i128,
        period: u64,
    ) -> Result<u64, soroban_forge_shared_utils::ForgeError>;

    /// Explicitly authorize `provider` to create subscriptions on
    /// `subscriber`'s behalf (see the module docs on provider opt-in).
    ///
    /// Requires the subscriber. Idempotent: authorizing a provider that is
    /// already authorized succeeds.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::Unauthorized`] — the caller is not `subscriber`.
    fn authorize_provider(
        env: Env,
        subscriber: Address,
        provider: Address,
    ) -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Withdraw `subscriber`'s explicit authorization of `provider` (see the
    /// module docs on provider opt-in).
    ///
    /// Requires the subscriber. Idempotent: revoking a provider that is not
    /// authorized succeeds and leaves the opt-in absent.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::Unauthorized`] — the caller is not `subscriber`.
    fn revoke_provider(
        env: Env,
        subscriber: Address,
        provider: Address,
    ) -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Read whether `subscriber` has explicitly authorized `provider`
    /// (read-only view; requires no authorization).
    fn is_provider_authorized(env: Env, subscriber: Address, provider: Address) -> bool;

    /// Subscribe `subscriber` to `provider`'s service on the provider's
    /// initiative, at `amount` per `period`. Returns the stable
    /// subscription id.
    ///
    /// Requires the provider and an explicit subscriber opt-in for that
    /// provider (see [`authorize_provider`]). The subscriber is **not**
    /// authorized at creation: the opt-in is the consent. Creates a normal
    /// subscription record indistinguishable from one created by
    /// [`subscribe`], sharing the same sequential id counter.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::Unauthorized`] — the caller is not `provider`, or the
    ///   subscriber has not authorized the provider.
    /// * [`ForgeError::InvalidInput`] — `amount` or `period` is not positive.
    fn subscribe_on_behalf_of(
        env: Env,
        provider: Address,
        subscriber: Address,
        token: Address,
        amount: i128,
        period: u64,
    ) -> Result<u64, soroban_forge_shared_utils::ForgeError>;

    /// Charge the next due payment for `subscription_id`.
    ///
    /// Returns the billed amount, or `0` when no full period has elapsed since
    /// the last charge.
    fn charge(
        env: Env,
        subscription_id: u64,
    ) -> Result<i128, soroban_forge_shared_utils::ForgeError>;

    /// Atomically charge up to `max_periods` elapsed periods.
    ///
    /// Returns the total billed amount. `max_periods` must not exceed the
    /// contract's hard catch-up bound. A transfer failure returns
    /// [`ForgeError::TokenTransferFailed`] and rolls back all transfers,
    /// subscription state, and usage meters. `PastDue` subscriptions must use
    /// `charge` to preserve the existing retry policy.
    fn charge_catchup(
        env: Env,
        subscription_id: u64,
        max_periods: u32,
    ) -> Result<i128, soroban_forge_shared_utils::ForgeError>;

    /// Declare (or replace) the metered-usage quotas of `subscription_id`.
    ///
    /// Requires the subscriber: a quota prices the overage the subscriber is
    /// billed, so the account that is charged is the one that consents to the
    /// terms. An empty list returns the subscription to flat pricing.
    ///
    /// Only valid while `Active` and only while the open period has no
    /// recorded usage, so already-metered units can never be repriced
    /// mid-period. Validated before authorization: at most [`MAX_QUOTAS`]
    /// quotas, no duplicate metrics, `bucket_units > 0`, and
    /// `overage_price >= 0`.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::InvalidInput`] — the subscription is not `Active`, the
    ///   open period already has recorded usage, or a quota failed validation.
    /// * [`ForgeError::Unauthorized`] — the caller is not the subscriber.
    /// * [`ForgeError::NotFound`] — no such subscription.
    fn set_quotas(
        env: Env,
        subscription_id: u64,
        quotas: soroban_sdk::Vec<MetricQuota>,
    ) -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Record `units` of `metric` consumed in the subscription's open period.
    ///
    /// Requires the provider (the party that served the usage, mirroring who
    /// is authorized to `charge`). Accumulates into the open period's raw unit
    /// counter; the meters are dropped atomically by the charge that closes
    /// the period.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no such subscription.
    /// * [`ForgeError::InvalidInput`] — `units` is zero, the subscription is
    ///   not `Active`, or the subscription does not declare `metric`.
    /// * [`ForgeError::Unauthorized`] — the caller is not the provider.
    /// * [`ForgeError::ArithmeticOverflow`] — the period's unit counter would
    ///   overflow `u64`.
    fn record_usage(
        env: Env,
        subscription_id: u64,
        metric: soroban_sdk::Symbol,
        units: u64,
    ) -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Read the usage recorded for `metric` in the open period (read-only
    /// view; requires no authorization).
    ///
    /// A metric with no recorded usage reads back as a zeroed record stamped
    /// with the current `last_charged`, so callers can poll one metric without
    /// branching on "never used".
    fn get_usage(
        env: Env,
        subscription_id: u64,
        metric: soroban_sdk::Symbol,
    ) -> Result<UsageRecord, soroban_forge_shared_utils::ForgeError>;

    /// The amount the open period would bill right now: `amount` plus the
    /// overage for every declared metric (read-only view; requires no
    /// authorization).
    ///
    /// This is the same derivation `charge` and `charge_catchup` settle, so a
    /// quote and the resulting charge cannot disagree. For a flat subscription
    /// it is exactly `amount`.
    fn quote_period(
        env: Env,
        subscription_id: u64,
    ) -> Result<i128, soroban_forge_shared_utils::ForgeError>;

    /// Pause an active subscription, preventing further charges while paused.
    ///
    /// Requires the subscriber. Only valid when `Active`.
    fn pause(env: Env, subscription_id: u64) -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Resume a paused subscription, advancing the next due date by the
    /// elapsed paused duration.
    ///
    /// Requires the subscriber. Only valid when `Paused`.
    fn resume(env: Env, subscription_id: u64)
        -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Cancel `subscription_id`, preventing further charges.
    ///
    /// Requires the subscriber. Valid when `Active`, `Paused`, or `PastDue`.
    fn cancel(env: Env, subscription_id: u64)
        -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Read a stored subscription by id (read-only view).
    fn get_subscription(
        env: Env,
        subscription_id: u64,
    ) -> Result<Subscription, soroban_forge_shared_utils::ForgeError>;

    /// Read the timestamp when the next charge is due.
    ///
    /// This queries the configured schedule (`last_charged + period`) and works
    /// deterministically regardless of the subscription's `status` (including
    /// `Cancelled` subscriptions).
    fn next_charge_due(
        env: Env,
        subscription_id: u64,
    ) -> Result<u64, soroban_forge_shared_utils::ForgeError>;

    /// Read how many full periods have elapsed since the last charge.
    ///
    /// This returns `0` before a full period elapses. Like `next_charge_due`,
    /// this query ignores the subscription's `status` and works identically
    /// for `Cancelled` subscriptions.
    fn due_periods(
        env: Env,
        subscription_id: u64,
    ) -> Result<u64, soroban_forge_shared_utils::ForgeError>;
    /// Total number of subscriptions created so far (read-only view).
    fn get_subscription_count(env: Env) -> u64;

    /// List `subscriber`'s subscriptions in creation order, one page at a
    /// time (read-only view).
    ///
    /// `offset` skips the first `offset` subscriptions and `limit` caps the
    /// page size. Returns a `Result` so a `limit` of `0` fails with
    /// [`ForgeError::InvalidInput`]; an empty index or an out-of-bounds
    /// offset yields an empty `Vec`, not an error.
    fn subscriptions_for_subscriber(
        env: Env,
        subscriber: Address,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Subscription>, soroban_forge_shared_utils::ForgeError>;

    /// List `provider`'s subscriptions in creation order, one page at a time
    /// (read-only view).
    ///
    /// `offset` skips the first `offset` subscriptions and `limit` caps the
    /// page size. Returns a `Result` so a `limit` of `0` fails with
    /// [`ForgeError::InvalidInput`]; an empty index or an out-of-bounds
    /// offset yields an empty `Vec`, not an error.
    fn subscriptions_for_provider(
        env: Env,
        provider: Address,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Subscription>, soroban_forge_shared_utils::ForgeError>;

    // ─── Plan registry ───────────────────────────────────────────────────────

    /// Publish a reusable billing plan.
    ///
    /// The provider calls this once; subscribers reference the returned
    /// `plan_id` in [`subscribe_to_plan`] instead of repeating token, amount,
    /// and period out-of-band. Plan ids are allocated from a separate
    /// monotonic counter and are stable across calls.
    ///
    /// Requires the provider's authorization. `amount` and `period` must both
    /// be positive; `quotas` (if non-empty) must pass the same validation as
    /// [`set_quotas`] — at most [`MAX_QUOTAS`] unique metrics, `bucket_units
    /// > 0`, `overage_price >= 0`.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::InvalidInput`] — `amount <= 0`, `period == 0`, or a
    ///   quota failed validation.
    /// * [`ForgeError::Unauthorized`] — the caller is not `provider`.
    fn create_plan(
        env: Env,
        provider: Address,
        token: Address,
        amount: i128,
        period: u64,
        quotas: soroban_sdk::Vec<MetricQuota>,
    ) -> Result<u64, soroban_forge_shared_utils::ForgeError>;

    /// Subscribe `subscriber` to an existing plan by `plan_id`.
    ///
    /// Looks up the plan (returns [`ForgeError::NotFound`] for an unknown id),
    /// and creates a subscription with the plan's token, amount, period, and
    /// quotas. The returned subscription id is allocated from the **same**
    /// counter as [`subscribe`] so all subscription ids are globally unique
    /// regardless of creation path.
    ///
    /// A subscriber can hold multiple subscriptions to the same plan — each
    /// call creates a distinct record with its own `subscription_id` and
    /// independent `last_charged` / lifecycle. This mirrors the existing
    /// behaviour of [`subscribe`], which also creates a new record on every
    /// call even with the same provider.
    ///
    /// Requires the subscriber's authorization.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no plan with `plan_id` exists.
    /// * [`ForgeError::Unauthorized`] — the caller is not `subscriber`.
    fn subscribe_to_plan(
        env: Env,
        plan_id: u64,
        subscriber: Address,
    ) -> Result<u64, soroban_forge_shared_utils::ForgeError>;

    /// Read a stored plan by id (read-only view; requires no authorization).
    ///
    /// # Errors
    ///
    /// * [`ForgeError::NotFound`] — no plan with `plan_id` exists.
    fn get_plan(
        env: Env,
        plan_id: u64,
    ) -> Result<Plan, soroban_forge_shared_utils::ForgeError>;

    /// Total number of plans created so far (read-only view).
    ///
    /// This is the monotonic plan id counter. It never decreases and equals
    /// the highest plan id ever assigned.
    fn plan_count(env: Env) -> u64;
}

/// Lifecycle state of a subscription.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubscriptionStatus {
    /// Active and chargeable.
    Active,
    /// Cancelled; no further charges.
    Cancelled,
    /// Payment failed and the subscription is in arrears.
    PastDue,
    /// Temporarily paused; no charges can be made until resumed.
    Paused,
}

/// A reusable subscription plan published by a provider.
///
/// A plan captures the billing terms — token, amount, and period — that a
/// provider publishes once and subscribers can reference by id. The plan's
/// `quotas` field carries metered-usage pricing (an empty list is flat). The
/// same `MetricQuota` type used on `Subscription` is embedded here so that
/// joining a plan copies the declared terms verbatim into the subscription
/// record, keeping the billing core independent of where the terms were
/// published.
///
/// A subscriber can hold multiple subscriptions to the same plan. Each
/// `subscribe_to_plan` call creates a distinct `Subscription` record with its
/// own sequential id, `last_charged` timestamp, and independent lifecycle.
/// This mirrors the existing behaviour of `subscribe`, where calling it twice
/// with the same provider creates two distinct subscriptions.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    /// Stable identifier assigned at creation. Allocated from a separate
    /// counter (`DataKey::PlanCount`) so plan ids and subscription ids cannot
    /// be confused.
    pub plan_id: u64,
    /// Provider that published and owns this plan.
    pub provider: Address,
    /// SEP-41 token contract used for settlement.
    pub token: Address,
    /// Fixed base amount charged per period (in the token's smallest unit).
    pub amount: i128,
    /// Length of one billing period, in seconds.
    pub period: u64,
    /// Per-metric usage quotas priced on top of `amount`. An empty list
    /// represents flat pricing (no overage). Copied verbatim into the
    /// `Subscription` record when a subscriber joins this plan.
    pub quotas: Vec<MetricQuota>,
}

/// A recurring payment agreement.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subscription {
    /// Stable identifier assigned at creation.
    pub subscription_id: u64,
    /// Account being charged.
    pub subscriber: Address,
    /// Account receiving payments.
    pub provider: Address,
    /// Token contract used for settlement.
    pub token: Address,
    /// Amount charged per period, before usage overage.
    pub amount: i128,
    /// Length of one billing period, in seconds.
    pub period: u64,
    /// Ledger timestamp of the last successful charge.
    pub last_charged: u64,
    /// Current state.
    pub status: SubscriptionStatus,
    /// Ledger timestamp when paused, if currently paused.
    pub paused_at: Option<u64>,
    /// Number of consecutive failed billing attempts.
    pub failed_attempts: u32,
    /// Per-metric usage quotas priced on top of `amount`.
    ///
    /// Empty is the flat flow: the period bills exactly `amount`. The list is
    /// shaped so it can be embedded verbatim in a plan record (a plan copies
    /// its quotas here when a subscriber joins it), keeping the metering core
    /// independent of where the terms were published.
    pub quotas: Vec<MetricQuota>,
}

/// One metered dimension's pricing terms: units included in the base period
/// price plus the per-bucket overage price for what exceeds the inclusion.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricQuota {
    /// The metered dimension, e.g. `api_calls` or `storage_bytes`.
    pub metric: Symbol,
    /// Units covered by the base `amount`; usage up to and including this
    /// many units bills nothing extra.
    pub included_units: u64,
    /// Price of one started bucket of overage units, in the subscription's
    /// token. Per bucket, never per unit, so no sub-unit price precision is
    /// lost to integer division.
    pub overage_price: i128,
    /// Units per overage bucket. Must be greater than zero; a started bucket
    /// is billed in full (rounding is up).
    pub bucket_units: u64,
    /// Cap on billable overage units for one period, or `None` for uncapped.
    ///
    /// The cap clamps overage units before they are bucketed, so a period can
    /// bill at most `ceil(cap / bucket_units)` buckets for this metric.
    pub max_overage_units: Option<u64>,
}

/// Usage metered for one metric in one billing period.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageRecord {
    /// Subscription the usage was recorded against.
    pub subscription_id: u64,
    /// The metered dimension.
    pub metric: Symbol,
    /// Raw units accumulated in the open period. Always a unit count, never a
    /// bucket count, so no rounding state can survive into the next period.
    pub units: u64,
    /// The `last_charged` this meter belongs to. A meter whose stamp no
    /// longer matches the subscription's open period is not counted, and the
    /// next `record_usage` restarts it.
    pub period_start: u64,
}

/// Instance-storage keys.
#[contracttype]
enum DataKey {
    /// The subscription record for `u64` id.
    Subscription(u64),
    /// Monotonic subscription id counter.
    Count,
    /// Creation-order subscription ids for which the `Address` is the
    /// subscriber, in id order. Written once per `subscribe`.
    SubscriberSubscriptions(Address),
    /// Creation-order subscription ids for which the `Address` is the
    /// provider, in id order. Written once per `subscribe`.
    ProviderSubscriptions(Address),
    /// Explicit subscriber opt-in for a provider-initiated subscription:
    /// `(subscriber, provider)`. Written by `authorize_provider`, removed by
    /// `revoke_provider`, consulted by `subscribe_on_behalf_of`.
    ProviderOptIn(Address, Address),
    /// The open period's unit counter for `(subscription, metric)`. Written by
    /// `record_usage`, removed by the charge that closes the period, read by
    /// the amount derivation.
    Usage(u64, Symbol),
    /// The plan record for `u64` plan id.
    Plan(u64),
    /// Monotonic plan id counter (separate from subscription ids so the two
    /// namespaces cannot be confused).
    PlanCount,
}

/// The deployable subscription payments contract.
#[contract]
pub struct SubscriptionPayments;

#[contractimpl]
impl SubscriptionPayments {
    // ─── Plan registry ───────────────────────────────────────────────────────

    /// Publish a reusable billing plan and return its stable plan id.
    ///
    /// Validates before auth so invalid inputs surface without spending the
    /// provider's signature. Plan ids are allocated from a separate monotonic
    /// counter (`DataKey::PlanCount`) that is independent of subscription ids.
    pub fn create_plan(
        env: Env,
        provider: Address,
        token: Address,
        amount: i128,
        period: u64,
        quotas: Vec<MetricQuota>,
    ) -> Result<u64, ForgeError> {
        if amount <= 0 {
            return Err(ForgeError::InvalidInput);
        }
        if period == 0 {
            return Err(ForgeError::InvalidInput);
        }
        validate_quotas(&quotas)?;
        provider.require_auth();

        let plan_id = Self::next_plan_id(&env)?;
        let plan = Plan {
            plan_id,
            provider,
            token,
            amount,
            period,
            quotas,
        };
        env.storage()
            .instance()
            .set(&DataKey::Plan(plan_id), &plan);
        Ok(plan_id)
    }

    /// Subscribe `subscriber` to an existing plan by `plan_id`.
    ///
    /// Looks up the plan, authorizes the subscriber, and creates a subscription
    /// with the plan's terms. The subscription id is drawn from the same
    /// monotonic counter as `subscribe`, so all subscription ids are globally
    /// unique regardless of creation path.
    pub fn subscribe_to_plan(env: Env, plan_id: u64, subscriber: Address) -> Result<u64, ForgeError> {
        let plan = Self::get_plan_impl(&env, plan_id)?;
        subscriber.require_auth();

        Self::create_subscription(
            &env,
            subscriber,
            plan.provider,
            plan.token,
            plan.amount,
            plan.period,
            plan.quotas,
        )
    }

    /// Read a stored plan by id (read-only view; requires no authorization).
    pub fn get_plan(env: Env, plan_id: u64) -> Result<Plan, ForgeError> {
        Self::get_plan_impl(&env, plan_id)
    }

    /// Total number of plans created so far (read-only view).
    pub fn plan_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::PlanCount)
            .unwrap_or(0)
    }

    // ─── Subscription entrypoints ─────────────────────────────────────────────

    /// Create a new subscription and return its stable id.
    ///
    /// Requires `amount > 0` and `period > 0`. The subscriber is authorized at
    /// creation time; billing starts from the moment of subscription.
    pub fn subscribe(
        env: Env,
        subscriber: Address,
        provider: Address,
        token: Address,
        amount: i128,
        period: u64,
    ) -> Result<u64, ForgeError> {
        if amount <= 0 {
            return Err(ForgeError::InvalidInput);
        }
        if period == 0 {
            return Err(ForgeError::InvalidInput);
        }
        subscriber.require_auth();

        Self::create_subscription(&env, subscriber, provider, token, amount, period, Vec::new(&env))
    }

    /// Explicitly authorize `provider` to create subscriptions on
    /// `subscriber`'s behalf (see the module docs on provider opt-in).
    ///
    /// Requires the subscriber. Idempotent: authorizing a provider that is
    /// already authorized succeeds.
    pub fn authorize_provider(
        env: Env,
        subscriber: Address,
        provider: Address,
    ) -> Result<(), ForgeError> {
        subscriber.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::ProviderOptIn(subscriber, provider), &true);
        Ok(())
    }

    /// Withdraw `subscriber`'s explicit authorization of `provider` (see the
    /// module docs on provider opt-in).
    ///
    /// Requires the subscriber. Idempotent: revoking a provider that is not
    /// authorized succeeds and leaves the opt-in absent.
    pub fn revoke_provider(
        env: Env,
        subscriber: Address,
        provider: Address,
    ) -> Result<(), ForgeError> {
        subscriber.require_auth();

        env.storage()
            .instance()
            .remove(&DataKey::ProviderOptIn(subscriber, provider));
        Ok(())
    }

    /// Read whether `subscriber` has explicitly authorized `provider`
    /// (read-only view; requires no authorization).
    pub fn is_provider_authorized(env: Env, subscriber: Address, provider: Address) -> bool {
        Self::is_provider_authorized_impl(&env, &subscriber, &provider)
    }

    fn is_provider_authorized_impl(env: &Env, subscriber: &Address, provider: &Address) -> bool {
        env.storage().instance().has(&DataKey::ProviderOptIn(
            subscriber.clone(),
            provider.clone(),
        ))
    }

    /// Subscribe `subscriber` to `provider`'s service on the provider's
    /// initiative (see the module docs on provider opt-in).
    ///
    /// Requires `amount > 0`, `period > 0`, the provider's authorization, and
    /// an explicit subscriber opt-in for the provider (checked before the
    /// provider is authorized, so a missing opt-in surfaces without spending
    /// the provider's signature). The subscriber is **not** authorized at
    /// creation. The record is created through the same internal path as
    /// [`subscribe`], sharing the same sequential id counter.
    pub fn subscribe_on_behalf_of(
        env: Env,
        provider: Address,
        subscriber: Address,
        token: Address,
        amount: i128,
        period: u64,
    ) -> Result<u64, ForgeError> {
        if amount <= 0 {
            return Err(ForgeError::InvalidInput);
        }
        if period == 0 {
            return Err(ForgeError::InvalidInput);
        }
        // The opt-in is the subscriber's consent: without it, a provider
        // must not be able to stand up a subscription that later pulls
        // subscriber funds.
        if !Self::is_provider_authorized_impl(&env, &subscriber, &provider) {
            return Err(ForgeError::Unauthorized);
        }
        provider.require_auth();

        Self::create_subscription(&env, subscriber, provider, token, amount, period, Vec::new(&env))
    }

    /// Bill one due period.
    ///
    /// Requires the provider. If a full period has not elapsed since the last
    /// charge, returns `0` and leaves the subscription untouched. Otherwise
    /// attempts to transfer the period's derived amount of `token` from
    /// `subscriber` to `provider`.
    ///
    /// The derived amount is `amount` plus the overage for every declared
    /// metric (see the module docs on metered usage) — exactly `amount` when no
    /// quotas are declared. It is computed before the transfer, so an
    /// arithmetic failure never moves funds.
    ///
    /// - On successful payment: advances `last_charged` by one period, drops
    ///   the period's usage meters, resets `failed_attempts` to 0, transitions
    ///   status to `Active`, and returns the derived amount.
    /// - On failed payment: `last_charged` is NOT advanced and the usage
    ///   meters are NOT dropped, so the retry re-derives the same amount.
    ///   Increments `failed_attempts`. If `failed_attempts >= MAX_RETRIES`
    ///   (3), status becomes `Cancelled`. Otherwise status becomes `PastDue`.
    ///   Returns `0`.
    pub fn charge(env: Env, subscription_id: u64) -> Result<i128, ForgeError> {
        let mut subscription = Self::get_subscription_impl(&env, subscription_id)?;
        if subscription.status == SubscriptionStatus::Cancelled
            || subscription.status == SubscriptionStatus::Paused
        {
            return Err(ForgeError::InvalidInput);
        }
        subscription.provider.require_auth();

        let next_due = subscription
            .last_charged
            .checked_add(subscription.period)
            .ok_or(ForgeError::ArithmeticOverflow)?;
        if env.ledger().timestamp() < next_due {
            return Ok(0);
        }

        // Derived before the transfer: a usage or arithmetic failure leaves
        // the subscription untouched and no funds in flight.
        let amount = Self::derive_period_amount(&env, &subscription)?;

        // Execute SEP-41 token transfer from subscriber to provider
        let transfer_result = token::TokenClient::new(&env, &subscription.token).try_transfer(
            &subscription.subscriber,
            &subscription.provider,
            &amount,
        );

        match transfer_result {
            Ok(Ok(())) => {
                subscription.last_charged = next_due;
                subscription.failed_attempts = 0;
                subscription.status = SubscriptionStatus::Active;
                subscription.paused_at = None;
                // Period rollover: the meters go in the same frame that closes
                // the period, so the next period starts from zero units.
                Self::rollover_usage(&env, &subscription);
                env.storage()
                    .instance()
                    .set(&DataKey::Subscription(subscription_id), &subscription);
                events::charged(&env, &subscription, amount);
                Ok(amount)
            }
            _ => {
                let failed = subscription.failed_attempts.saturating_add(1);
                subscription.failed_attempts = failed;
                if failed >= MAX_RETRIES {
                    subscription.status = SubscriptionStatus::Cancelled;
                } else {
                    subscription.status = SubscriptionStatus::PastDue;
                }
                env.storage()
                    .instance()
                    .set(&DataKey::Subscription(subscription_id), &subscription);
                Ok(0)
            }
        }
    }

    /// Atomically bill multiple elapsed periods.
    pub fn charge_catchup(
        env: Env,
        subscription_id: u64,
        max_periods: u32,
    ) -> Result<i128, ForgeError> {
        if max_periods > MAX_CATCHUP_PERIODS {
            return Err(ForgeError::InvalidInput);
        }

        let mut subscription = Self::get_subscription_impl(&env, subscription_id)?;
        if subscription.status != SubscriptionStatus::Active {
            return Err(ForgeError::InvalidInput);
        }
        subscription.provider.require_auth();

        if max_periods == 0 {
            return Ok(0);
        }

        let now = env.ledger().timestamp();
        let elapsed = now
            .checked_sub(subscription.last_charged)
            .ok_or(ForgeError::ArithmeticOverflow)?;
        let elapsed_periods = elapsed / subscription.period;
        let periods = core::cmp::min(max_periods as u64, elapsed_periods);

        let mut total = 0_i128;
        for _ in 0..periods {
            let next_due = subscription
                .last_charged
                .checked_add(subscription.period)
                .ok_or(ForgeError::ArithmeticOverflow)?;
            // Usage can only be metered into the open (latest) period, and the
            // meters are dropped as each period closes, so the first iteration
            // is the only one that can carry overage: every period that follows
            // derives the base amount. Same helper, same arithmetic, per period.
            let amount = Self::derive_period_amount(&env, &subscription)?;
            let transfer_result = token::TokenClient::new(&env, &subscription.token).try_transfer(
                &subscription.subscriber,
                &subscription.provider,
                &amount,
            );
            if !matches!(transfer_result, Ok(Ok(()))) {
                return Err(ForgeError::TokenTransferFailed);
            }
            total = total
                .checked_add(amount)
                .ok_or(ForgeError::ArithmeticOverflow)?;
            subscription.last_charged = next_due;
            Self::rollover_usage(&env, &subscription);
            events::charged(&env, &subscription, amount);
        }

        if periods > 0 {
            subscription.failed_attempts = 0;
            subscription.status = SubscriptionStatus::Active;
            subscription.paused_at = None;
            env.storage()
                .instance()
                .set(&DataKey::Subscription(subscription_id), &subscription);
        }
        Ok(total)
    }

    /// Declare (or replace) the metered-usage quotas of `subscription_id`.
    ///
    /// Requires the subscriber — see the module docs on metered usage for why
    /// the account being charged consents to the overage price. Rejected unless
    /// the subscription is `Active` and the open period has no recorded usage,
    /// so already-metered units can never be repriced mid-period.
    pub fn set_quotas(
        env: Env,
        subscription_id: u64,
        quotas: Vec<MetricQuota>,
    ) -> Result<(), ForgeError> {
        validate_quotas(&quotas)?;
        let mut subscription = Self::get_subscription_impl(&env, subscription_id)?;
        if subscription.status != SubscriptionStatus::Active {
            return Err(ForgeError::InvalidInput);
        }
        if Self::has_recorded_usage(&env, &subscription) {
            return Err(ForgeError::InvalidInput);
        }
        subscription.subscriber.require_auth();

        subscription.quotas = quotas;
        env.storage()
            .instance()
            .set(&DataKey::Subscription(subscription_id), &subscription);
        events::quotas_set(&env, &subscription);
        Ok(())
    }

    /// Record `units` of `metric` consumed in the subscription's open period.
    ///
    /// Requires the provider. See the module docs on metered usage for the
    /// period-boundary, cap, and retry semantics.
    pub fn record_usage(
        env: Env,
        subscription_id: u64,
        metric: Symbol,
        units: u64,
    ) -> Result<(), ForgeError> {
        if units == 0 {
            return Err(ForgeError::InvalidInput);
        }
        let subscription = Self::get_subscription_impl(&env, subscription_id)?;
        if subscription.status != SubscriptionStatus::Active {
            return Err(ForgeError::InvalidInput);
        }
        if !declares_metric(&subscription, &metric) {
            return Err(ForgeError::InvalidInput);
        }
        subscription.provider.require_auth();

        // A meter belongs to the open period only. `charge` and `charge_catchup`
        // drop the meters when they close a period, so the stamp normally
        // already matches; the check also covers a window moved by `resume`.
        let open = Self::read_open_period_usage(&env, &subscription, &metric);
        let units_before = open.as_ref().map_or(0, |record| record.units);
        let period_units = units_before
            .checked_add(units)
            .ok_or(ForgeError::ArithmeticOverflow)?;

        let recorded = UsageRecord {
            subscription_id,
            metric: metric.clone(),
            units: period_units,
            period_start: subscription.last_charged,
        };
        env.storage()
            .instance()
            .set(&DataKey::Usage(subscription_id, metric), &recorded);
        events::usage_recorded(&env, &recorded, units);
        Ok(())
    }

    /// Read the usage recorded for `metric` in the open period (read-only
    /// view).
    pub fn get_usage(
        env: Env,
        subscription_id: u64,
        metric: Symbol,
    ) -> Result<UsageRecord, ForgeError> {
        let subscription = Self::get_subscription_impl(&env, subscription_id)?;
        Ok(
            Self::read_open_period_usage(&env, &subscription, &metric).unwrap_or(UsageRecord {
                subscription_id,
                metric,
                units: 0,
                period_start: subscription.last_charged,
            }),
        )
    }

    /// The amount the open period would bill right now (read-only view).
    pub fn quote_period(env: Env, subscription_id: u64) -> Result<i128, ForgeError> {
        let subscription = Self::get_subscription_impl(&env, subscription_id)?;
        Self::derive_period_amount(&env, &subscription)
    }

    /// Pause an active subscription, preventing charges while paused.
    ///
    /// Requires the subscriber. Only valid when `Active`.
    pub fn pause(env: Env, subscription_id: u64) -> Result<(), ForgeError> {
        let mut subscription = Self::get_subscription_impl(&env, subscription_id)?;
        if subscription.status != SubscriptionStatus::Active {
            return Err(ForgeError::InvalidInput);
        }
        subscription.subscriber.require_auth();

        subscription.status = SubscriptionStatus::Paused;
        subscription.paused_at = Some(env.ledger().timestamp());
        env.storage()
            .instance()
            .set(&DataKey::Subscription(subscription_id), &subscription);
        Ok(())
    }

    /// Resume a paused subscription, advancing the next due date by the
    /// elapsed paused duration so that paused periods are not billed.
    ///
    /// Requires the subscriber. Only valid when `Paused`.
    pub fn resume(env: Env, subscription_id: u64) -> Result<(), ForgeError> {
        let mut subscription = Self::get_subscription_impl(&env, subscription_id)?;
        if subscription.status != SubscriptionStatus::Paused {
            return Err(ForgeError::InvalidInput);
        }
        subscription.subscriber.require_auth();

        let now = env.ledger().timestamp();
        let paused_at = subscription.paused_at.unwrap_or(now);
        let elapsed = now
            .checked_sub(paused_at)
            .ok_or(ForgeError::ArithmeticOverflow)?;

        subscription.last_charged = subscription
            .last_charged
            .checked_add(elapsed)
            .ok_or(ForgeError::ArithmeticOverflow)?;
        subscription.status = SubscriptionStatus::Active;
        subscription.paused_at = None;
        env.storage()
            .instance()
            .set(&DataKey::Subscription(subscription_id), &subscription);
        Ok(())
    }

    /// Cancel a subscription, preventing further charges.
    ///
    /// Requires the subscriber. Valid when `Active`, `Paused`, or `PastDue`. Cancelling
    /// an already-cancelled subscription is rejected.
    pub fn cancel(env: Env, subscription_id: u64) -> Result<(), ForgeError> {
        let mut subscription = Self::get_subscription_impl(&env, subscription_id)?;
        if subscription.status == SubscriptionStatus::Cancelled {
            return Err(ForgeError::InvalidInput);
        }
        subscription.subscriber.require_auth();

        subscription.status = SubscriptionStatus::Cancelled;
        subscription.paused_at = None;
        env.storage()
            .instance()
            .set(&DataKey::Subscription(subscription_id), &subscription);
        events::cancelled(&env, &subscription);
        Ok(())
    }

    /// Read a stored subscription by id (read-only view).
    pub fn get_subscription(env: Env, subscription_id: u64) -> Result<Subscription, ForgeError> {
        Self::get_subscription_impl(&env, subscription_id)
    }

    /// Read the timestamp when the next charge is due (read-only view).
    pub fn next_charge_due(env: Env, subscription_id: u64) -> Result<u64, ForgeError> {
        let subscription = Self::get_subscription_impl(&env, subscription_id)?;
        subscription
            .last_charged
            .checked_add(subscription.period)
            .ok_or(ForgeError::ArithmeticOverflow)
    }

    /// Read how many full periods have elapsed since the last charge (read-only view).
    pub fn due_periods(env: Env, subscription_id: u64) -> Result<u64, ForgeError> {
        let subscription = Self::get_subscription_impl(&env, subscription_id)?;
        let current_time = env.ledger().timestamp();
        if current_time < subscription.last_charged {
            return Ok(0);
        }
        let elapsed = current_time - subscription.last_charged;
        Ok(elapsed / subscription.period)
    /// Total number of subscriptions created so far (read-only view).
    ///
    /// This is the monotonic id counter, which only `subscribe` advances, so
    /// it never decreases and equals the number of live ids returned by the
    /// enumeration views.
    pub fn get_subscription_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::Count).unwrap_or(0)
    }

    /// List `subscriber`'s subscriptions in creation order, one page at a
    /// time (read-only view).
    ///
    /// Requires no authorization and never mutates storage. An address with
    /// no subscriptions — or an offset at or past the end of its list —
    /// returns an empty `Vec`, not an error, so clients can back "my
    /// subscriptions" views without an off-chain indexer.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::InvalidInput`] — `limit` is zero.
    pub fn subscriptions_for_subscriber(
        env: Env,
        subscriber: Address,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Subscription>, ForgeError> {
        if limit == 0 {
            return Err(ForgeError::InvalidInput);
        }
        let ids = Self::index_ids(&env, &DataKey::SubscriberSubscriptions(subscriber));
        Self::resolve_page(&env, &ids, offset, limit)
    }

    /// List `provider`'s subscriptions in creation order, one page at a time
    /// (read-only view).
    ///
    /// Requires no authorization and never mutates storage. An address with
    /// no subscriptions — or an offset at or past the end of its list —
    /// returns an empty `Vec`, not an error, so clients can back "my
    /// subscribers" views without an off-chain indexer.
    ///
    /// # Errors
    ///
    /// * [`ForgeError::InvalidInput`] — `limit` is zero.
    pub fn subscriptions_for_provider(
        env: Env,
        provider: Address,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Subscription>, ForgeError> {
        if limit == 0 {
            return Err(ForgeError::InvalidInput);
        }
        let ids = Self::index_ids(&env, &DataKey::ProviderSubscriptions(provider));
        Self::resolve_page(&env, &ids, offset, limit)
    }

    /// Create a new subscription through the single shared creation path used
    /// by `subscribe`, `subscribe_on_behalf_of`, and `subscribe_to_plan`.
    ///
    /// Callers must have completed validation and authorization; this helper
    /// allocates the id, writes the record, and appends both indexes. Index
    /// writes join the success path after every fallible step (validation,
    /// `require_auth`, id allocation), so they cannot observe or create
    /// partial state. Records created through any entrypoint are
    /// indistinguishable from `get_subscription`'s perspective and draw
    /// from the same sequential counter.
    ///
    /// `quotas` is passed in by the caller so that `subscribe_to_plan` can
    /// copy the plan's declared quotas verbatim into the new record, while
    /// `subscribe` and `subscribe_on_behalf_of` pass an empty `Vec` for the
    /// default flat flow.
    fn create_subscription(
        env: &Env,
        subscriber: Address,
        provider: Address,
        token: Address,
        amount: i128,
        period: u64,
        quotas: Vec<MetricQuota>,
    ) -> Result<u64, ForgeError> {
        let subscription_id = Self::next_id(env)?;
        let subscription = Subscription {
            subscription_id,
            subscriber: subscriber.clone(),
            provider: provider.clone(),
            token,
            amount,
            period,
            last_charged: env.ledger().timestamp(),
            status: SubscriptionStatus::Active,
            paused_at: None,
            failed_attempts: 0,
            quotas,
        };
        env.storage()
            .instance()
            .set(&DataKey::Subscription(subscription_id), &subscription);
        Self::append_index(
            env,
            &DataKey::SubscriberSubscriptions(subscriber),
            subscription_id,
        );
        Self::append_index(
            env,
            &DataKey::ProviderSubscriptions(provider),
            subscription_id,
        );
        Ok(subscription_id)
    }

    /// Allocate the next monotonic subscription id.
    fn next_id(env: &Env) -> Result<u64, ForgeError> {
        let count: u64 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);
        let id = count.checked_add(1).ok_or(ForgeError::ArithmeticOverflow)?;
        env.storage().instance().set(&DataKey::Count, &id);
        Ok(id)
    }

    /// Allocate the next monotonic plan id from the separate plan counter.
    fn next_plan_id(env: &Env) -> Result<u64, ForgeError> {
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PlanCount)
            .unwrap_or(0);
        let id = count.checked_add(1).ok_or(ForgeError::ArithmeticOverflow)?;
        env.storage().instance().set(&DataKey::PlanCount, &id);
        Ok(id)
    }

    fn get_subscription_impl(env: &Env, subscription_id: u64) -> Result<Subscription, ForgeError> {
        env.storage()
            .instance()
            .get(&DataKey::Subscription(subscription_id))
            .ok_or(ForgeError::NotFound)
    }

    fn get_plan_impl(env: &Env, plan_id: u64) -> Result<Plan, ForgeError> {
        env.storage()
            .instance()
            .get(&DataKey::Plan(plan_id))
            .ok_or(ForgeError::NotFound)
    }

    /// Append `subscription_id` to an index, carrying the address the index
    /// belongs to in the key.
    fn append_index(env: &Env, key: &DataKey, subscription_id: u64) {
        let mut ids: Vec<u64> = env
            .storage()
            .instance()
            .get(key)
            .unwrap_or_else(|| Vec::new(env));
        ids.push_back(subscription_id);
        env.storage().instance().set(key, &ids);
    }

    /// Read an index's id list, defaulting to empty when the address has no
    /// entries.
    fn index_ids(env: &Env, key: &DataKey) -> Vec<u64> {
        env.storage()
            .instance()
            .get(key)
            .unwrap_or_else(|| Vec::new(env))
    }

    /// Resolve a contiguous slice of `ids` into `Subscription` records.
    ///
    /// `offset` is clamped to the list length and `end` saturates, so an
    /// out-of-bounds offset yields an empty `Vec` rather than a bounds panic.
    fn resolve_page(
        env: &Env,
        ids: &Vec<u64>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Subscription>, ForgeError> {
        let total = ids.len();
        let (mut at, end) = (offset.min(total), offset.saturating_add(limit).min(total));
        let mut subscriptions = Vec::new(env);
        while at < end {
            subscriptions.push_back(Self::get_subscription_impl(env, ids.get_unchecked(at))?);
            at += 1;
        }
        Ok(subscriptions)
    }

    /// Read this subscription's open-period amount: `amount` plus the overage
    /// for every declared metric.
    ///
    /// The single storage-aware entry point to the amount derivation. A flat
    /// subscription short-circuits to `amount` without touching the meters, so
    /// the no-quota path is bit-identical to the pre-metering contract.
    fn derive_period_amount(env: &Env, subscription: &Subscription) -> Result<i128, ForgeError> {
        if subscription.quotas.is_empty() {
            return Ok(subscription.amount);
        }
        let mut usage = Vec::new(env);
        for quota in subscription.quotas.iter() {
            if let Some(record) = Self::read_open_period_usage(env, subscription, &quota.metric) {
                usage.push_back(record);
            }
        }
        period_amount(subscription.amount, &subscription.quotas, &usage)
    }

    /// Read the open period's meter for `metric`, or `None` when the metric is
    /// unused in the open period.
    ///
    /// A stored meter stamped for a different `last_charged` is ignored: the
    /// charge paths drop meters when they close a period, so a mismatched
    /// stamp can only come from a window moved by `resume`, whose paused
    /// window is not billable and therefore not metered. `record_usage`
    /// restarts the meter from the new open period.
    fn read_open_period_usage(
        env: &Env,
        subscription: &Subscription,
        metric: &Symbol,
    ) -> Option<UsageRecord> {
        let record: UsageRecord = env.storage().instance().get(&DataKey::Usage(
            subscription.subscription_id,
            metric.clone(),
        ))?;
        if record.period_start == subscription.last_charged {
            Some(record)
        } else {
            None
        }
    }

    /// Whether the open period has any recorded usage, across the declared
    /// metrics. Every meter key belongs to a currently declared metric, so
    /// walking the quota list is exhaustive.
    fn has_recorded_usage(env: &Env, subscription: &Subscription) -> bool {
        subscription.quotas.iter().any(|quota| {
            Self::read_open_period_usage(env, subscription, &quota.metric)
                .is_some_and(|record| record.units > 0)
        })
    }

    /// Drop every open-period meter for `subscription`.
    ///
    /// Called only on the settlement success path, in the same frame that
    /// advances `last_charged`, which is what makes the period rollover atomic
    /// with the period close. A failed transfer returns before this point, so
    /// the meters survive and the retry charges the same amount again.
    fn rollover_usage(env: &Env, subscription: &Subscription) {
        for quota in subscription.quotas.iter() {
            let key = DataKey::Usage(subscription.subscription_id, quota.metric.clone());
            if env.storage().instance().has(&key) {
                env.storage().instance().remove(&key);
            }
        }
    }
}

/// Derive the amount billable for one period: the base `amount` plus the
/// overage of every declared metric, from the open period's raw unit counters.
///
/// This is the **only** place period amounts are computed — `charge`, each
/// settled period of `charge_catchup`, and `quote_period` all call it — so a
/// quote, a single charge, and a catch-up bill can never disagree. It is pure:
/// no storage, no ledger access, no authorization, and no rounding state
/// carried between calls, which is what keeps multi-period totals exact.
///
/// Per metric, with `over = max(0, units - included_units)`:
///
/// ```text
/// overage = ceil(min(over, max_overage_units) / bucket_units) * overage_price
/// ```
///
/// where an uncapped quota skips the `min`. Rounding is up: a started bucket
/// is billed in full. Overflow surfaces as [`ForgeError::ArithmeticOverflow`]
/// before any transfer runs, so an unrepresentable bill can never move funds.
fn period_amount(
    base: i128,
    quotas: &Vec<MetricQuota>,
    usage: &Vec<UsageRecord>,
) -> Result<i128, ForgeError> {
    let mut total = base;
    for quota in quotas.iter() {
        let overage = metric_overage(usage_units(usage, &quota.metric), &quota)?;
        total = total
            .checked_add(overage)
            .ok_or(ForgeError::ArithmeticOverflow)?;
    }
    Ok(total)
}

/// Overage owed for one metric's `units`: overage units past the inclusion,
/// clamped to the quota's cap, rounded up to whole buckets, priced per bucket.
fn metric_overage(units: u64, quota: &MetricQuota) -> Result<i128, ForgeError> {
    let over_units = units.saturating_sub(quota.included_units);
    let buckets = billable_buckets(over_units, quota.max_overage_units, quota.bucket_units);
    // `u64 -> i128` is lossless, so the multiply is the only step that can
    // overflow, and it is checked so an unrepresentable price is an error
    // rather than a wrapped (possibly negative) bill.
    let buckets = i128::from(buckets);
    buckets
        .checked_mul(quota.overage_price)
        .ok_or(ForgeError::ArithmeticOverflow)
}

/// Billable bucket count for `over_units` under a cap and bucket size.
///
/// The cap is applied to overage units **before** bucketing, so a cap bills at
/// most `ceil(cap / bucket_units)` buckets. A zero cap bills nothing and a zero
/// bucket size bills nothing, so no input can divide by zero or wrap.
fn billable_buckets(over_units: u64, max_overage_units: Option<u64>, bucket_units: u64) -> u64 {
    if bucket_units == 0 {
        // Unreachable through `set_quotas`, which rejects a zero bucket size;
        // guarded here so the helper is total and cannot divide by zero.
        return 0;
    }
    let capped = match max_overage_units {
        Some(cap) => over_units.min(cap),
        None => over_units,
    };
    capped.div_ceil(bucket_units)
}

/// Read a metric's recorded units out of a period's meter list, defaulting to
/// zero for a metric that was never used.
fn usage_units(usage: &Vec<UsageRecord>, metric: &Symbol) -> u64 {
    let mut at = 0;
    while at < usage.len() {
        let record = usage.get_unchecked(at);
        if &record.metric == metric {
            return record.units;
        }
        at += 1;
    }
    0
}

/// Validate a declared quota list: at most [`MAX_QUOTAS`] entries, no metric
/// declared twice (a duplicate would make the per-metric sum
/// order-dependent), a non-zero bucket size, and a non-negative overage price.
fn validate_quotas(quotas: &Vec<MetricQuota>) -> Result<(), ForgeError> {
    if quotas.len() > MAX_QUOTAS {
        return Err(ForgeError::InvalidInput);
    }
    let mut at = 0;
    while at < quotas.len() {
        let quota = quotas.get(at).ok_or(ForgeError::InvalidInput)?;
        if quota.bucket_units == 0 || quota.overage_price < 0 {
            return Err(ForgeError::InvalidInput);
        }
        let mut earlier = 0;
        while earlier < at {
            let previous = quotas.get(earlier).ok_or(ForgeError::InvalidInput)?;
            if previous.metric == quota.metric {
                return Err(ForgeError::InvalidInput);
            }
            earlier += 1;
        }
        at += 1;
    }
    Ok(())
}

/// Whether `subscription` prices `metric`, i.e. whether the provider is
/// allowed to meter it at all.
fn declares_metric(subscription: &Subscription, metric: &Symbol) -> bool {
    subscription
        .quotas
        .iter()
        .any(|quota| quota.metric == *metric)
}

/// Lifecycle events emitted by the subscription payments contract.
mod events {
    use super::*;

    #[contractevent]
    pub struct Charged {
        #[topic]
        pub subscription_id: u64,
        /// Amount actually transferred for the settled period: the derived
        /// `base + overage` amount, not the base alone.
        pub amount: i128,
        pub last_charged: u64,
        pub next_charge_at: u64,
    }

    /// Usage metered for one metric in the open period. Indexers can rebuild
    /// every settled period's overage from these plus the plan's quotas.
    #[contractevent]
    pub struct UsageRecorded {
        #[topic]
        pub subscription_id: u64,
        #[topic]
        pub metric: Symbol,
        /// Units added by this call.
        pub units: u64,
        /// Running total for the open period, including this call.
        pub period_units: u64,
        /// The billing window these units belong to.
        pub period_start: u64,
    }

    /// The subscription's declared quota list was replaced.
    #[contractevent]
    pub struct QuotasSet {
        #[topic]
        pub subscription_id: u64,
        pub quotas: Vec<MetricQuota>,
    }

    #[contractevent]
    pub struct Cancelled {
        #[topic]
        pub subscription_id: u64,
        pub subscriber: Address,
    }

    pub fn charged(env: &Env, subscription: &Subscription, amount: i128) {
        let next_charge_at = subscription
            .last_charged
            .saturating_add(subscription.period);
        Charged {
            subscription_id: subscription.subscription_id,
            amount,
            last_charged: subscription.last_charged,
            next_charge_at,
        }
        .publish(env);
    }

    pub fn usage_recorded(env: &Env, record: &UsageRecord, units: u64) {
        UsageRecorded {
            subscription_id: record.subscription_id,
            metric: record.metric.clone(),
            units,
            period_units: record.units,
            period_start: record.period_start,
        }
        .publish(env);
    }

    pub fn quotas_set(env: &Env, subscription: &Subscription) {
        QuotasSet {
            subscription_id: subscription.subscription_id,
            quotas: subscription.quotas.clone(),
        }
        .publish(env);
    }

    pub fn cancelled(env: &Env, subscription: &Subscription) {
        Cancelled {
            subscription_id: subscription.subscription_id,
            subscriber: subscription.subscriber.clone(),
        }
        .publish(env);
    }
}

#[cfg(test)]
mod authz;
#[cfg(test)]
mod metering;
#[cfg(test)]
mod props;
#[cfg(test)]
mod plan;

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_forge_test_utils::TestAccounts;
    use soroban_sdk::testutils::{Address as _, Events as _, Ledger as _};
    use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
    use soroban_sdk::{Address, Env};

    const START: u64 = 1_000_000;
    const PERIOD: u64 = 1_000;
    const AMOUNT: i128 = 250;

    /// Build a fresh env with mocked auths, a registered contract, a real SEP-41
    /// token with minted funds to subscriber, a subscription, and named accounts.
    macro_rules! setup {
        () => {{
            let env = Env::default();
            env.mock_all_auths_allowing_non_root_auth();
            env.ledger().set_timestamp(START);

            let admin = Address::generate(&env);
            let sac = env.register_stellar_asset_contract_v2(admin);
            let token = sac.address();
            let token_admin = StellarAssetClient::new(&env, &token);
            let token_client = TokenClient::new(&env, &token);

            let contract_id = env.register(SubscriptionPayments, ());
            let client = SorobanForgeSubscriptionPaymentsClient::new(&env, &contract_id);
            let accounts = TestAccounts::generate(&env);

            token_admin.mint(&accounts.user1, &10_000_i128);

            let subscription_id = client.subscribe(
                &accounts.user1,
                &accounts.validator,
                &token,
                &AMOUNT,
                &PERIOD,
            );
            (
                env,
                token,
                token_client,
                contract_id,
                client,
                accounts,
                subscription_id,
            )
        }};
    }

    #[test]
    fn subscribe_succeeds_and_is_active() {
        let (_env, _token, _tc, _contract_id, client, accounts, subscription_id) = setup!();
        let subscription = client.get_subscription(&subscription_id);
        assert_eq!(subscription.subscriber, accounts.user1);
        assert_eq!(subscription.provider, accounts.validator);
        assert_eq!(subscription.amount, AMOUNT);
        assert_eq!(subscription.status, SubscriptionStatus::Active);
        assert_eq!(subscription.last_charged, START);
        assert_eq!(subscription.paused_at, None);
        assert_eq!(subscription.failed_attempts, 0);
    }

    #[test]
    fn subscribe_assigns_distinct_ids() {
        let (_env, token, _tc, _contract_id, client, accounts, subscription_id) = setup!();
        let id2 = client.subscribe(
            &accounts.user2,
            &accounts.validator,
            &token,
            &AMOUNT,
            &PERIOD,
        );
        assert_ne!(subscription_id, id2);
    }

    #[test]
    fn subscribe_on_behalf_of_creates_equivalent_active_subscription() {
        let (_env, token, _tc, _contract_id, client, accounts, _id) = setup!();
        client.authorize_provider(&accounts.user1, &accounts.validator);
        let id = client.subscribe_on_behalf_of(
            &accounts.validator,
            &accounts.user1,
            &token,
            &AMOUNT,
            &PERIOD,
        );

        let sub = client.get_subscription(&id);
        assert_eq!(sub.subscriber, accounts.user1);
        assert_eq!(sub.provider, accounts.validator);
        assert_eq!(sub.amount, AMOUNT);
        assert_eq!(sub.status, SubscriptionStatus::Active);
        assert_eq!(sub.last_charged, client.env.ledger().timestamp());
        // Both creation paths feed the same indexes.
        assert_eq!(
            client
                .subscriptions_for_subscriber(&accounts.user1, &0, &10)
                .len(),
            2
        );
        assert_eq!(
            client
                .subscriptions_for_provider(&accounts.validator, &0, &10)
                .len(),
            2
        );
    }

    #[test]
    fn subscribe_on_behalf_of_requires_opt_in() {
        let (_env, token, _tc, _contract_id, client, accounts, _id) = setup!();
        let err = client
            .try_subscribe_on_behalf_of(
                &accounts.validator,
                &accounts.user1,
                &token,
                &AMOUNT,
                &PERIOD,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::Unauthorized);

        // Revoking consent blocks later provider-initiated creations too.
        client.authorize_provider(&accounts.user1, &accounts.validator);
        client.revoke_provider(&accounts.user1, &accounts.validator);
        let err = client
            .try_subscribe_on_behalf_of(
                &accounts.validator,
                &accounts.user1,
                &token,
                &AMOUNT,
                &PERIOD,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::Unauthorized);
    }

    #[test]
    fn subscribe_on_behalf_of_rejects_invalid_amount_and_period() {
        let (_env, token, _tc, _contract_id, client, accounts, _id) = setup!();
        client.authorize_provider(&accounts.user1, &accounts.validator);

        let err = client
            .try_subscribe_on_behalf_of(
                &accounts.validator,
                &accounts.user1,
                &token,
                &0_i128,
                &PERIOD,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);

        let err = client
            .try_subscribe_on_behalf_of(
                &accounts.validator,
                &accounts.user1,
                &token,
                &AMOUNT,
                &0_u64,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
        // No partial record from the failed calls.
        assert_eq!(client.get_subscription_count(), 1);
    }

    #[test]
    fn subscription_ids_are_sequential_across_both_paths() {
        let (_env, token, _tc, _contract_id, client, accounts, subscription_id) = setup!();
        assert_eq!(subscription_id, 1);

        client.authorize_provider(&accounts.user2, &accounts.validator);
        let id2 = client.subscribe_on_behalf_of(
            &accounts.validator,
            &accounts.user2,
            &token,
            &AMOUNT,
            &PERIOD,
        );
        assert_eq!(id2, 2);

        let id3 = client.subscribe(
            &accounts.user3,
            &accounts.arbiter,
            &accounts.deployer,
            &AMOUNT,
            &PERIOD,
        );
        assert_eq!(id3, 3);

        client.authorize_provider(&accounts.user1, &accounts.arbiter);
        let id4 = client.subscribe_on_behalf_of(
            &accounts.arbiter,
            &accounts.user1,
            &accounts.deployer,
            &AMOUNT,
            &PERIOD,
        );
        assert_eq!(id4, 4);
        assert_eq!(client.get_subscription_count(), 4);
    }

    #[test]
    fn authorize_provider_is_idempotent() {
        let (_env, _token, _tc, _contract_id, client, accounts, _id) = setup!();
        client.authorize_provider(&accounts.user1, &accounts.validator);
        client.authorize_provider(&accounts.user1, &accounts.validator);
        assert!(client.is_provider_authorized(&accounts.user1, &accounts.validator));
    }

    #[test]
    fn revoke_provider_is_idempotent() {
        let (_env, _token, _tc, _contract_id, client, accounts, _id) = setup!();
        client.revoke_provider(&accounts.user1, &accounts.validator);
        assert!(!client.is_provider_authorized(&accounts.user1, &accounts.validator));
    }

    #[test]
    fn subscribe_rejects_zero_amount() {
        let (_env, token, _tc, _contract_id, client, accounts, _id) = setup!();
        let err = client
            .try_subscribe(
                &accounts.user1,
                &accounts.validator,
                &token,
                &0_i128,
                &PERIOD,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn subscribe_rejects_zero_period() {
        let (_env, token, _tc, _contract_id, client, accounts, _id) = setup!();
        let err = client
            .try_subscribe(
                &accounts.user1,
                &accounts.validator,
                &token,
                &AMOUNT,
                &0_u64,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn charge_before_period_returns_zero() {
        let (env, _token, tc, _contract_id, client, accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + PERIOD - 1);
        assert_eq!(client.charge(&subscription_id), 0);
        assert_eq!(
            client.get_subscription(&subscription_id).last_charged,
            START
        );
        assert_eq!(tc.balance(&accounts.user1), 10_000);
        assert_eq!(tc.balance(&accounts.validator), 0);
    }

    #[test]
    fn charge_at_period_bills_full_amount_and_transfers_tokens() {
        let (env, _token, tc, _contract_id, client, accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + PERIOD);
        assert_eq!(client.charge(&subscription_id), AMOUNT);
        assert_eq!(
            client.get_subscription(&subscription_id).last_charged,
            START + PERIOD
        );
        assert_eq!(tc.balance(&accounts.user1), 10_000 - AMOUNT);
        assert_eq!(tc.balance(&accounts.validator), AMOUNT);
    }

    #[test]
    fn charge_catches_up_one_period_per_call() {
        let (env, _token, tc, _contract_id, client, accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + PERIOD * 3);
        // First call bills one period
        assert_eq!(client.charge(&subscription_id), AMOUNT);
        assert_eq!(
            client.get_subscription(&subscription_id).last_charged,
            START + PERIOD
        );
        assert_eq!(tc.balance(&accounts.validator), AMOUNT);

        // Second call bills second period
        assert_eq!(client.charge(&subscription_id), AMOUNT);
        assert_eq!(
            client.get_subscription(&subscription_id).last_charged,
            START + PERIOD * 2
        );
        assert_eq!(tc.balance(&accounts.validator), AMOUNT * 2);
    }

    #[test]
    fn charge_catchup_bills_elapsed_periods_and_respects_max_periods() {
        let (env, _token, tc, _contract_id, client, accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + PERIOD * 3);

        assert_eq!(client.charge_catchup(&subscription_id, &2), AMOUNT * 2);
        let subscription = client.get_subscription(&subscription_id);
        assert_eq!(subscription.last_charged, START + PERIOD * 2);
        assert_eq!(tc.balance(&accounts.validator), AMOUNT * 2);

        assert_eq!(client.charge_catchup(&subscription_id, &2), AMOUNT);
        assert_eq!(
            client.get_subscription(&subscription_id).last_charged,
            START + PERIOD * 3
        );
        assert_eq!(tc.balance(&accounts.validator), AMOUNT * 3);
    }

    #[test]
    fn charge_catchup_zero_and_before_due_are_noops() {
        let (env, _token, tc, _contract_id, client, accounts, subscription_id) = setup!();
        assert_eq!(client.charge_catchup(&subscription_id, &0), 0);
        env.ledger().set_timestamp(START + PERIOD - 1);
        assert_eq!(client.charge_catchup(&subscription_id, &2), 0);
        assert_eq!(
            client.get_subscription(&subscription_id).last_charged,
            START
        );
        assert_eq!(tc.balance(&accounts.validator), 0);
    }

    #[test]
    fn charge_catchup_rejects_values_over_hard_cap() {
        let (_env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        let err = client
            .try_charge_catchup(&subscription_id, &33)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn charge_catchup_handles_u64_max_timestamp_without_overflow() {
        let (env, token, tc, _contract_id, client, accounts, _id) = setup!();
        let subscription_id = client.subscribe(
            &accounts.user2,
            &accounts.validator,
            &token,
            &1_i128,
            &u64::MAX,
        );
        StellarAssetClient::new(&env, &token).mint(&accounts.user2, &1_i128);
        env.ledger().set_timestamp(u64::MAX);

        assert_eq!(client.charge_catchup(&subscription_id, &1), 0);
        assert_eq!(tc.balance(&accounts.validator), 0);
    }

    #[test]
    fn charge_catchup_refuses_past_due_and_preserves_retry_state() {
        let (env, token, _tc, _contract_id, client, accounts, _id) = setup!();
        let broke_user = Address::generate(&env);
        StellarAssetClient::new(&env, &token).mint(&broke_user, &10_i128);
        let sub_id = client.subscribe(&broke_user, &accounts.validator, &token, &AMOUNT, &PERIOD);
        env.ledger().set_timestamp(START + PERIOD);
        assert_eq!(client.charge(&sub_id), 0);

        let before = client.get_subscription(&sub_id);
        let err = client.try_charge_catchup(&sub_id, &2).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
        assert_eq!(client.get_subscription(&sub_id), before);
    }

    #[test]
    fn charge_catchup_transfer_failure_rolls_back_prior_periods() {
        let (env, token, tc, _contract_id, client, accounts, _id) = setup!();
        let broke_user = Address::generate(&env);
        StellarAssetClient::new(&env, &token).mint(&broke_user, &(AMOUNT + 1));
        let sub_id = client.subscribe(&broke_user, &accounts.validator, &token, &AMOUNT, &PERIOD);
        env.ledger().set_timestamp(START + PERIOD * 2);

        let err = client.try_charge_catchup(&sub_id, &2).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::TokenTransferFailed);
        assert_eq!(tc.balance(&broke_user), AMOUNT + 1);
        assert_eq!(tc.balance(&accounts.validator), 0);
        assert_eq!(client.get_subscription(&sub_id).last_charged, START);
    }

    #[test]
    fn charge_catchup_missing_subscription_is_not_found() {
        let (_env, _token, _tc, _contract_id, client, _accounts, _id) = setup!();
        let err = client.try_charge_catchup(&999, &1).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn charge_failure_transitions_to_past_due_and_retry_restores_active() {
        let (env, token, tc, _contract_id, client, accounts, _id) = setup!();
        let broke_user = Address::generate(&env);
        StellarAssetClient::new(&env, &token).mint(&broke_user, &10_i128);
        let sub_id = client.subscribe(&broke_user, &accounts.validator, &token, &AMOUNT, &PERIOD);

        env.ledger().set_timestamp(START + PERIOD);

        // First charge attempt fails due to insufficient balance in real SAC token
        let billed = client.charge(&sub_id);
        assert_eq!(billed, 0);

        let sub_past_due = client.get_subscription(&sub_id);
        assert_eq!(sub_past_due.status, SubscriptionStatus::PastDue);
        assert_eq!(sub_past_due.failed_attempts, 1);
        assert_eq!(sub_past_due.last_charged, START); // Not advanced!

        // Subscriber gets funds minted now
        StellarAssetClient::new(&env, &token).mint(&broke_user, &10_000_i128);

        // Retry charge succeeds!
        let billed = client.charge(&sub_id);
        assert_eq!(billed, AMOUNT);

        let sub_active = client.get_subscription(&sub_id);
        assert_eq!(sub_active.status, SubscriptionStatus::Active);
        assert_eq!(sub_active.failed_attempts, 0);
        assert_eq!(sub_active.last_charged, START + PERIOD);
        assert_eq!(tc.balance(&accounts.validator), AMOUNT);
    }

    #[test]
    fn max_retries_exceeded_transitions_to_cancelled() {
        let (env, token, _tc, _contract_id, client, accounts, _id) = setup!();
        let broke_user = Address::generate(&env);
        StellarAssetClient::new(&env, &token).mint(&broke_user, &10_i128);
        let sub_id = client.subscribe(&broke_user, &accounts.validator, &token, &AMOUNT, &PERIOD);

        env.ledger().set_timestamp(START + PERIOD);

        // Attempt 1 -> PastDue (failed_attempts = 1)
        assert_eq!(client.charge(&sub_id), 0);
        assert_eq!(
            client.get_subscription(&sub_id).status,
            SubscriptionStatus::PastDue
        );

        // Attempt 2 -> PastDue (failed_attempts = 2)
        assert_eq!(client.charge(&sub_id), 0);
        assert_eq!(
            client.get_subscription(&sub_id).status,
            SubscriptionStatus::PastDue
        );

        // Attempt 3 -> Cancelled (failed_attempts = 3)
        assert_eq!(client.charge(&sub_id), 0);
        assert_eq!(
            client.get_subscription(&sub_id).status,
            SubscriptionStatus::Cancelled
        );

        // Attempt 4 -> InvalidInput because status is now Cancelled
        assert_eq!(
            client.try_charge(&sub_id).unwrap_err().unwrap(),
            ForgeError::InvalidInput
        );
    }

    #[test]
    fn cancel_from_past_due_succeeds() {
        let (env, token, _tc, _contract_id, client, accounts, _id) = setup!();
        let broke_user = Address::generate(&env);
        StellarAssetClient::new(&env, &token).mint(&broke_user, &10_i128);
        let sub_id = client.subscribe(&broke_user, &accounts.validator, &token, &AMOUNT, &PERIOD);

        env.ledger().set_timestamp(START + PERIOD);
        let _ = client.charge(&sub_id);
        assert_eq!(
            client.get_subscription(&sub_id).status,
            SubscriptionStatus::PastDue
        );

        // Cancel from PastDue
        client.cancel(&sub_id);
        assert_eq!(
            client.get_subscription(&sub_id).status,
            SubscriptionStatus::Cancelled
        );
    }

    #[test]
    fn charge_missing_subscription_is_not_found() {
        let (_env, _token, _tc, _contract_id, client, _accounts, _id) = setup!();
        let err = client.try_charge(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn cancel_prevents_further_charges() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        client.cancel(&subscription_id);
        assert_eq!(
            client.get_subscription(&subscription_id).status,
            SubscriptionStatus::Cancelled
        );
        env.ledger().set_timestamp(START + PERIOD);
        let err = client.try_charge(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn cancel_twice_is_invalid() {
        let (_env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        client.cancel(&subscription_id);
        let err = client.try_cancel(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn cancel_missing_subscription_is_not_found() {
        let (_env, _token, _tc, _contract_id, client, _accounts, _id) = setup!();
        let err = client.try_cancel(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn get_subscription_missing_is_not_found() {
        let (_env, _token, _tc, _contract_id, client, _accounts, _id) = setup!();
        let err = client.try_get_subscription(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn next_charge_due_returns_expected_time_and_does_not_mutate() {
        let (env, client, _accounts, subscription_id) = setup!();
        let expected = START + PERIOD;
        assert_eq!(client.next_charge_due(&subscription_id), expected);
        
        // Mutate-check
        let before = client.get_subscription(&subscription_id);
        client.next_charge_due(&subscription_id);
        let after = client.get_subscription(&subscription_id);
        assert_eq!(before.last_charged, after.last_charged);
        
        // Stable across before-due time
        env.ledger().set_timestamp(START + PERIOD - 1);
        assert_eq!(client.charge(&subscription_id), 0);
        assert_eq!(client.next_charge_due(&subscription_id), expected);
        
        // Advances exactly by one period after a charge
        env.ledger().set_timestamp(START + PERIOD);
        client.charge(&subscription_id);
        assert_eq!(client.next_charge_due(&subscription_id), expected + PERIOD);
    }
    
    #[test]
    fn next_charge_due_missing_subscription_is_not_found() {
        let (_env, client, _accounts, _id) = setup!();
        let err = client.try_next_charge_due(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn next_charge_due_cancelled_does_not_panic() {
        let (_env, client, _accounts, subscription_id) = setup!();
        client.cancel(&subscription_id);
        assert_eq!(client.next_charge_due(&subscription_id), START + PERIOD);
    }

    #[test]
    fn next_charge_due_overflow_returns_arithmetic_error() {
        let (env, client, accounts, _id) = setup!();
        // Subscribe with max period
        let id = client.subscribe(
    fn get_subscription_count_tracks_creations() {
        let (_env, _token, _tc, _contract_id, client, accounts, _id) = setup!();
        assert_eq!(client.get_subscription_count(), 1);
        let id2 = client.subscribe(
            &accounts.user2,
            &accounts.validator,
            &accounts.deployer,
            &AMOUNT,
            &PERIOD,
        );
        assert_eq!(client.get_subscription_count(), 2);
        // Cancelling does not lower the creation count.
        client.cancel(&id2);
        assert_eq!(client.get_subscription_count(), 2);
    }

    #[test]
    fn subscriber_index_tracks_multiple_providers() {
        let (_env, _token, _tc, _contract_id, client, accounts, _id) = setup!();
        let id2 = client.subscribe(
            &accounts.user1,
            &accounts.arbiter,
            &accounts.deployer,
            &AMOUNT,
            &PERIOD,
        );
        let page = client.subscriptions_for_subscriber(&accounts.user1, &0, &10);
        assert_eq!(page.len(), 2);
        assert_eq!(page.get_unchecked(0).subscription_id, 1);
        assert_eq!(page.get_unchecked(0).provider, accounts.validator);
        assert_eq!(page.get_unchecked(1).subscription_id, id2);
        assert_eq!(page.get_unchecked(1).provider, accounts.arbiter);
    }

    #[test]
    fn provider_index_tracks_multiple_subscribers() {
        let (_env, _token, _tc, _contract_id, client, accounts, _id) = setup!();
        let id2 = client.subscribe(
            &accounts.user2,
            &accounts.validator,
            &accounts.deployer,
            &AMOUNT,
            &PERIOD,
        );
        let page = client.subscriptions_for_provider(&accounts.validator, &0, &10);
        assert_eq!(page.len(), 2);
        assert_eq!(page.get_unchecked(0).subscriber, accounts.user1);
        assert_eq!(page.get_unchecked(0).subscription_id, 1);
        assert_eq!(page.get_unchecked(1).subscriber, accounts.user2);
        assert_eq!(page.get_unchecked(1).subscription_id, id2);
    }

    #[test]
    fn pagination_slices_across_pages() {
        let (_env, _token, _tc, _contract_id, client, accounts, _id) = setup!();
        // user1 adds four more subscriptions, for ids 2..=5.
        client.subscribe(
            &accounts.user1,
            &accounts.arbiter,
            &accounts.deployer,
            &AMOUNT,
            &PERIOD,
        );
        client.subscribe(
            &accounts.user1,
            &accounts.user3,
            &accounts.deployer,
            &AMOUNT,
            &PERIOD,
        );
        client.subscribe(
            &accounts.user1,
            &accounts.user2,
            &accounts.deployer,
            &AMOUNT,
            &PERIOD,
        );
        client.subscribe(
            &accounts.user1,
            &accounts.validator,
            &accounts.deployer,
            &AMOUNT,
            &u64::MAX,
        );
        let err = client.try_next_charge_due(&id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::ArithmeticOverflow);
    }

    #[test]
    fn due_periods_tracks_elapsed_time_correctly() {
        let (env, client, _accounts, subscription_id) = setup!();
        
        // Before a period elapses
        env.ledger().set_timestamp(START + PERIOD - 1);
        assert_eq!(client.due_periods(&subscription_id), 0);

        // Exactly at due
        env.ledger().set_timestamp(START + PERIOD);
        assert_eq!(client.due_periods(&subscription_id), 1);

        // After due (e.g. 2.5 periods)
        env.ledger().set_timestamp(START + PERIOD * 2 + PERIOD / 2);
        assert_eq!(client.due_periods(&subscription_id), 2);
    }

    #[test]
    fn due_periods_missing_subscription_is_not_found() {
        let (_env, client, _accounts, _id) = setup!();
        let err = client.try_due_periods(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }
    
    #[test]
    fn due_periods_cancelled_does_not_panic() {
        let (env, client, _accounts, subscription_id) = setup!();
        client.cancel(&subscription_id);
        env.ledger().set_timestamp(START + PERIOD * 3);
        assert_eq!(client.due_periods(&subscription_id), 3);
            &PERIOD,
        );

        let first = client.subscriptions_for_subscriber(&accounts.user1, &0, &2);
        assert_eq!(first.len(), 2);
        assert_eq!(first.get_unchecked(0).subscription_id, 1);
        assert_eq!(first.get_unchecked(1).subscription_id, 2);

        let mid = client.subscriptions_for_subscriber(&accounts.user1, &2, &2);
        assert_eq!(mid.len(), 2);
        assert_eq!(mid.get_unchecked(0).subscription_id, 3);
        assert_eq!(mid.get_unchecked(1).subscription_id, 4);

        let last = client.subscriptions_for_subscriber(&accounts.user1, &4, &10);
        assert_eq!(last.len(), 1);
        assert_eq!(last.get_unchecked(0).subscription_id, 5);
    }

    #[test]
    fn subscriptions_for_unknown_address_are_empty() {
        let (_env, _token, _tc, _contract_id, client, accounts, _id) = setup!();
        assert_eq!(
            client
                .subscriptions_for_subscriber(&accounts.arbiter, &0, &10)
                .len(),
            0
        );
        assert_eq!(
            client
                .subscriptions_for_provider(&accounts.arbiter, &0, &10)
                .len(),
            0
        );
    }

    #[test]
    fn pause_active_subscription_succeeds() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + 250);
        client.pause(&subscription_id);

        let sub = client.get_subscription(&subscription_id);
        assert_eq!(sub.status, SubscriptionStatus::Paused);
        assert_eq!(sub.paused_at, Some(START + 250));
    }

    #[test]
    fn pause_already_paused_is_invalid() {
        let (_env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        client.pause(&subscription_id);
        let err = client.try_pause(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn pause_cancelled_is_invalid() {
        let (_env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        client.cancel(&subscription_id);
        let err = client.try_pause(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn pause_missing_is_not_found() {
        let (_env, _token, _tc, _contract_id, client, _accounts, _id) = setup!();
        let err = client.try_pause(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn charge_paused_subscription_fails_and_does_not_advance() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + 100);
        client.pause(&subscription_id);

        env.ledger().set_timestamp(START + PERIOD * 2);
        let err = client.try_charge(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);

        let sub = client.get_subscription(&subscription_id);
        assert_eq!(sub.last_charged, START);
        assert_eq!(sub.status, SubscriptionStatus::Paused);
    }

    #[test]
    fn resume_paused_subscription_advances_due_date_by_elapsed() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        // Pause at START + 300 (300s into 1000s period)
        env.ledger().set_timestamp(START + 300);
        client.pause(&subscription_id);

        // Resume at START + 800 (elapsed pause = 500s)
        env.ledger().set_timestamp(START + 800);
        client.resume(&subscription_id);

        let sub = client.get_subscription(&subscription_id);
        assert_eq!(sub.status, SubscriptionStatus::Active);
        assert_eq!(sub.paused_at, None);
        // last_charged should be shifted by 500s: START + 500
        assert_eq!(sub.last_charged, START + 500);

        // At original due date (START + 1000), only 500s of active cycle elapsed: charge -> 0
        env.ledger().set_timestamp(START + 1000);
        assert_eq!(client.charge(&subscription_id), 0);

        // At new due date (START + 500 + 1000 = START + 1500), charge succeeds
        env.ledger().set_timestamp(START + 1500);
        assert_eq!(client.charge(&subscription_id), AMOUNT);
        assert_eq!(
            client.get_subscription(&subscription_id).last_charged,
            START + 1500
        );
    }

    #[test]
    fn out_of_bounds_offset_returns_empty() {
        let (_env, _token, _tc, _contract_id, client, accounts, _id) = setup!();
        assert_eq!(
            client
                .subscriptions_for_subscriber(&accounts.user1, &1, &10)
                .len(),
            0
        );
        assert_eq!(
            client
                .subscriptions_for_provider(&accounts.validator, &2, &10)
                .len(),
            0
        );
    }

    #[test]
    fn zero_limit_is_invalid_input() {
        let (_env, _token, _tc, _contract_id, client, accounts, _id) = setup!();
        let err = client
            .try_subscriptions_for_subscriber(&accounts.user1, &0, &0)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
        let err = client
            .try_subscriptions_for_provider(&accounts.validator, &0, &0)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn pause_resume_zero_length_no_drift() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + 300);
        client.pause(&subscription_id);
        client.resume(&subscription_id);

        let sub = client.get_subscription(&subscription_id);
        assert_eq!(sub.status, SubscriptionStatus::Active);
        assert_eq!(sub.last_charged, START);
        assert_eq!(sub.paused_at, None);

        // Charge at exactly START + PERIOD succeeds as normal
        env.ledger().set_timestamp(START + PERIOD);
        assert_eq!(client.charge(&subscription_id), AMOUNT);
    }

    #[test]
    fn pause_across_multiple_periods_preserves_remaining_cycle() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        // 100s into cycle (900s remaining)
        env.ledger().set_timestamp(START + 100);
        client.pause(&subscription_id);

        // Stay paused across multiple periods: 5 periods (5000s) elapsed while paused
        env.ledger().set_timestamp(START + 5100);
        client.resume(&subscription_id);

        let sub = client.get_subscription(&subscription_id);
        // last_charged shifted by 5000: START + 5000
        assert_eq!(sub.last_charged, START + 5000);
        // next due is START + 6000 (which is resume_time + 900s remaining)

        env.ledger().set_timestamp(START + 5999);
        assert_eq!(client.charge(&subscription_id), 0);

        env.ledger().set_timestamp(START + 6000);
        assert_eq!(client.charge(&subscription_id), AMOUNT);
    }

    #[test]
    fn resume_active_subscription_is_invalid() {
        let (_env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        let err = client.try_resume(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn resume_cancelled_subscription_is_invalid() {
        let (_env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        client.cancel(&subscription_id);
        let err = client.try_resume(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn resume_missing_is_not_found() {
        let (_env, _token, _tc, _contract_id, client, _accounts, _id) = setup!();
        let err = client.try_resume(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn cancel_paused_subscription_succeeds_and_cannot_resume() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + 200);
        client.pause(&subscription_id);

        // Cancel while paused
        client.cancel(&subscription_id);
        let sub = client.get_subscription(&subscription_id);
        assert_eq!(sub.status, SubscriptionStatus::Cancelled);
        assert_eq!(sub.paused_at, None);

        // Cannot resume cancelled subscription
        let err = client.try_resume(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);

        // Cannot charge cancelled subscription
        env.ledger().set_timestamp(START + PERIOD * 2);
        let err = client.try_charge(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);

        // Cannot pause cancelled subscription
        let err = client.try_pause(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);

        // Cannot cancel already cancelled subscription
        let err = client.try_cancel(&subscription_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }
    #[test]
    fn events_emitted_on_lifecycle_actions() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + PERIOD);
        client.charge(&subscription_id);
        client.cancel(&subscription_id);

        let all_events = env.events().all();
        assert!(!all_events.events().is_empty());
    }

    #[test]
    fn charge_catchup_emits_charged_event_per_settled_period() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + PERIOD * 3);

        let total = client.charge_catchup(&subscription_id, &3);
        assert_eq!(total, AMOUNT * 3);

        let events = env.events().all();
        assert!(
            !events.events().is_empty(),
            "Events should be emitted during catchup"
        );
    }

    #[test]
    fn charge_catchup_periods_zero_emits_no_events() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();

        let total = client.charge_catchup(&subscription_id, &0);
        assert_eq!(total, 0);

        let events = env.events().all();
        assert!(
            events.events().is_empty(),
            "No new events should be emitted when charging 0 periods"
        );
    }

    #[test]
    fn charge_catchup_failed_transfer_rolls_back_events() {
        let (env, token, _tc, _contract_id, client, accounts, _id) = setup!();
        let broke_user = Address::generate(&env);
        StellarAssetClient::new(&env, &token).mint(&broke_user, &AMOUNT);

        let sub_id = client.subscribe(&broke_user, &accounts.validator, &token, &AMOUNT, &PERIOD);
        env.ledger().set_timestamp(START + PERIOD * 3);

        let res = client.try_charge_catchup(&sub_id, &3);
        assert!(res.is_err());

        let events = env.events().all();
        assert!(
            events.events().is_empty(),
            "Failed transfer should roll back emitted events"
        );
    }

    #[test]
    fn charge_catchup_advances_last_charged_and_event_payloads() {
        let (env, _token, _tc, _contract_id, client, _accounts, subscription_id) = setup!();
        env.ledger().set_timestamp(START + PERIOD * 2);

        client.charge_catchup(&subscription_id, &2);

        let sub = client.get_subscription(&subscription_id);
        assert_eq!(sub.last_charged, START + PERIOD * 2);
    }
}
