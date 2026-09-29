# Subscription Payments Contract

Recurring, on-chain subscription billing: a subscriber authorizes a provider to
pull a fixed `amount` per `period` (seconds) through SEP-41 token transfers.

## Interface

```rust
fn subscribe(subscriber, provider, token, amount, period) -> Result<u64, ForgeError>
fn subscribe_on_behalf_of(provider, subscriber, token, amount, period) -> Result<u64, ForgeError>
fn authorize_provider(subscriber, provider) -> Result<(), ForgeError>
fn revoke_provider(subscriber, provider) -> Result<(), ForgeError>
fn is_provider_authorized(subscriber, provider) -> bool
fn charge(subscription_id) -> Result<i128, ForgeError>
fn charge_catchup(subscription_id, max_periods: u32) -> Result<i128, ForgeError>
fn set_quotas(subscription_id, quotas: Vec<MetricQuota>) -> Result<(), ForgeError>
fn record_usage(subscription_id, metric: Symbol, units: u64) -> Result<(), ForgeError>
fn quote_period(subscription_id) -> Result<i128, ForgeError>
fn cancel(subscription_id) -> Result<(), ForgeError>
fn get_subscription(subscription_id) -> Result<Subscription, ForgeError>
fn get_usage(subscription_id, metric: Symbol) -> Result<UsageRecord, ForgeError>
fn get_subscription_count() -> u64
fn subscriptions_for_subscriber(subscriber, offset, limit) -> Result<Vec<Subscription>, ForgeError>
fn subscriptions_for_provider(provider, offset, limit) -> Result<Vec<Subscription>, ForgeError>
// Plan registry (issue #116)
fn create_plan(provider, token, amount, period, quotas: Vec<MetricQuota>) -> Result<u64, ForgeError>
fn subscribe_to_plan(plan_id, subscriber) -> Result<u64, ForgeError>
fn get_plan(plan_id) -> Result<Plan, ForgeError>
fn plan_count() -> u64
```

- `subscribe` requires the subscriber and returns a stable, monotonic id.
- `charge` requires the provider and settles at most one full period per call.
- `charge_catchup` requires the provider and settles `min(max_periods,
elapsed_periods)` periods in one atomic invocation. `max_periods == 0` is a
  no-op; values above the hard cap of 32 are rejected. The cap bounds Soroban
  instruction use and token outlay for keeper calls.
- `charge_catchup` refuses `PastDue`; call `charge` to use the existing retry
  policy. A failed catch-up transfer returns `TokenTransferFailed` and rolls
  back all transfers and `last_charged` through Soroban frame rollback.
