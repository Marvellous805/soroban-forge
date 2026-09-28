import { Buffer } from "buffer";
import { Address } from "@stellar/stellar-sdk";
import {
  AssembledTransaction,
  Client as ContractClient,
  ClientOptions as ContractClientOptions,
  MethodOptions,
  Result,
  Spec as ContractSpec,
} from "@stellar/stellar-sdk/contract";
import type {
  u32,
  i32,
  u64,
  i64,
  u128,
  i128,
  u256,
  i256,
  Option,
  Timepoint,
  Duration,
} from "@stellar/stellar-sdk/contract";
export * from "@stellar/stellar-sdk";
export * as contract from "@stellar/stellar-sdk/contract";
export * as rpc from "@stellar/stellar-sdk/rpc";

if (typeof window !== "undefined") {
  //@ts-ignore Buffer exists
  window.Buffer = window.Buffer || Buffer;
}





/**
 * A recurring payment agreement.
 */
export interface Subscription {
  /**
 * Amount charged per period.
 */
amount: i128;
  /**
 * Number of consecutive failed billing attempts.
 */
failed_attempts: u32;
  /**
 * Ledger timestamp of the last successful charge.
 */
last_charged: u64;
  /**
 * Ledger timestamp when paused, if currently paused.
 */
paused_at: Option<u64>;
  /**
 * Length of one billing period, in seconds.
 */
period: u64;
  /**
 * Account receiving payments.
 */
provider: string;
  /**
 * Current state.
 */
status: SubscriptionStatus;
  /**
 * Account being charged.
 */
subscriber: string;
  /**
 * Stable identifier assigned at creation.
 */
subscription_id: u64;
  /**
 * Token contract used for settlement.
 */
token: string;
}

/**
 * Lifecycle state of a subscription.
 */
export type SubscriptionStatus = {tag: "Active", values: void} | {tag: "Cancelled", values: void} | {tag: "PastDue", values: void} | {tag: "Paused", values: void};


/**
 * A participant in a multi-party flow (escrow, governance, ...).
 */
export interface Party {
  /**
 * On-chain address of the participant.
 */
address: string;
  /**
 * Whether this party has granted approval for the current action.
 */
approved: boolean;
  /**
 * Human-readable role label, e.g. `"buyer"` or `"arbiter"`.
 */
role: string;
}


/**
 * Inclusive time window expressed as Unix timestamps (seconds).
 * 
 * Stored as plain `u64` because `soroban_sdk` models time as `u64`; a
 * dedicated newtype would add conversions without benefit.
 */
export interface TimeBounds {
  /**
 * Latest moment (inclusive) at which the window is active.
 */
end: u64;
  /**
 * Earliest moment (inclusive) at which the window is active.
 */
start: u64;
}


/**
 * A page of results plus the cursor needed to fetch the next page.
 * 
 * Items are stored as serialized `Bytes` so the helper is agnostic to the
 * concrete value type a contract paginates. Callers decode each item into
 * their domain type. `Debug` is omitted because the SDK collection does not
 * implement it for this contract type.
 */
export interface PaginatedResult {
  /**
 * Cursor describing the next page (offset advanced by `limit`).
 */
cursor: PaginationCursor;
  /**
 * Items on this page.
 */
items: Array<Buffer>;
  /**
 * Total number of items across all pages.
 */
total: u32;
}


/**
 * Offset/limit pair for paginated reads.
 */
export interface PaginationCursor {
  /**
 * Maximum number of items to return.
 */
limit: u32;
  /**
 * Number of items to skip from the start of the full result set.
 */
offset: u32;
}

/**
 * Shared error type used across all Soroban Forge contracts.
 * 
 * Defining a single error enum in `shared-utils` keeps the on-chain error
 * space consistent and intelligible to SDK consumers, and avoids every
 * contract re-declaring the same failure modes. Contract crates may expose
 * their own domain-specific errors, but should prefer these where they fit.
 * 
 * Error codes start at 1; code 0 is reserved by the Soroban host.
 */
export const ForgeError = {
  /**
   * The caller is not permitted to perform this action.
   */
  1: {message:"Unauthorized"},
  /**
   * The requested entity (escrow, proposal, subscription, ...) does not exist.
   */
  2: {message:"NotFound"},
  /**
   * One or more arguments failed validation (e.g. zero amount, bad address).
   */
  3: {message:"InvalidInput"},
  /**
   * The contract does not hold enough balance to satisfy the operation.
   */
  4: {message:"InsufficientFunds"},
  /**
   * The entity was already initialised; re-initialisation is rejected.
   */
  5: {message:"AlreadyInitialized"},
  /**
   * The entity was expected to be initialised but was not.
   */
  6: {message:"NotInitialized"},
  /**
   * An operation was attempted after its deadline elapsed.
   */
  7: {message:"DeadlineReached"},
  /**
   * A required token allowance was lower than the amount being spent.
   */
  8: {message:"InsufficientAllowance"},
  /**
   * An arithmetic operation overflowed.
   */
  9: {message:"ArithmeticOverflow"},
  /**
   * Contract-specific error that does not map to the categories above.
   */
  10: {message:"Custom"},
  /**
   * A SEP-41 token invocation failed (insufficient balance, missing or
   * deauthorized trustline, undeployed token contract, or token-logic
   * rejection). The raw token error discriminant is intentionally not
   * forwarded — callers cannot tell which contract produced a forwarded
   * code, so it is bucketed; the root cause remains visible in the
   * transaction's diagnostic events.
   */
  11: {message:"TokenTransferFailed"},
  /**
   * A cross-contract invocation failed (target reverted or host
   * abort). The target reverts are surfaced here so the caller can
   * distinguish them from token-transfer failures, and the invoking
   * transaction is left un-executed.
   */
  12: {message:"ContractInvocationFailed"},
  /**
   * The requested withdrawal would push a token's rolling-window total
   * past its configured withdrawal limit. Kept distinct from
   * [`ForgeError::InvalidInput`] so a caller can tell a policy rejection
   * (a valid withdrawal that is too large right now) from a malformed
   * argument.
   */
  13: {message:"WithdrawalLimitExceeded"}
}


