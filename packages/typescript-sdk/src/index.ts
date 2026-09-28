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

export const networks = {
  testnet: {
    networkPassphrase: "Test SDF Network ; September 2015",
    contractId: "CC227UDF6WBLRTOKKVRIJN7BGSBK67ZGV6IDARJ2AMATGSQ7UZNBZHSB",
  }
} as const


/**
 * A single three-party escrow record.
 */
export interface EscrowData {
  /**
 * Amount of `token` held in custody.
 */
amount: i128;
  /**
 * Neutral party deciding disputes. Recorded at creation; authorizes
 * only `resolve`.
 */
arbiter: string;
  /**
 * Party funding the escrow and the default refund recipient.
 */
buyer: string;
  /**
 * Unix timestamp of creation.
 */
created_at: u64;
  /**
 * Stable id, never reused.
 */
escrow_id: u64;
  /**
 * Party paid on release.
 */
seller: string;
  /**
 * Current lifecycle state.
 */
status: EscrowStatus;
  /**
 * Seconds after `created_at` at which the buyer may self-refund.
 */
timeout: u64;
  /**
 * SEP-41 token contract custodied by this escrow.
 */
token: string;
}

/**
 * Lifecycle state of an escrow.
 */
export type EscrowStatus = {tag: "Pending", values: void} | {tag: "Funded", values: void} | {tag: "Completed", values: void} | {tag: "Refunded", values: void} | {tag: "Disputed", values: void} | {tag: "Cancelled", values: void};


/**
 * A page of escrow ids involving a participant.
 * 
 * Returned by [`SorobanForgeEscrow::escrows_for_participant`]; powered by
 * the per-party persistent index written at creation.
 */