- `cancel` requires the subscriber and prevents further charges.
- `set_quotas`, `record_usage`, and the metering views are described under
  [Metered usage](#metered-usage-quotas-and-overage).

## Metered usage: quotas and overage

A subscription can declare up to 16 per-metric quotas, so a period bills
`amount` plus overage for real consumption. **No quotas means the original flat
flow**: the charged amount and every event payload are identical to a contract
without this feature.

```rust
struct MetricQuota {
    metric: Symbol,             // e.g. api_calls
    included_units: u64,        // covered by the base `amount`
    overage_price: i128,        // per started bucket
    bucket_units: u64,          // units per bucket, must be > 0
    max_overage_units: Option<u64>, // cap on billable overage units
}
```

The billable amount for one period is derived in a single pure helper used by
`charge`, by every period of `charge_catchup`, and by `quote_period`:

```text
amount = base
       + Σ over metric of ceil(min(max(0, units - included), cap) / bucket) * overage_price
```

with the `min` skipped for an uncapped quota.

- `set_quotas` requires the **subscriber** — a quota prices what the subscriber
  is billed, so the charged account consents to the terms. It is rejected unless
  the subscription is `Active` and the open period has no recorded usage, so
  already-metered units can never be repriced mid-period. An empty list returns
  the subscription to flat pricing.
- `record_usage` requires the **provider**, mirroring who may `charge`, and
  rejects a metric the subscription does not declare, so usage the subscriber
  has not been asked to pay for can never be billed.
- `record_usage` accumulates **raw units** for the open period only, so each
  period's amount is a pure function of that period's units: charging `N`
  periods bills exactly `N` times the per-period derivation, and no rounding can
  compound across periods. A bucket is billed once started (rounded up), and the
  price is per bucket, never per unit.
- Meters are dropped atomically with the charge that closes the period, in the
  same frame that advances `last_charged`. A failed transfer returns before that
  point, so every meter survives and the arrears retry re-derives an identical
  amount.
- `charge_catchup` settles the open period **last**: usage can only be metered
  into the open (latest) period, so earlier unsettled periods bill the base
  amount and only the final one carries overage.
- `record_usage` requires `Active`, which freezes the meter at the attempted
  amount once a charge has failed, so the `PastDue` retry is identical to the
  attempt that failed.
- Caps are enforced by clamping at settlement, never by rejecting, so a usage
  spike cannot wedge a subscription; the subscriber's worst case per period is
  `base + Σ caps`. A `u64` unit-counter overflow surfaces as
  `ArithmeticOverflow` rather than wrapping into a silent underbill.

## Subscription States

- `Active` — chargeable
- `Cancelled` — no further charges
- `PastDue` — a failed single-period payment requiring `charge` retry semantics

## Secondary Indices

Each subscription is indexed in two secondary lists, written on the `subscribe`
success path:

- `SubscriberSubscriptions(subscriber)` — creation-order subscription ids for
  which the address is the subscriber.
- `ProviderSubscriptions(provider)` — creation-order subscription ids for which
  the address is the provider.

The indices store lightweight `Vec<u64>` id lists; full `Subscription` records
are only loaded for the requested page slice.

## View Methods

- `get_subscription_count()` returns the total number of subscriptions created
  (the monotonic id counter; cancellation never lowers it).
- `subscriptions_for_subscriber(subscriber, offset, limit)` returns a page of
  `Subscription` records for the subscriber, in creation order.
- `subscriptions_for_provider(provider, offset, limit)` returns a page of
  `Subscription` records for the provider, in creation order.

Both enumeration methods are read-only (no authorization, no storage writes).
`offset` skips the first `offset` subscriptions and `limit` caps the page size:
an empty index or an out-of-bounds offset yields an empty `Vec`, and a `limit`
of `0` fails with `ForgeError::InvalidInput`.

## Storage

- `Subscription(id)` — the record for id `u64`.
- `Count` — monotonic subscription id counter.
- `SubscriberSubscriptions(address)` — subscriber id index.
- `ProviderSubscriptions(address)` — provider id index.
- `Usage(subscription_id, metric)` — the open period's raw unit counter for one
  metric, stamped with the `last_charged` window it belongs to; removed by the
  charge that closes the period.

## Payment Semantics

`charge` returns the billed amount, or `0` when no full period has elapsed since
the last charge. `charge_catchup` returns the total billed amount and advances
`last_charged` once per successfully settled period. Period and total arithmetic
uses checked operations. For a metered subscription the billed amount is the
derived `base + overage` amount rather than `amount` alone; it is computed
before the transfer, so an unrepresentable bill never moves funds.

## Events

The contract emits typed on-chain lifecycle events for indexers and off-chain monitoring:

- `Subscribed` (topic: `subscription_id: u64`) — emitted when a subscription is created via `subscribe`. Contains `subscriber`, `provider`, `token`, `amount`, and `period`.
- `Charged` (topic: `subscription_id: u64`) — emitted on successful billing via `charge` (one event) or `charge_catchup` (one event per settled period). Contains `amount`, `last_charged`, and `next_charge_at`.
- `Cancelled` (topic: `subscription_id: u64`) — emitted when a subscription is cancelled via `cancel`. Contains `subscriber`.


## Plan Registry

A provider can publish a reusable billing plan once and share its `plan_id`
with subscribers, who then self-serve without needing the token, amount, and
period out-of-band.

### Creating a plan

```rust
fn create_plan(provider, token, amount, period, quotas) -> Result<u64, ForgeError>
```

- Requires the **provider's authorization**.
- Validates `amount > 0` and `period > 0` before auth, so invalid inputs never
  spend the provider's signature.
- `quotas` follows the same validation as `set_quotas`: at most 16 entries, no
  duplicate metrics, `bucket_units > 0`, `overage_price >= 0`. Pass an empty
  `Vec` for flat pricing.
- Returns a stable `plan_id` allocated from a **separate monotonic counter**
  (`DataKey::PlanCount`) so plan ids and subscription ids are in distinct
  namespaces and can never be confused.
- Plan ids are sequential starting from 1 and stable — they never change after
  creation.

### Subscribing to a plan

```rust
fn subscribe_to_plan(plan_id, subscriber) -> Result<u64, ForgeError>
```

- Requires the **subscriber's authorization**.
- Looks up the plan; returns `ForgeError::NotFound` for an unknown `plan_id`.
- Creates a `Subscription` record with the plan's token, amount, period, and
  quotas copied verbatim. The subscription id is allocated from the **same**
  counter as `subscribe`, so all subscription ids are globally unique regardless
  of creation path.
- **A subscriber can hold multiple subscriptions to the same plan.** Each call
  creates a distinct record with its own `subscription_id`, `last_charged`
  timestamp, and independent lifecycle. This mirrors the existing behaviour of
  `subscribe`, which also creates a new record on every call even with the same
  provider.
- The resulting subscription is indistinguishable from one created via
  `subscribe`: it appears in both subscriber and provider secondary indexes,
  supports `charge`, `charge_catchup`, `pause`, `resume`, `cancel`,
  `set_quotas`, `record_usage`, and all views.

### Reading plan state

```rust
fn get_plan(plan_id) -> Result<Plan, ForgeError>  // no auth required
fn plan_count() -> u64                            // no auth required
```

- `get_plan` returns `ForgeError::NotFound` for an unknown id. No
  authorization required.
- `plan_count` returns the total number of plans created (the monotonic counter;
  it never decreases). No authorization required.

### Plan struct

```rust
struct Plan {
    plan_id:  u64,
    provider: Address,
    token:    Address,
    amount:   i128,
    period:   u64,
    quotas:   Vec<MetricQuota>,  // empty = flat pricing
}
```

### Authorization table (plan entrypoints)

| Entrypoint          | Required authorization |
| ------------------- | ---------------------- |
| `create_plan`       | provider               |
| `subscribe_to_plan` | subscriber             |
| `get_plan`          | none                   |
| `plan_count`        | none                   |

### Storage keys (plan registry)

- `Plan(plan_id: u64)` — the plan record.
- `PlanCount` — monotonic plan id counter (separate from `Count` used for
  subscriptions).