/**
 * Audit metadata attached to a persisted value.
 * 
 * Contracts store domain data in Soroban instance storage; wrapping it with
 * this record lets callers (and off-chain indexers) see when a value was last
 * written. The payload is stored as opaque serialized bytes so the record is
 * valid as a Soroban contract type without coupling it to a concrete domain
 * value.
 */
export interface StorageEntry {
  /**
 * Unix timestamp (seconds) at which the entry was last written.
 */
updated_at: u64;
  /**
 * The stored payload, serialized as opaque Soroban bytes.
 */
value: Buffer;
}

export interface Client {
  /**
   * Construct and simulate a pause transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Pause an active subscription, preventing charges while paused.
   * 
   * Requires the subscriber. Only valid when `Active`.
   */
  pause: ({subscription_id}: {subscription_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a cancel transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Cancel a subscription, preventing further charges.
   * 
   * Requires the subscriber. Valid when `Active`, `Paused`, or `PastDue`. Cancelling
   * an already-cancelled subscription is rejected.
   */
  cancel: ({subscription_id}: {subscription_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a charge transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Bill one due period.
   * 
   * Requires the provider. If a full period has not elapsed since the last
   * charge, returns `0` and leaves the subscription untouched. Otherwise
   * attempts to transfer `amount` of `token` from `subscriber` to `provider`.
   * 
   * - On successful payment: advances `last_charged` by one period, resets
   * `failed_attempts` to 0, transitions status to `Active`, and returns `amount`.
   * - On failed payment: `last_charged` is NOT advanced. Increments `failed_attempts`.
   * If `failed_attempts >= MAX_RETRIES` (3), status becomes `Cancelled`.
   * Otherwise status becomes `PastDue`. Returns `0`.
   */
  charge: ({subscription_id}: {subscription_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a resume transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Resume a paused subscription, advancing the next due date by the
   * elapsed paused duration so that paused periods are not billed.
   * 
   * Requires the subscriber. Only valid when `Paused`.
   */
  resume: ({subscription_id}: {subscription_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a subscribe transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Create a new subscription and return its stable id.
   * 
   * Requires `amount > 0` and `period > 0`. The subscriber is authorized at
   * creation time; billing starts from the moment of subscription.
   */
  subscribe: ({subscriber, provider, token, amount, period}: {subscriber: string, provider: string, token: string, amount: i128, period: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a revoke_provider transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Withdraw `subscriber`'s explicit authorization of `provider` (see the
   * module docs on provider opt-in).
   * 
   * Requires the subscriber. Idempotent: revoking a provider that is not
   * authorized succeeds and leaves the opt-in absent.
   */
  revoke_provider: ({subscriber, provider}: {subscriber: string, provider: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_subscription transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read a stored subscription by id (read-only view).
   */
  get_subscription: ({subscription_id}: {subscription_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Subscription>>>

  /**
   * Construct and simulate a authorize_provider transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Explicitly authorize `provider` to create subscriptions on
   * `subscriber`'s behalf (see the module docs on provider opt-in).
   * 
   * Requires the subscriber. Idempotent: authorizing a provider that is
   * already authorized succeeds.
   */
  authorize_provider: ({subscriber, provider}: {subscriber: string, provider: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_subscription_count transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Total number of subscriptions created so far (read-only view).
   * 
   * This is the monotonic id counter, which only `subscribe` advances, so
   * it never decreases and equals the number of live ids returned by the
   * enumeration views.
   */
  get_subscription_count: (options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a is_provider_authorized transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read whether `subscriber` has explicitly authorized `provider`
   * (read-only view; requires no authorization).
   */
  is_provider_authorized: ({subscriber, provider}: {subscriber: string, provider: string}, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a subscribe_on_behalf_of transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Subscribe `subscriber` to `provider`'s service on the provider's
   * initiative (see the module docs on provider opt-in).
   * 
   * Requires `amount > 0`, `period > 0`, the provider's authorization, and
   * an explicit subscriber opt-in for the provider (checked before the
   * provider is authorized, so a missing opt-in surfaces without spending
   * the provider's signature). The subscriber is **not** authorized at
   * creation. The record is created through the same internal path as
   * [`subscribe`], sharing the same sequential id counter.
   */
  subscribe_on_behalf_of: ({provider, subscriber, token, amount, period}: {provider: string, subscriber: string, token: string, amount: i128, period: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a subscriptions_for_provider transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * List `provider`'s subscriptions in creation order, one page at a time
   * (read-only view).
   * 
   * Requires no authorization and never mutates storage. An address with
   * no subscriptions — or an offset at or past the end of its list —
   * returns an empty `Vec`, not an error, so clients can back "my
   * subscribers" views without an off-chain indexer.
   * 
   * # Errors
   * 
   * * [`ForgeError::InvalidInput`] — `limit` is zero.
   */
  subscriptions_for_provider: ({provider, offset, limit}: {provider: string, offset: u32, limit: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<Subscription>>>>

  /**
   * Construct and simulate a subscriptions_for_subscriber transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * List `subscriber`'s subscriptions in creation order, one page at a
   * time (read-only view).
   * 
   * Requires no authorization and never mutates storage. An address with
   * no subscriptions — or an offset at or past the end of its list —
   * returns an empty `Vec`, not an error, so clients can back "my
   * subscriptions" views without an off-chain indexer.
   * 
   * # Errors
   * 
   * * [`ForgeError::InvalidInput`] — `limit` is zero.
   */
  subscriptions_for_subscriber: ({subscriber, offset, limit}: {subscriber: string, offset: u32, limit: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<Subscription>>>>

}
export class Client extends ContractClient {
  static async deploy<T = Client>(
    /** Options for initializing a Client as well as for calling a method, with extras specific to deploying. */
    options: MethodOptions &
      Omit<ContractClientOptions, "contractId"> & {
        /** The hash of the Wasm blob, which must already be installed on-chain. */
        wasmHash: Buffer | string;
        /** Salt used to generate the contract's ID. Passed through to {@link Operation.createCustomContract}. Default: random. */
        salt?: Buffer | Uint8Array;
        /** The format used to decode `wasmHash`, if it's provided as a string. */
        format?: "hex" | "base64";
      }
  ): Promise<AssembledTransaction<T>> {
    return ContractClient.deploy(null, options)
  }
  constructor(public readonly options: ContractClientOptions) {
    super(
      new ContractSpec([ "AAAAAQAAAB5BIHJlY3VycmluZyBwYXltZW50IGFncmVlbWVudC4AAAAAAAAAAAAMU3Vic2NyaXB0aW9uAAAACgAAABpBbW91bnQgY2hhcmdlZCBwZXIgcGVyaW9kLgAAAAAABmFtb3VudAAAAAAACwAAAC5OdW1iZXIgb2YgY29uc2VjdXRpdmUgZmFpbGVkIGJpbGxpbmcgYXR0ZW1wdHMuAAAAAAAPZmFpbGVkX2F0dGVtcHRzAAAAAAQAAAAvTGVkZ2VyIHRpbWVzdGFtcCBvZiB0aGUgbGFzdCBzdWNjZXNzZnVsIGNoYXJnZS4AAAAADGxhc3RfY2hhcmdlZAAAAAYAAAAyTGVkZ2VyIHRpbWVzdGFtcCB3aGVuIHBhdXNlZCwgaWYgY3VycmVudGx5IHBhdXNlZC4AAAAAAAlwYXVzZWRfYXQAAAAAAAPoAAAABgAAAClMZW5ndGggb2Ygb25lIGJpbGxpbmcgcGVyaW9kLCBpbiBzZWNvbmRzLgAAAAAAAAZwZXJpb2QAAAAAAAYAAAAbQWNjb3VudCByZWNlaXZpbmcgcGF5bWVudHMuAAAAAAhwcm92aWRlcgAAABMAAAAOQ3VycmVudCBzdGF0ZS4AAAAAAAZzdGF0dXMAAAAAB9AAAAASU3Vic2NyaXB0aW9uU3RhdHVzAAAAAAAWQWNjb3VudCBiZWluZyBjaGFyZ2VkLgAAAAAACnN1YnNjcmliZXIAAAAAABMAAAAnU3RhYmxlIGlkZW50aWZpZXIgYXNzaWduZWQgYXQgY3JlYXRpb24uAAAAAA9zdWJzY3JpcHRpb25faWQAAAAABgAAACNUb2tlbiBjb250cmFjdCB1c2VkIGZvciBzZXR0bGVtZW50LgAAAAAFdG9rZW4AAAAAAAAT",
        "AAAAAgAAACJMaWZlY3ljbGUgc3RhdGUgb2YgYSBzdWJzY3JpcHRpb24uAAAAAAAAAAAAElN1YnNjcmlwdGlvblN0YXR1cwAAAAAABAAAAAAAAAAWQWN0aXZlIGFuZCBjaGFyZ2VhYmxlLgAAAAAABkFjdGl2ZQAAAAAAAAAAAB5DYW5jZWxsZWQ7IG5vIGZ1cnRoZXIgY2hhcmdlcy4AAAAAAAlDYW5jZWxsZWQAAAAAAAAAAAAAMlBheW1lbnQgZmFpbGVkIGFuZCB0aGUgc3Vic2NyaXB0aW9uIGlzIGluIGFycmVhcnMuAAAAAAAHUGFzdER1ZQAAAAAAAAAAOVRlbXBvcmFyaWx5IHBhdXNlZDsgbm8gY2hhcmdlcyBjYW4gYmUgbWFkZSB1bnRpbCByZXN1bWVkLgAAAAAAAAZQYXVzZWQAAA==",
        "AAAAAAAAAHJQYXVzZSBhbiBhY3RpdmUgc3Vic2NyaXB0aW9uLCBwcmV2ZW50aW5nIGNoYXJnZXMgd2hpbGUgcGF1c2VkLgoKUmVxdWlyZXMgdGhlIHN1YnNjcmliZXIuIE9ubHkgdmFsaWQgd2hlbiBgQWN0aXZlYC4AAAAAAAVwYXVzZQAAAAAAAAEAAAAAAAAAD3N1YnNjcmlwdGlvbl9pZAAAAAAGAAAAAQAAA+kAAAACAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAALNDYW5jZWwgYSBzdWJzY3JpcHRpb24sIHByZXZlbnRpbmcgZnVydGhlciBjaGFyZ2VzLgoKUmVxdWlyZXMgdGhlIHN1YnNjcmliZXIuIFZhbGlkIHdoZW4gYEFjdGl2ZWAsIGBQYXVzZWRgLCBvciBgUGFzdER1ZWAuIENhbmNlbGxpbmcKYW4gYWxyZWFkeS1jYW5jZWxsZWQgc3Vic2NyaXB0aW9uIGlzIHJlamVjdGVkLgAAAAAGY2FuY2VsAAAAAAABAAAAAAAAAA9zdWJzY3JpcHRpb25faWQAAAAABgAAAAEAAAPpAAAAAgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAkpCaWxsIG9uZSBkdWUgcGVyaW9kLgoKUmVxdWlyZXMgdGhlIHByb3ZpZGVyLiBJZiBhIGZ1bGwgcGVyaW9kIGhhcyBub3QgZWxhcHNlZCBzaW5jZSB0aGUgbGFzdApjaGFyZ2UsIHJldHVybnMgYDBgIGFuZCBsZWF2ZXMgdGhlIHN1YnNjcmlwdGlvbiB1bnRvdWNoZWQuIE90aGVyd2lzZQphdHRlbXB0cyB0byB0cmFuc2ZlciBgYW1vdW50YCBvZiBgdG9rZW5gIGZyb20gYHN1YnNjcmliZXJgIHRvIGBwcm92aWRlcmAuCgotIE9uIHN1Y2Nlc3NmdWwgcGF5bWVudDogYWR2YW5jZXMgYGxhc3RfY2hhcmdlZGAgYnkgb25lIHBlcmlvZCwgcmVzZXRzCmBmYWlsZWRfYXR0ZW1wdHNgIHRvIDAsIHRyYW5zaXRpb25zIHN0YXR1cyB0byBgQWN0aXZlYCwgYW5kIHJldHVybnMgYGFtb3VudGAuCi0gT24gZmFpbGVkIHBheW1lbnQ6IGBsYXN0X2NoYXJnZWRgIGlzIE5PVCBhZHZhbmNlZC4gSW5jcmVtZW50cyBgZmFpbGVkX2F0dGVtcHRzYC4KSWYgYGZhaWxlZF9hdHRlbXB0cyA+PSBNQVhfUkVUUklFU2AgKDMpLCBzdGF0dXMgYmVjb21lcyBgQ2FuY2VsbGVkYC4KT3RoZXJ3aXNlIHN0YXR1cyBiZWNvbWVzIGBQYXN0RHVlYC4gUmV0dXJucyBgMGAuAAAAAAAGY2hhcmdlAAAAAAABAAAAAAAAAA9zdWJzY3JpcHRpb25faWQAAAAABgAAAAEAAAPpAAAACwAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAALNSZXN1bWUgYSBwYXVzZWQgc3Vic2NyaXB0aW9uLCBhZHZhbmNpbmcgdGhlIG5leHQgZHVlIGRhdGUgYnkgdGhlCmVsYXBzZWQgcGF1c2VkIGR1cmF0aW9uIHNvIHRoYXQgcGF1c2VkIHBlcmlvZHMgYXJlIG5vdCBiaWxsZWQuCgpSZXF1aXJlcyB0aGUgc3Vic2NyaWJlci4gT25seSB2YWxpZCB3aGVuIGBQYXVzZWRgLgAAAAAGcmVzdW1lAAAAAAABAAAAAAAAAA9zdWJzY3JpcHRpb25faWQAAAAABgAAAAEAAAPpAAAAAgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAALtDcmVhdGUgYSBuZXcgc3Vic2NyaXB0aW9uIGFuZCByZXR1cm4gaXRzIHN0YWJsZSBpZC4KClJlcXVpcmVzIGBhbW91bnQgPiAwYCBhbmQgYHBlcmlvZCA+IDBgLiBUaGUgc3Vic2NyaWJlciBpcyBhdXRob3JpemVkIGF0CmNyZWF0aW9uIHRpbWU7IGJpbGxpbmcgc3RhcnRzIGZyb20gdGhlIG1vbWVudCBvZiBzdWJzY3JpcHRpb24uAAAAAAlzdWJzY3JpYmUAAAAAAAAFAAAAAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAAAAAAhwcm92aWRlcgAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAAAAAAZwZXJpb2QAAAAAAAYAAAABAAAD6QAAAAYAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAAN5XaXRoZHJhdyBgc3Vic2NyaWJlcmAncyBleHBsaWNpdCBhdXRob3JpemF0aW9uIG9mIGBwcm92aWRlcmAgKHNlZSB0aGUKbW9kdWxlIGRvY3Mgb24gcHJvdmlkZXIgb3B0LWluKS4KClJlcXVpcmVzIHRoZSBzdWJzY3JpYmVyLiBJZGVtcG90ZW50OiByZXZva2luZyBhIHByb3ZpZGVyIHRoYXQgaXMgbm90CmF1dGhvcml6ZWQgc3VjY2VlZHMgYW5kIGxlYXZlcyB0aGUgb3B0LWluIGFic2VudC4AAAAAAA9yZXZva2VfcHJvdmlkZXIAAAAAAgAAAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAIcHJvdmlkZXIAAAATAAAAAQAAA+kAAAACAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAADJSZWFkIGEgc3RvcmVkIHN1YnNjcmlwdGlvbiBieSBpZCAocmVhZC1vbmx5IHZpZXcpLgAAAAAAEGdldF9zdWJzY3JpcHRpb24AAAABAAAAAAAAAA9zdWJzY3JpcHRpb25faWQAAAAABgAAAAEAAAPpAAAH0AAAAAxTdWJzY3JpcHRpb24AAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAANxFeHBsaWNpdGx5IGF1dGhvcml6ZSBgcHJvdmlkZXJgIHRvIGNyZWF0ZSBzdWJzY3JpcHRpb25zIG9uCmBzdWJzY3JpYmVyYCdzIGJlaGFsZiAoc2VlIHRoZSBtb2R1bGUgZG9jcyBvbiBwcm92aWRlciBvcHQtaW4pLgoKUmVxdWlyZXMgdGhlIHN1YnNjcmliZXIuIElkZW1wb3RlbnQ6IGF1dGhvcml6aW5nIGEgcHJvdmlkZXIgdGhhdCBpcwphbHJlYWR5IGF1dGhvcml6ZWQgc3VjY2VlZHMuAAAAEmF1dGhvcml6ZV9wcm92aWRlcgAAAAAAAgAAAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAIcHJvdmlkZXIAAAATAAAAAQAAA+kAAAACAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAN1Ub3RhbCBudW1iZXIgb2Ygc3Vic2NyaXB0aW9ucyBjcmVhdGVkIHNvIGZhciAocmVhZC1vbmx5IHZpZXcpLgoKVGhpcyBpcyB0aGUgbW9ub3RvbmljIGlkIGNvdW50ZXIsIHdoaWNoIG9ubHkgYHN1YnNjcmliZWAgYWR2YW5jZXMsIHNvCml0IG5ldmVyIGRlY3JlYXNlcyBhbmQgZXF1YWxzIHRoZSBudW1iZXIgb2YgbGl2ZSBpZHMgcmV0dXJuZWQgYnkgdGhlCmVudW1lcmF0aW9uIHZpZXdzLgAAAAAAABZnZXRfc3Vic2NyaXB0aW9uX2NvdW50AAAAAAAAAAAAAQAAAAY=",
        "AAAAAAAAAGtSZWFkIHdoZXRoZXIgYHN1YnNjcmliZXJgIGhhcyBleHBsaWNpdGx5IGF1dGhvcml6ZWQgYHByb3ZpZGVyYAoocmVhZC1vbmx5IHZpZXc7IHJlcXVpcmVzIG5vIGF1dGhvcml6YXRpb24pLgAAAAAWaXNfcHJvdmlkZXJfYXV0aG9yaXplZAAAAAAAAgAAAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAIcHJvdmlkZXIAAAATAAAAAQAAAAE=",
        "AAAAAAAAAgJTdWJzY3JpYmUgYHN1YnNjcmliZXJgIHRvIGBwcm92aWRlcmAncyBzZXJ2aWNlIG9uIHRoZSBwcm92aWRlcidzCmluaXRpYXRpdmUgKHNlZSB0aGUgbW9kdWxlIGRvY3Mgb24gcHJvdmlkZXIgb3B0LWluKS4KClJlcXVpcmVzIGBhbW91bnQgPiAwYCwgYHBlcmlvZCA+IDBgLCB0aGUgcHJvdmlkZXIncyBhdXRob3JpemF0aW9uLCBhbmQKYW4gZXhwbGljaXQgc3Vic2NyaWJlciBvcHQtaW4gZm9yIHRoZSBwcm92aWRlciAoY2hlY2tlZCBiZWZvcmUgdGhlCnByb3ZpZGVyIGlzIGF1dGhvcml6ZWQsIHNvIGEgbWlzc2luZyBvcHQtaW4gc3VyZmFjZXMgd2l0aG91dCBzcGVuZGluZwp0aGUgcHJvdmlkZXIncyBzaWduYXR1cmUpLiBUaGUgc3Vic2NyaWJlciBpcyAqKm5vdCoqIGF1dGhvcml6ZWQgYXQKY3JlYXRpb24uIFRoZSByZWNvcmQgaXMgY3JlYXRlZCB0aHJvdWdoIHRoZSBzYW1lIGludGVybmFsIHBhdGggYXMKW2BzdWJzY3JpYmVgXSwgc2hhcmluZyB0aGUgc2FtZSBzZXF1ZW50aWFsIGlkIGNvdW50ZXIuAAAAAAAWc3Vic2NyaWJlX29uX2JlaGFsZl9vZgAAAAAABQAAAAAAAAAIcHJvdmlkZXIAAAATAAAAAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAAAAAAGcGVyaW9kAAAAAAAGAAAAAQAAA+kAAAAGAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAZBMaXN0IGBwcm92aWRlcmAncyBzdWJzY3JpcHRpb25zIGluIGNyZWF0aW9uIG9yZGVyLCBvbmUgcGFnZSBhdCBhIHRpbWUKKHJlYWQtb25seSB2aWV3KS4KClJlcXVpcmVzIG5vIGF1dGhvcml6YXRpb24gYW5kIG5ldmVyIG11dGF0ZXMgc3RvcmFnZS4gQW4gYWRkcmVzcyB3aXRoCm5vIHN1YnNjcmlwdGlvbnMg4oCUIG9yIGFuIG9mZnNldCBhdCBvciBwYXN0IHRoZSBlbmQgb2YgaXRzIGxpc3Qg4oCUCnJldHVybnMgYW4gZW1wdHkgYFZlY2AsIG5vdCBhbiBlcnJvciwgc28gY2xpZW50cyBjYW4gYmFjayAibXkKc3Vic2NyaWJlcnMiIHZpZXdzIHdpdGhvdXQgYW4gb2ZmLWNoYWluIGluZGV4ZXIuCgojIEVycm9ycwoKKiBbYEZvcmdlRXJyb3I6OkludmFsaWRJbnB1dGBdIOKAlCBgbGltaXRgIGlzIHplcm8uAAAAGnN1YnNjcmlwdGlvbnNfZm9yX3Byb3ZpZGVyAAAAAAADAAAAAAAAAAhwcm92aWRlcgAAABMAAAAAAAAABm9mZnNldAAAAAAABAAAAAAAAAAFbGltaXQAAAAAAAAEAAAAAQAAA+kAAAPqAAAH0AAAAAxTdWJzY3JpcHRpb24AAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAAZRMaXN0IGBzdWJzY3JpYmVyYCdzIHN1YnNjcmlwdGlvbnMgaW4gY3JlYXRpb24gb3JkZXIsIG9uZSBwYWdlIGF0IGEKdGltZSAocmVhZC1vbmx5IHZpZXcpLgoKUmVxdWlyZXMgbm8gYXV0aG9yaXphdGlvbiBhbmQgbmV2ZXIgbXV0YXRlcyBzdG9yYWdlLiBBbiBhZGRyZXNzIHdpdGgKbm8gc3Vic2NyaXB0aW9ucyDigJQgb3IgYW4gb2Zmc2V0IGF0IG9yIHBhc3QgdGhlIGVuZCBvZiBpdHMgbGlzdCDigJQKcmV0dXJucyBhbiBlbXB0eSBgVmVjYCwgbm90IGFuIGVycm9yLCBzbyBjbGllbnRzIGNhbiBiYWNrICJteQpzdWJzY3JpcHRpb25zIiB2aWV3cyB3aXRob3V0IGFuIG9mZi1jaGFpbiBpbmRleGVyLgoKIyBFcnJvcnMKCiogW2BGb3JnZUVycm9yOjpJbnZhbGlkSW5wdXRgXSDigJQgYGxpbWl0YCBpcyB6ZXJvLgAAABxzdWJzY3JpcHRpb25zX2Zvcl9zdWJzY3JpYmVyAAAAAwAAAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAGb2Zmc2V0AAAAAAAEAAAAAAAAAAVsaW1pdAAAAAAAAAQAAAABAAAD6QAAA+oAAAfQAAAADFN1YnNjcmlwdGlvbgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAQAAAD5BIHBhcnRpY2lwYW50IGluIGEgbXVsdGktcGFydHkgZmxvdyAoZXNjcm93LCBnb3Zlcm5hbmNlLCAuLi4pLgAAAAAAAAAAAAVQYXJ0eQAAAAAAAAMAAAAkT24tY2hhaW4gYWRkcmVzcyBvZiB0aGUgcGFydGljaXBhbnQuAAAAB2FkZHJlc3MAAAAAEwAAAD9XaGV0aGVyIHRoaXMgcGFydHkgaGFzIGdyYW50ZWQgYXBwcm92YWwgZm9yIHRoZSBjdXJyZW50IGFjdGlvbi4AAAAACGFwcHJvdmVkAAAAAQAAADlIdW1hbi1yZWFkYWJsZSByb2xlIGxhYmVsLCBlLmcuIGAiYnV5ZXIiYCBvciBgImFyYml0ZXIiYC4AAAAAAAAEcm9sZQAAABA=",
        "AAAAAQAAALtJbmNsdXNpdmUgdGltZSB3aW5kb3cgZXhwcmVzc2VkIGFzIFVuaXggdGltZXN0YW1wcyAoc2Vjb25kcykuCgpTdG9yZWQgYXMgcGxhaW4gYHU2NGAgYmVjYXVzZSBgc29yb2Jhbl9zZGtgIG1vZGVscyB0aW1lIGFzIGB1NjRgOyBhCmRlZGljYXRlZCBuZXd0eXBlIHdvdWxkIGFkZCBjb252ZXJzaW9ucyB3aXRob3V0IGJlbmVmaXQuAAAAAAAAAAAKVGltZUJvdW5kcwAAAAAAAgAAADhMYXRlc3QgbW9tZW50IChpbmNsdXNpdmUpIGF0IHdoaWNoIHRoZSB3aW5kb3cgaXMgYWN0aXZlLgAAAANlbmQAAAAABgAAADpFYXJsaWVzdCBtb21lbnQgKGluY2x1c2l2ZSkgYXQgd2hpY2ggdGhlIHdpbmRvdyBpcyBhY3RpdmUuAAAAAAAFc3RhcnQAAAAAAAAG",
        "AAAAAQAAAUBBIHBhZ2Ugb2YgcmVzdWx0cyBwbHVzIHRoZSBjdXJzb3IgbmVlZGVkIHRvIGZldGNoIHRoZSBuZXh0IHBhZ2UuCgpJdGVtcyBhcmUgc3RvcmVkIGFzIHNlcmlhbGl6ZWQgYEJ5dGVzYCBzbyB0aGUgaGVscGVyIGlzIGFnbm9zdGljIHRvIHRoZQpjb25jcmV0ZSB2YWx1ZSB0eXBlIGEgY29udHJhY3QgcGFnaW5hdGVzLiBDYWxsZXJzIGRlY29kZSBlYWNoIGl0ZW0gaW50bwp0aGVpciBkb21haW4gdHlwZS4gYERlYnVnYCBpcyBvbWl0dGVkIGJlY2F1c2UgdGhlIFNESyBjb2xsZWN0aW9uIGRvZXMgbm90CmltcGxlbWVudCBpdCBmb3IgdGhpcyBjb250cmFjdCB0eXBlLgAAAAAAAAAPUGFnaW5hdGVkUmVzdWx0AAAAAAMAAAA9Q3Vyc29yIGRlc2NyaWJpbmcgdGhlIG5leHQgcGFnZSAob2Zmc2V0IGFkdmFuY2VkIGJ5IGBsaW1pdGApLgAAAAAAAAZjdXJzb3IAAAAAB9AAAAAQUGFnaW5hdGlvbkN1cnNvcgAAABNJdGVtcyBvbiB0aGlzIHBhZ2UuAAAAAAVpdGVtcwAAAAAAA+oAAAAOAAAAJ1RvdGFsIG51bWJlciBvZiBpdGVtcyBhY3Jvc3MgYWxsIHBhZ2VzLgAAAAAFdG90YWwAAAAAAAAE",
        "AAAAAQAAACZPZmZzZXQvbGltaXQgcGFpciBmb3IgcGFnaW5hdGVkIHJlYWRzLgAAAAAAAAAAABBQYWdpbmF0aW9uQ3Vyc29yAAAAAgAAACJNYXhpbXVtIG51bWJlciBvZiBpdGVtcyB0byByZXR1cm4uAAAAAAAFbGltaXQAAAAAAAAEAAAAPk51bWJlciBvZiBpdGVtcyB0byBza2lwIGZyb20gdGhlIHN0YXJ0IG9mIHRoZSBmdWxsIHJlc3VsdCBzZXQuAAAAAAAGb2Zmc2V0AAAAAAAE",
        "AAAABAAAAZxTaGFyZWQgZXJyb3IgdHlwZSB1c2VkIGFjcm9zcyBhbGwgU29yb2JhbiBGb3JnZSBjb250cmFjdHMuCgpEZWZpbmluZyBhIHNpbmdsZSBlcnJvciBlbnVtIGluIGBzaGFyZWQtdXRpbHNgIGtlZXBzIHRoZSBvbi1jaGFpbiBlcnJvcgpzcGFjZSBjb25zaXN0ZW50IGFuZCBpbnRlbGxpZ2libGUgdG8gU0RLIGNvbnN1bWVycywgYW5kIGF2b2lkcyBldmVyeQpjb250cmFjdCByZS1kZWNsYXJpbmcgdGhlIHNhbWUgZmFpbHVyZSBtb2Rlcy4gQ29udHJhY3QgY3JhdGVzIG1heSBleHBvc2UKdGhlaXIgb3duIGRvbWFpbi1zcGVjaWZpYyBlcnJvcnMsIGJ1dCBzaG91bGQgcHJlZmVyIHRoZXNlIHdoZXJlIHRoZXkgZml0LgoKRXJyb3IgY29kZXMgc3RhcnQgYXQgMTsgY29kZSAwIGlzIHJlc2VydmVkIGJ5IHRoZSBTb3JvYmFuIGhvc3QuAAAAAAAAAApGb3JnZUVycm9yAAAAAAANAAAAM1RoZSBjYWxsZXIgaXMgbm90IHBlcm1pdHRlZCB0byBwZXJmb3JtIHRoaXMgYWN0aW9uLgAAAAAMVW5hdXRob3JpemVkAAAAAQAAAEpUaGUgcmVxdWVzdGVkIGVudGl0eSAoZXNjcm93LCBwcm9wb3NhbCwgc3Vic2NyaXB0aW9uLCAuLi4pIGRvZXMgbm90IGV4aXN0LgAAAAAACE5vdEZvdW5kAAAAAgAAAEhPbmUgb3IgbW9yZSBhcmd1bWVudHMgZmFpbGVkIHZhbGlkYXRpb24gKGUuZy4gemVybyBhbW91bnQsIGJhZCBhZGRyZXNzKS4AAAAMSW52YWxpZElucHV0AAAAAwAAAENUaGUgY29udHJhY3QgZG9lcyBub3QgaG9sZCBlbm91Z2ggYmFsYW5jZSB0byBzYXRpc2Z5IHRoZSBvcGVyYXRpb24uAAAAABFJbnN1ZmZpY2llbnRGdW5kcwAAAAAAAAQAAABCVGhlIGVudGl0eSB3YXMgYWxyZWFkeSBpbml0aWFsaXNlZDsgcmUtaW5pdGlhbGlzYXRpb24gaXMgcmVqZWN0ZWQuAAAAAAASQWxyZWFkeUluaXRpYWxpemVkAAAAAAAFAAAANlRoZSBlbnRpdHkgd2FzIGV4cGVjdGVkIHRvIGJlIGluaXRpYWxpc2VkIGJ1dCB3YXMgbm90LgAAAAAADk5vdEluaXRpYWxpemVkAAAAAAAGAAAANkFuIG9wZXJhdGlvbiB3YXMgYXR0ZW1wdGVkIGFmdGVyIGl0cyBkZWFkbGluZSBlbGFwc2VkLgAAAAAAD0RlYWRsaW5lUmVhY2hlZAAAAAAHAAAAQUEgcmVxdWlyZWQgdG9rZW4gYWxsb3dhbmNlIHdhcyBsb3dlciB0aGFuIHRoZSBhbW91bnQgYmVpbmcgc3BlbnQuAAAAAAAAFUluc3VmZmljaWVudEFsbG93YW5jZQAAAAAAAAgAAAAjQW4gYXJpdGhtZXRpYyBvcGVyYXRpb24gb3ZlcmZsb3dlZC4AAAAAEkFyaXRobWV0aWNPdmVyZmxvdwAAAAAACQAAAEJDb250cmFjdC1zcGVjaWZpYyBlcnJvciB0aGF0IGRvZXMgbm90IG1hcCB0byB0aGUgY2F0ZWdvcmllcyBhYm92ZS4AAAAAAAZDdXN0b20AAAAAAAoAAAFsQSBTRVAtNDEgdG9rZW4gaW52b2NhdGlvbiBmYWlsZWQgKGluc3VmZmljaWVudCBiYWxhbmNlLCBtaXNzaW5nIG9yCmRlYXV0aG9yaXplZCB0cnVzdGxpbmUsIHVuZGVwbG95ZWQgdG9rZW4gY29udHJhY3QsIG9yIHRva2VuLWxvZ2ljCnJlamVjdGlvbikuIFRoZSByYXcgdG9rZW4gZXJyb3IgZGlzY3JpbWluYW50IGlzIGludGVudGlvbmFsbHkgbm90CmZvcndhcmRlZCDigJQgY2FsbGVycyBjYW5ub3QgdGVsbCB3aGljaCBjb250cmFjdCBwcm9kdWNlZCBhIGZvcndhcmRlZApjb2RlLCBzbyBpdCBpcyBidWNrZXRlZDsgdGhlIHJvb3QgY2F1c2UgcmVtYWlucyB2aXNpYmxlIGluIHRoZQp0cmFuc2FjdGlvbidzIGRpYWdub3N0aWMgZXZlbnRzLgAAABNUb2tlblRyYW5zZmVyRmFpbGVkAAAAAAsAAADbQSBjcm9zcy1jb250cmFjdCBpbnZvY2F0aW9uIGZhaWxlZCAodGFyZ2V0IHJldmVydGVkIG9yIGhvc3QKYWJvcnQpLiBUaGUgdGFyZ2V0IHJldmVydHMgYXJlIHN1cmZhY2VkIGhlcmUgc28gdGhlIGNhbGxlciBjYW4KZGlzdGluZ3Vpc2ggdGhlbSBmcm9tIHRva2VuLXRyYW5zZmVyIGZhaWx1cmVzLCBhbmQgdGhlIGludm9raW5nCnRyYW5zYWN0aW9uIGlzIGxlZnQgdW4tZXhlY3V0ZWQuAAAAABhDb250cmFjdEludm9jYXRpb25GYWlsZWQAAAAMAAABDFRoZSByZXF1ZXN0ZWQgd2l0aGRyYXdhbCB3b3VsZCBwdXNoIGEgdG9rZW4ncyByb2xsaW5nLXdpbmRvdyB0b3RhbApwYXN0IGl0cyBjb25maWd1cmVkIHdpdGhkcmF3YWwgbGltaXQuIEtlcHQgZGlzdGluY3QgZnJvbQpbYEZvcmdlRXJyb3I6OkludmFsaWRJbnB1dGBdIHNvIGEgY2FsbGVyIGNhbiB0ZWxsIGEgcG9saWN5IHJlamVjdGlvbgooYSB2YWxpZCB3aXRoZHJhd2FsIHRoYXQgaXMgdG9vIGxhcmdlIHJpZ2h0IG5vdykgZnJvbSBhIG1hbGZvcm1lZAphcmd1bWVudC4AAAAXV2l0aGRyYXdhbExpbWl0RXhjZWVkZWQAAAAADQ==",
        "AAAAAQAAAWBBdWRpdCBtZXRhZGF0YSBhdHRhY2hlZCB0byBhIHBlcnNpc3RlZCB2YWx1ZS4KCkNvbnRyYWN0cyBzdG9yZSBkb21haW4gZGF0YSBpbiBTb3JvYmFuIGluc3RhbmNlIHN0b3JhZ2U7IHdyYXBwaW5nIGl0IHdpdGgKdGhpcyByZWNvcmQgbGV0cyBjYWxsZXJzIChhbmQgb2ZmLWNoYWluIGluZGV4ZXJzKSBzZWUgd2hlbiBhIHZhbHVlIHdhcyBsYXN0CndyaXR0ZW4uIFRoZSBwYXlsb2FkIGlzIHN0b3JlZCBhcyBvcGFxdWUgc2VyaWFsaXplZCBieXRlcyBzbyB0aGUgcmVjb3JkIGlzCnZhbGlkIGFzIGEgU29yb2JhbiBjb250cmFjdCB0eXBlIHdpdGhvdXQgY291cGxpbmcgaXQgdG8gYSBjb25jcmV0ZSBkb21haW4KdmFsdWUuAAAAAAAAAAxTdG9yYWdlRW50cnkAAAACAAAAPVVuaXggdGltZXN0YW1wIChzZWNvbmRzKSBhdCB3aGljaCB0aGUgZW50cnkgd2FzIGxhc3Qgd3JpdHRlbi4AAAAAAAAKdXBkYXRlZF9hdAAAAAAABgAAADdUaGUgc3RvcmVkIHBheWxvYWQsIHNlcmlhbGl6ZWQgYXMgb3BhcXVlIFNvcm9iYW4gYnl0ZXMuAAAAAAV2YWx1ZQAAAAAAAA4=" ]),
      options
    )
  }
  public readonly fromJSON = {
    pause: this.txFromJSON<Result<void>>,
        cancel: this.txFromJSON<Result<void>>,
        charge: this.txFromJSON<Result<i128>>,
        resume: this.txFromJSON<Result<void>>,
        subscribe: this.txFromJSON<Result<u64>>,
        revoke_provider: this.txFromJSON<Result<void>>,
        get_subscription: this.txFromJSON<Result<Subscription>>,
        authorize_provider: this.txFromJSON<Result<void>>,
        get_subscription_count: this.txFromJSON<u64>,
        is_provider_authorized: this.txFromJSON<boolean>,
        subscribe_on_behalf_of: this.txFromJSON<Result<u64>>,
        subscriptions_for_provider: this.txFromJSON<Result<Array<Subscription>>>,
        subscriptions_for_subscriber: this.txFromJSON<Result<Array<Subscription>>>
  }
}