export interface ParticipantEscrowsPage {
  /**
 * Escrow ids on this page, in creation order.
 */
ids: Array<u64>;
  /**
 * Offset for the next page, or `None` when `ids` ends the participant's
 * list. Replay it until `None` to iterate the whole list.
 */
next_cursor: Option<u32>;
  /**
 * Total escrows involving the participant across all pages.
 */
total: u32;
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
   * Construct and simulate a cancel transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Cancel a `Pending` escrow. Requires the buyer. Nothing has moved,
   * so no token transfer occurs.
   */
  cancel: ({escrow_id}: {escrow_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a refund transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Refund the buyer.
   * 
   * Pre-deadline: seller authorizes (voluntary refund). Post-deadline:
   * buyer authorizes (reclaim of unfulfilled funds).
   */
  refund: ({escrow_id}: {escrow_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a deposit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Fund the escrow, pulling tokens from the buyer into this contract.
   * 
   * Ordering: transfer **first**, state write **second** — see the
   * module docs for why the inverse would be a fund-safety bug.
   */
  deposit: ({escrow_id}: {escrow_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a dispute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Raise a dispute: the claimant (buyer or seller) authorizes, while
   * `Funded`. Freezes all payout paths until the arbiter resolves.
   */
  dispute: ({escrow_id, claimant}: {escrow_id: u64, claimant: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a release transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Release funds to the seller. Seller-authorized: delivery
   * confirmation by the paid party, not the paying one.
   */
  release: ({escrow_id}: {escrow_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a resolve transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Resolve a dispute: arbiter only, final. Pays the full amount to the
   * seller (`true`) or refunds the buyer (`false`).
   */
  resolve: ({escrow_id, in_favor_of_seller}: {escrow_id: u64, in_favor_of_seller: boolean}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a touch_ttl transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Permissionless keeper: bump the escrow entry's TTL without changing
   * any state. The existence check is deliberate — touching a missing
   * id must fail loudly so a keeper can distinguish "extended" from
   * "no such escrow".
   */
  touch_ttl: ({escrow_id}: {escrow_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_escrow transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the full escrow record.
   */
  get_escrow: ({escrow_id}: {escrow_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<EscrowData>>>

  /**
   * Construct and simulate a get_status transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the current lifecycle status.
   */
  get_status: ({escrow_id}: {escrow_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<EscrowStatus>>>

  /**
   * Construct and simulate a create_escrow transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Create a new escrow and return its stable id.
   * 
   * Only the buyer authorizes at creation. The arbiter does not
   * authorize either: they must be able to `resolve` later even if
   * they never participated in creation.
   */
  create_escrow: ({buyer, seller, arbiter, token, amount, timeout}: {buyer: string, seller: string, arbiter: string, token: string, amount: i128, timeout: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a escrows_for_participant transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the creation-order escrow ids one page at a time for a
   * participant. Read-only: never mutates storage and never errors — an
   * unknown address or one with no escrows simply yields an empty page.
   */
  escrows_for_participant: ({participant, cursor, limit}: {participant: string, cursor: u32, limit: u32}, options?: MethodOptions) => Promise<AssembledTransaction<ParticipantEscrowsPage>>

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
      new ContractSpec([ "AAAAAAAAAF5DYW5jZWwgYSBgUGVuZGluZ2AgZXNjcm93LiBSZXF1aXJlcyB0aGUgYnV5ZXIuIE5vdGhpbmcgaGFzIG1vdmVkLApzbyBubyB0b2tlbiB0cmFuc2ZlciBvY2N1cnMuAAAAAAAGY2FuY2VsAAAAAAABAAAAAAAAAAllc2Nyb3dfaWQAAAAAAAAGAAAAAQAAA+kAAAACAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAIZSZWZ1bmQgdGhlIGJ1eWVyLgoKUHJlLWRlYWRsaW5lOiBzZWxsZXIgYXV0aG9yaXplcyAodm9sdW50YXJ5IHJlZnVuZCkuIFBvc3QtZGVhZGxpbmU6CmJ1eWVyIGF1dGhvcml6ZXMgKHJlY2xhaW0gb2YgdW5mdWxmaWxsZWQgZnVuZHMpLgAAAAAABnJlZnVuZAAAAAAAAQAAAAAAAAAJZXNjcm93X2lkAAAAAAAABgAAAAEAAAPpAAAAAgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAMBGdW5kIHRoZSBlc2Nyb3csIHB1bGxpbmcgdG9rZW5zIGZyb20gdGhlIGJ1eWVyIGludG8gdGhpcyBjb250cmFjdC4KCk9yZGVyaW5nOiB0cmFuc2ZlciAqKmZpcnN0KiosIHN0YXRlIHdyaXRlICoqc2Vjb25kKiog4oCUIHNlZSB0aGUKbW9kdWxlIGRvY3MgZm9yIHdoeSB0aGUgaW52ZXJzZSB3b3VsZCBiZSBhIGZ1bmQtc2FmZXR5IGJ1Zy4AAAAHZGVwb3NpdAAAAAABAAAAAAAAAAllc2Nyb3dfaWQAAAAAAAAGAAAAAQAAA+kAAAACAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAIBSYWlzZSBhIGRpc3B1dGU6IHRoZSBjbGFpbWFudCAoYnV5ZXIgb3Igc2VsbGVyKSBhdXRob3JpemVzLCB3aGlsZQpgRnVuZGVkYC4gRnJlZXplcyBhbGwgcGF5b3V0IHBhdGhzIHVudGlsIHRoZSBhcmJpdGVyIHJlc29sdmVzLgAAAAdkaXNwdXRlAAAAAAIAAAAAAAAACWVzY3Jvd19pZAAAAAAAAAYAAAAAAAAACGNsYWltYW50AAAAEwAAAAEAAAPpAAAAAgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAGxSZWxlYXNlIGZ1bmRzIHRvIHRoZSBzZWxsZXIuIFNlbGxlci1hdXRob3JpemVkOiBkZWxpdmVyeQpjb25maXJtYXRpb24gYnkgdGhlIHBhaWQgcGFydHksIG5vdCB0aGUgcGF5aW5nIG9uZS4AAAAHcmVsZWFzZQAAAAABAAAAAAAAAAllc2Nyb3dfaWQAAAAAAAAGAAAAAQAAA+kAAAACAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAHNSZXNvbHZlIGEgZGlzcHV0ZTogYXJiaXRlciBvbmx5LCBmaW5hbC4gUGF5cyB0aGUgZnVsbCBhbW91bnQgdG8gdGhlCnNlbGxlciAoYHRydWVgKSBvciByZWZ1bmRzIHRoZSBidXllciAoYGZhbHNlYCkuAAAAAAdyZXNvbHZlAAAAAAIAAAAAAAAACWVzY3Jvd19pZAAAAAAAAAYAAAAAAAAAEmluX2Zhdm9yX29mX3NlbGxlcgAAAAAAAQAAAAEAAAPpAAAAAgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAANlQZXJtaXNzaW9ubGVzcyBrZWVwZXI6IGJ1bXAgdGhlIGVzY3JvdyBlbnRyeSdzIFRUTCB3aXRob3V0IGNoYW5naW5nCmFueSBzdGF0ZS4gVGhlIGV4aXN0ZW5jZSBjaGVjayBpcyBkZWxpYmVyYXRlIOKAlCB0b3VjaGluZyBhIG1pc3NpbmcKaWQgbXVzdCBmYWlsIGxvdWRseSBzbyBhIGtlZXBlciBjYW4gZGlzdGluZ3Vpc2ggImV4dGVuZGVkIiBmcm9tCiJubyBzdWNoIGVzY3JvdyIuAAAAAAAACXRvdWNoX3R0bAAAAAAAAAEAAAAAAAAACWVzY3Jvd19pZAAAAAAAAAYAAAABAAAD6QAAAAIAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAABxSZWFkIHRoZSBmdWxsIGVzY3JvdyByZWNvcmQuAAAACmdldF9lc2Nyb3cAAAAAAAEAAAAAAAAACWVzY3Jvd19pZAAAAAAAAAYAAAABAAAD6QAAB9AAAAAKRXNjcm93RGF0YQAAAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAACJSZWFkIHRoZSBjdXJyZW50IGxpZmVjeWNsZSBzdGF0dXMuAAAAAAAKZ2V0X3N0YXR1cwAAAAAAAQAAAAAAAAAJZXNjcm93X2lkAAAAAAAABgAAAAEAAAPpAAAH0AAAAAxFc2Nyb3dTdGF0dXMAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAQAAACNBIHNpbmdsZSB0aHJlZS1wYXJ0eSBlc2Nyb3cgcmVjb3JkLgAAAAAAAAAACkVzY3Jvd0RhdGEAAAAAAAkAAAAiQW1vdW50IG9mIGB0b2tlbmAgaGVsZCBpbiBjdXN0b2R5LgAAAAAABmFtb3VudAAAAAAACwAAAFFOZXV0cmFsIHBhcnR5IGRlY2lkaW5nIGRpc3B1dGVzLiBSZWNvcmRlZCBhdCBjcmVhdGlvbjsgYXV0aG9yaXplcwpvbmx5IGByZXNvbHZlYC4AAAAAAAAHYXJiaXRlcgAAAAATAAAAOlBhcnR5IGZ1bmRpbmcgdGhlIGVzY3JvdyBhbmQgdGhlIGRlZmF1bHQgcmVmdW5kIHJlY2lwaWVudC4AAAAAAAVidXllcgAAAAAAABMAAAAbVW5peCB0aW1lc3RhbXAgb2YgY3JlYXRpb24uAAAAAApjcmVhdGVkX2F0AAAAAAAGAAAAGFN0YWJsZSBpZCwgbmV2ZXIgcmV1c2VkLgAAAAllc2Nyb3dfaWQAAAAAAAAGAAAAFlBhcnR5IHBhaWQgb24gcmVsZWFzZS4AAAAAAAZzZWxsZXIAAAAAABMAAAAYQ3VycmVudCBsaWZlY3ljbGUgc3RhdGUuAAAABnN0YXR1cwAAAAAH0AAAAAxFc2Nyb3dTdGF0dXMAAAA+U2Vjb25kcyBhZnRlciBgY3JlYXRlZF9hdGAgYXQgd2hpY2ggdGhlIGJ1eWVyIG1heSBzZWxmLXJlZnVuZC4AAAAAAAd0aW1lb3V0AAAAAAYAAAAvU0VQLTQxIHRva2VuIGNvbnRyYWN0IGN1c3RvZGllZCBieSB0aGlzIGVzY3Jvdy4AAAAABXRva2VuAAAAAAAAEw==",
        "AAAAAgAAAB1MaWZlY3ljbGUgc3RhdGUgb2YgYW4gZXNjcm93LgAAAAAAAAAAAAAMRXNjcm93U3RhdHVzAAAABgAAAAAAAAAXQ3JlYXRlZCBidXQgbm90IGZ1bmRlZC4AAAAAB1BlbmRpbmcAAAAAAAAAABxUb2tlbnMgaGVsZCBieSB0aGUgY29udHJhY3QuAAAABkZ1bmRlZAAAAAAAAAAAABdSZWxlYXNlZCB0byB0aGUgc2VsbGVyLgAAAAAJQ29tcGxldGVkAAAAAAAAAAAAABZSZWZ1bmRlZCB0byB0aGUgYnV5ZXIuAAAAAAAIUmVmdW5kZWQAAAAAAAAAPEEgcGFydHkgcmFpc2VkIGEgZGlzcHV0ZTsgZnJvemVuIHVudGlsIHRoZSBhcmJpdGVyIHJlc29sdmVzLgAAAAhEaXNwdXRlZAAAAAAAAAAZQ2FuY2VsbGVkIGJlZm9yZSBmdW5kaW5nLgAAAAAAAAlDYW5jZWxsZWQAAAA=",
        "AAAAAAAAAM5DcmVhdGUgYSBuZXcgZXNjcm93IGFuZCByZXR1cm4gaXRzIHN0YWJsZSBpZC4KCk9ubHkgdGhlIGJ1eWVyIGF1dGhvcml6ZXMgYXQgY3JlYXRpb24uIFRoZSBhcmJpdGVyIGRvZXMgbm90CmF1dGhvcml6ZSBlaXRoZXI6IHRoZXkgbXVzdCBiZSBhYmxlIHRvIGByZXNvbHZlYCBsYXRlciBldmVuIGlmCnRoZXkgbmV2ZXIgcGFydGljaXBhdGVkIGluIGNyZWF0aW9uLgAAAAAADWNyZWF0ZV9lc2Nyb3cAAAAAAAAGAAAAAAAAAAVidXllcgAAAAAAABMAAAAAAAAABnNlbGxlcgAAAAAAEwAAAAAAAAAHYXJiaXRlcgAAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAAAAAAHdGltZW91dAAAAAAGAAAAAQAAA+kAAAAGAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAQAAAKpBIHBhZ2Ugb2YgZXNjcm93IGlkcyBpbnZvbHZpbmcgYSBwYXJ0aWNpcGFudC4KClJldHVybmVkIGJ5IFtgU29yb2JhbkZvcmdlRXNjcm93Ojplc2Nyb3dzX2Zvcl9wYXJ0aWNpcGFudGBdOyBwb3dlcmVkIGJ5CnRoZSBwZXItcGFydHkgcGVyc2lzdGVudCBpbmRleCB3cml0dGVuIGF0IGNyZWF0aW9uLgAAAAAAAAAAABZQYXJ0aWNpcGFudEVzY3Jvd3NQYWdlAAAAAAADAAAAK0VzY3JvdyBpZHMgb24gdGhpcyBwYWdlLCBpbiBjcmVhdGlvbiBvcmRlci4AAAAAA2lkcwAAAAPqAAAABgAAAH1PZmZzZXQgZm9yIHRoZSBuZXh0IHBhZ2UsIG9yIGBOb25lYCB3aGVuIGBpZHNgIGVuZHMgdGhlIHBhcnRpY2lwYW50J3MKbGlzdC4gUmVwbGF5IGl0IHVudGlsIGBOb25lYCB0byBpdGVyYXRlIHRoZSB3aG9sZSBsaXN0LgAAAAAAAAtuZXh0X2N1cnNvcgAAAAPoAAAABAAAADlUb3RhbCBlc2Nyb3dzIGludm9sdmluZyB0aGUgcGFydGljaXBhbnQgYWNyb3NzIGFsbCBwYWdlcy4AAAAAAAAFdG90YWwAAAAAAAAE",
        "AAAAAAAAAMVSZWFkIHRoZSBjcmVhdGlvbi1vcmRlciBlc2Nyb3cgaWRzIG9uZSBwYWdlIGF0IGEgdGltZSBmb3IgYQpwYXJ0aWNpcGFudC4gUmVhZC1vbmx5OiBuZXZlciBtdXRhdGVzIHN0b3JhZ2UgYW5kIG5ldmVyIGVycm9ycyDigJQgYW4KdW5rbm93biBhZGRyZXNzIG9yIG9uZSB3aXRoIG5vIGVzY3Jvd3Mgc2ltcGx5IHlpZWxkcyBhbiBlbXB0eSBwYWdlLgAAAAAAABdlc2Nyb3dzX2Zvcl9wYXJ0aWNpcGFudAAAAAADAAAAAAAAAAtwYXJ0aWNpcGFudAAAAAATAAAAAAAAAAZjdXJzb3IAAAAAAAQAAAAAAAAABWxpbWl0AAAAAAAABAAAAAEAAAfQAAAAFlBhcnRpY2lwYW50RXNjcm93c1BhZ2UAAA==",
        "AAAABQAAAAAAAAAAAAAACERpc3B1dGVkAAAAAQAAAAhkaXNwdXRlZAAAAAIAAAAAAAAACWVzY3Jvd19pZAAAAAAAAAYAAAABAAAAAAAAAARkYXRhAAAH0AAAAApFc2Nyb3dEYXRhAAAAAAAAAAAAAg==",
        "AAAABQAAAAAAAAAAAAAACFJlZnVuZGVkAAAAAQAAAAhyZWZ1bmRlZAAAAAIAAAAAAAAACWVzY3Jvd19pZAAAAAAAAAYAAAABAAAAAAAAAARkYXRhAAAH0AAAAApFc2Nyb3dEYXRhAAAAAAAAAAAAAg==",
        "AAAABQAAAAAAAAAAAAAACFJlbGVhc2VkAAAAAQAAAAhyZWxlYXNlZAAAAAIAAAAAAAAACWVzY3Jvd19pZAAAAAAAAAYAAAABAAAAAAAAAARkYXRhAAAH0AAAAApFc2Nyb3dEYXRhAAAAAAAAAAAAAg==",
        "AAAABQAAAAAAAAAAAAAACFJlc29sdmVkAAAAAQAAAAhyZXNvbHZlZAAAAAMAAAAAAAAACWVzY3Jvd19pZAAAAAAAAAYAAAABAAAAAAAAAARkYXRhAAAH0AAAAApFc2Nyb3dEYXRhAAAAAAAAAAAAAAAAABJpbl9mYXZvcl9vZl9zZWxsZXIAAAAAAAEAAAAAAAAAAg==",
        "AAAABQAAAAAAAAAAAAAACUNhbmNlbGxlZAAAAAAAAAEAAAAJY2FuY2VsbGVkAAAAAAAAAgAAAAAAAAAJZXNjcm93X2lkAAAAAAAABgAAAAEAAAAAAAAABGRhdGEAAAfQAAAACkVzY3Jvd0RhdGEAAAAAAAAAAAAC",
        "AAAABQAAAAAAAAAAAAAACURlcG9zaXRlZAAAAAAAAAEAAAAJZGVwb3NpdGVkAAAAAAAAAgAAAAAAAAAJZXNjcm93X2lkAAAAAAAABgAAAAEAAAAAAAAABGRhdGEAAAfQAAAACkVzY3Jvd0RhdGEAAAAAAAAAAAAC",
        "AAAABQAAAAAAAAAAAAAADUVzY3Jvd0NyZWF0ZWQAAAAAAAABAAAADmVzY3Jvd19jcmVhdGVkAAAAAAACAAAAAAAAAAllc2Nyb3dfaWQAAAAAAAAGAAAAAQAAAAAAAAAEZGF0YQAAB9AAAAAKRXNjcm93RGF0YQAAAAAAAAAAAAI=",
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
    cancel: this.txFromJSON<Result<void>>,
        refund: this.txFromJSON<Result<void>>,
        deposit: this.txFromJSON<Result<void>>,
        dispute: this.txFromJSON<Result<void>>,
        release: this.txFromJSON<Result<void>>,
        resolve: this.txFromJSON<Result<void>>,
        touch_ttl: this.txFromJSON<Result<void>>,
        get_escrow: this.txFromJSON<Result<EscrowData>>,
        get_status: this.txFromJSON<Result<EscrowStatus>>,
        create_escrow: this.txFromJSON<Result<u64>>,
        escrows_for_participant: this.txFromJSON<ParticipantEscrowsPage>
  }
}