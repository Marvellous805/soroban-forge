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
 * Lifecycle state of a vesting schedule.
 */
export type VestingStatus = {tag: "Locked", values: void} | {tag: "Vesting", values: void} | {tag: "Completed", values: void} | {tag: "Revoked", values: void};


/**
 * A single token-vesting schedule.
 */
export interface VestingSchedule {
  /**
 * Recipient of the vested tokens.
 */
beneficiary: string;
  /**
 * Amount already claimed by the beneficiary.
 */
claimed: i128;
  /**
 * Seconds after `start` at which claims become possible.
 */
cliff: u64;
  /**
 * Seconds after `start` at which the schedule is fully vested.
 */
duration: u64;
  /**
 * Ledger timestamp at which vesting begins (creation time).
 */
start: u64;
  /**
 * Current lifecycle state.
 */
status: VestingStatus;
  /**
 * Token contract whose balance is drawn down.
 */
token: string;
  /**
 * Total amount to vest linearly between `cliff` and `duration`.
 */
total_amount: i128;
}


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
   * Construct and simulate a claim transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Claim the vested-but-unclaimed amount.
   * 
   * Requires the beneficiary. Returns exactly what vested since the last
   * claim (or `0` when nothing is claimable), so repeated claims can never
   * overpay or underpay.
   * 
   * Ordering: the SEP-41 transfer runs **before** the schedule write —
   * see the module docs. A zero-claim call returns before either.
   */
  claim: ({schedule_id}: {schedule_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a claimable transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Amount currently claimable (read-only view; no state change).
   */
  claimable: ({schedule_id}: {schedule_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a get_status transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the current lifecycle status (read-only view).
   * 
   * The status is derived from the ledger time and claimed amount rather
   * than the stored field, so it is always current between claims.
   */
  get_status: ({schedule_id}: {schedule_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<VestingStatus>>>

  /**
   * Construct and simulate a create_schedule transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Create a new vesting schedule and return its stable id.
   * 
   * Requires `total_amount > 0`, `duration > 0`, and `cliff <= duration`.
   * The beneficiary is authorized at creation time.
   */
  create_schedule: ({beneficiary, token, total_amount, cliff, duration}: {beneficiary: string, token: string, total_amount: i128, cliff: u64, duration: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

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
      new ContractSpec([ "AAAAAAAAAUxDbGFpbSB0aGUgdmVzdGVkLWJ1dC11bmNsYWltZWQgYW1vdW50LgoKUmVxdWlyZXMgdGhlIGJlbmVmaWNpYXJ5LiBSZXR1cm5zIGV4YWN0bHkgd2hhdCB2ZXN0ZWQgc2luY2UgdGhlIGxhc3QKY2xhaW0gKG9yIGAwYCB3aGVuIG5vdGhpbmcgaXMgY2xhaW1hYmxlKSwgc28gcmVwZWF0ZWQgY2xhaW1zIGNhbiBuZXZlcgpvdmVycGF5IG9yIHVuZGVycGF5LgoKT3JkZXJpbmc6IHRoZSBTRVAtNDEgdHJhbnNmZXIgcnVucyAqKmJlZm9yZSoqIHRoZSBzY2hlZHVsZSB3cml0ZSDigJQKc2VlIHRoZSBtb2R1bGUgZG9jcy4gQSB6ZXJvLWNsYWltIGNhbGwgcmV0dXJucyBiZWZvcmUgZWl0aGVyLgAAAAVjbGFpbQAAAAAAAAEAAAAAAAAAC3NjaGVkdWxlX2lkAAAAAAYAAAABAAAD6QAAAAsAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAAD1BbW91bnQgY3VycmVudGx5IGNsYWltYWJsZSAocmVhZC1vbmx5IHZpZXc7IG5vIHN0YXRlIGNoYW5nZSkuAAAAAAAACWNsYWltYWJsZQAAAAAAAAEAAAAAAAAAC3NjaGVkdWxlX2lkAAAAAAYAAAABAAAD6QAAAAsAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAALhSZWFkIHRoZSBjdXJyZW50IGxpZmVjeWNsZSBzdGF0dXMgKHJlYWQtb25seSB2aWV3KS4KClRoZSBzdGF0dXMgaXMgZGVyaXZlZCBmcm9tIHRoZSBsZWRnZXIgdGltZSBhbmQgY2xhaW1lZCBhbW91bnQgcmF0aGVyCnRoYW4gdGhlIHN0b3JlZCBmaWVsZCwgc28gaXQgaXMgYWx3YXlzIGN1cnJlbnQgYmV0d2VlbiBjbGFpbXMuAAAACmdldF9zdGF0dXMAAAAAAAEAAAAAAAAAC3NjaGVkdWxlX2lkAAAAAAYAAAABAAAD6QAAB9AAAAANVmVzdGluZ1N0YXR1cwAAAAAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAgAAACZMaWZlY3ljbGUgc3RhdGUgb2YgYSB2ZXN0aW5nIHNjaGVkdWxlLgAAAAAAAAAAAA1WZXN0aW5nU3RhdHVzAAAAAAAABAAAAAAAAAAiQmVmb3JlIHRoZSBjbGlmZiBoYXMgYmVlbiByZWFjaGVkLgAAAAAABkxvY2tlZAAAAAAAAAAAACxQYXN0IHRoZSBjbGlmZjsgdG9rZW5zIGFyZSB2ZXN0aW5nIGxpbmVhcmx5LgAAAAdWZXN0aW5nAAAAAAAAAAAZRnVsbHkgdmVzdGVkIGFuZCBjbGFpbWVkLgAAAAAAAAlDb21wbGV0ZWQAAAAAAAAAAAAANVNjaGVkdWxlIHdhcyB0ZXJtaW5hdGVkIGJlZm9yZSBjb21wbGV0aW9uIChyZXNlcnZlZCkuAAAAAAAAB1Jldm9rZWQA",
        "AAAAAQAAACBBIHNpbmdsZSB0b2tlbi12ZXN0aW5nIHNjaGVkdWxlLgAAAAAAAAAPVmVzdGluZ1NjaGVkdWxlAAAAAAgAAAAfUmVjaXBpZW50IG9mIHRoZSB2ZXN0ZWQgdG9rZW5zLgAAAAALYmVuZWZpY2lhcnkAAAAAEwAAACpBbW91bnQgYWxyZWFkeSBjbGFpbWVkIGJ5IHRoZSBiZW5lZmljaWFyeS4AAAAAAAdjbGFpbWVkAAAAAAsAAAA2U2Vjb25kcyBhZnRlciBgc3RhcnRgIGF0IHdoaWNoIGNsYWltcyBiZWNvbWUgcG9zc2libGUuAAAAAAAFY2xpZmYAAAAAAAAGAAAAPFNlY29uZHMgYWZ0ZXIgYHN0YXJ0YCBhdCB3aGljaCB0aGUgc2NoZWR1bGUgaXMgZnVsbHkgdmVzdGVkLgAAAAhkdXJhdGlvbgAAAAYAAAA5TGVkZ2VyIHRpbWVzdGFtcCBhdCB3aGljaCB2ZXN0aW5nIGJlZ2lucyAoY3JlYXRpb24gdGltZSkuAAAAAAAABXN0YXJ0AAAAAAAABgAAABhDdXJyZW50IGxpZmVjeWNsZSBzdGF0ZS4AAAAGc3RhdHVzAAAAAAfQAAAADVZlc3RpbmdTdGF0dXMAAAAAAAArVG9rZW4gY29udHJhY3Qgd2hvc2UgYmFsYW5jZSBpcyBkcmF3biBkb3duLgAAAAAFdG9rZW4AAAAAAAATAAAAPVRvdGFsIGFtb3VudCB0byB2ZXN0IGxpbmVhcmx5IGJldHdlZW4gYGNsaWZmYCBhbmQgYGR1cmF0aW9uYC4AAAAAAAAMdG90YWxfYW1vdW50AAAACw==",
        "AAAAAAAAAK5DcmVhdGUgYSBuZXcgdmVzdGluZyBzY2hlZHVsZSBhbmQgcmV0dXJuIGl0cyBzdGFibGUgaWQuCgpSZXF1aXJlcyBgdG90YWxfYW1vdW50ID4gMGAsIGBkdXJhdGlvbiA+IDBgLCBhbmQgYGNsaWZmIDw9IGR1cmF0aW9uYC4KVGhlIGJlbmVmaWNpYXJ5IGlzIGF1dGhvcml6ZWQgYXQgY3JlYXRpb24gdGltZS4AAAAAAA9jcmVhdGVfc2NoZWR1bGUAAAAABQAAAAAAAAALYmVuZWZpY2lhcnkAAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAAAAAAx0b3RhbF9hbW91bnQAAAALAAAAAAAAAAVjbGlmZgAAAAAAAAYAAAAAAAAACGR1cmF0aW9uAAAABgAAAAEAAAPpAAAABgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
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
    claim: this.txFromJSON<Result<i128>>,
        claimable: this.txFromJSON<Result<i128>>,
        get_status: this.txFromJSON<Result<VestingStatus>>,
        create_schedule: this.txFromJSON<Result<u64>>
  }
}