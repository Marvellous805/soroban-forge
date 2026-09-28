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
 * A royalty configuration for a single collection.
 */
export interface Royalty {
  /**
 * Royalty rate in basis points (100 bps = 1%).
 */
bps: u32;
  /**
 * Collection (NFT contract) this configuration applies to.
 */
collection: string;
  /**
 * Address entitled to royalty payments.
 */
recipient: string;
  /**
 * Whether the configuration is currently enforced.
 */
status: RoyaltyStatus;
}


/**
 * The two amounts one atomic `settle_sale` invocation transferred.
 */
export interface Settlement {
  /**
 * Amount transferred to the configured royalty recipient.
 */
royalty_share: i128;
  /**
 * Amount transferred to the seller.
 */
seller_net: i128;
}

/**
 * Lifecycle state of a registered royalty configuration.
 */
export type RoyaltyStatus = {tag: "Active", values: void} | {tag: "Disabled", values: void};


/**
 * Cumulative settlement totals for one collection.
 */
export interface SettlementSummary {
  /**
 * Sum of every settled sale amount.
 */
gross_volume: i128;
  /**
 * Sum of every royalty share transferred to the recipient.
 */
royalties_paid: i128;
  /**
 * Number of sales settled so far.
 */
sales: u32;
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
   * Construct and simulate a distribute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Compute the royalty split for a sale.
   * 
   * Requires the collection's authorization and `amount > 0`. Returns the
   * net owed to `seller` after reserving `amount * bps / 10_000` for the
   * configured recipient. A `Disabled` configuration settles in full.
   * This entrypoint is a pure computation and moves no tokens; use
   * `settle_sale` to transfer the split in real SEP-41 tokens.
   */
  distribute: ({collection, seller, amount}: {collection: string, seller: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>

  /**
   * Construct and simulate a get_royalty transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the stored royalty configuration for `collection` (read-only view).
   */
  get_royalty: ({collection}: {collection: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Royalty>>>

  /**
   * Construct and simulate a set_royalty transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Register or update a royalty configuration for `collection`.
   * 
   * Requires the collection's authorization and `bps <= 10_000`
   * (100%). Re-registration updates the existing configuration in place.
   */
  set_royalty: ({collection, recipient, bps}: {collection: string, recipient: string, bps: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a settle_sale transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Settle a sale of `collection` atomically in `token`.
   * 
   * Requires the collection's authorization (as `distribute` does)
   * and the `payer`'s, which covers both nested token transfers. Requires
   * `amount > 0` and a stored configuration. The split is computed with
   * checked arithmetic, and both transfers run **before any settlement
   * state is committed**: the seller's net first, the royalty recipient
   * last, so a failed transfer can never leave the royalty recipient
   * partially paid. A `Disabled` or zero-bps configuration settles the
   * full amount to the seller in a single transfer. Token failures are
   * bucketed into [`ForgeError::TokenTransferFailed`], and any returned
   * error rolls the whole invocation back — including an earlier
   * successful transfer — so retrying after a failure never double-pays.
   */
  settle_sale: ({collection, token, payer, seller, amount}: {collection: string, token: string, payer: string, seller: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Settlement>>>

  /**
   * Construct and simulate a get_settlement_summary transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the cumulative settlement totals for `collection` (read-only
   * view). `NotFound` until the collection settles its first sale.
   */
  get_settlement_summary: ({collection}: {collection: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<SettlementSummary>>>

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
      new ContractSpec([ "AAAAAQAAADBBIHJveWFsdHkgY29uZmlndXJhdGlvbiBmb3IgYSBzaW5nbGUgY29sbGVjdGlvbi4AAAAAAAAAB1JveWFsdHkAAAAABAAAACxSb3lhbHR5IHJhdGUgaW4gYmFzaXMgcG9pbnRzICgxMDAgYnBzID0gMSUpLgAAAANicHMAAAAABAAAADhDb2xsZWN0aW9uIChORlQgY29udHJhY3QpIHRoaXMgY29uZmlndXJhdGlvbiBhcHBsaWVzIHRvLgAAAApjb2xsZWN0aW9uAAAAAAATAAAAJUFkZHJlc3MgZW50aXRsZWQgdG8gcm95YWx0eSBwYXltZW50cy4AAAAAAAAJcmVjaXBpZW50AAAAAAAAEwAAADBXaGV0aGVyIHRoZSBjb25maWd1cmF0aW9uIGlzIGN1cnJlbnRseSBlbmZvcmNlZC4AAAAGc3RhdHVzAAAAAAfQAAAADVJveWFsdHlTdGF0dXMAAAA=",
        "AAAAAQAAAEBUaGUgdHdvIGFtb3VudHMgb25lIGF0b21pYyBgc2V0dGxlX3NhbGVgIGludm9jYXRpb24gdHJhbnNmZXJyZWQuAAAAAAAAAApTZXR0bGVtZW50AAAAAAACAAAAN0Ftb3VudCB0cmFuc2ZlcnJlZCB0byB0aGUgY29uZmlndXJlZCByb3lhbHR5IHJlY2lwaWVudC4AAAAADXJveWFsdHlfc2hhcmUAAAAAAAALAAAAIUFtb3VudCB0cmFuc2ZlcnJlZCB0byB0aGUgc2VsbGVyLgAAAAAAAApzZWxsZXJfbmV0AAAAAAAL",
        "AAAAAgAAADZMaWZlY3ljbGUgc3RhdGUgb2YgYSByZWdpc3RlcmVkIHJveWFsdHkgY29uZmlndXJhdGlvbi4AAAAAAAAAAAANUm95YWx0eVN0YXR1cwAAAAAAAAIAAAAAAAAAHEFjdGl2ZSBhbmQgYXBwbGllZCB0byBzYWxlcy4AAAAGQWN0aXZlAAAAAAAAAAAALURpc2FibGVkOyBzYWxlcyBzZXR0bGUgdG8gdGhlIHNlbGxlciBpbiBmdWxsLgAAAAAAAAhEaXNhYmxlZA==",
        "AAAAAQAAADBDdW11bGF0aXZlIHNldHRsZW1lbnQgdG90YWxzIGZvciBvbmUgY29sbGVjdGlvbi4AAAAAAAAAEVNldHRsZW1lbnRTdW1tYXJ5AAAAAAAAAwAAACFTdW0gb2YgZXZlcnkgc2V0dGxlZCBzYWxlIGFtb3VudC4AAAAAAAAMZ3Jvc3Nfdm9sdW1lAAAACwAAADhTdW0gb2YgZXZlcnkgcm95YWx0eSBzaGFyZSB0cmFuc2ZlcnJlZCB0byB0aGUgcmVjaXBpZW50LgAAAA5yb3lhbHRpZXNfcGFpZAAAAAAACwAAAB9OdW1iZXIgb2Ygc2FsZXMgc2V0dGxlZCBzbyBmYXIuAAAAAAVzYWxlcwAAAAAAAAQ=",
        "AAAAAAAAAW1Db21wdXRlIHRoZSByb3lhbHR5IHNwbGl0IGZvciBhIHNhbGUuCgpSZXF1aXJlcyB0aGUgY29sbGVjdGlvbidzIGF1dGhvcml6YXRpb24gYW5kIGBhbW91bnQgPiAwYC4gUmV0dXJucyB0aGUKbmV0IG93ZWQgdG8gYHNlbGxlcmAgYWZ0ZXIgcmVzZXJ2aW5nIGBhbW91bnQgKiBicHMgLyAxMF8wMDBgIGZvciB0aGUKY29uZmlndXJlZCByZWNpcGllbnQuIEEgYERpc2FibGVkYCBjb25maWd1cmF0aW9uIHNldHRsZXMgaW4gZnVsbC4KVGhpcyBlbnRyeXBvaW50IGlzIGEgcHVyZSBjb21wdXRhdGlvbiBhbmQgbW92ZXMgbm8gdG9rZW5zOyB1c2UKYHNldHRsZV9zYWxlYCB0byB0cmFuc2ZlciB0aGUgc3BsaXQgaW4gcmVhbCBTRVAtNDEgdG9rZW5zLgAAAAAAAApkaXN0cmlidXRlAAAAAAADAAAAAAAAAApjb2xsZWN0aW9uAAAAAAATAAAAAAAAAAZzZWxsZXIAAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAEAAAPpAAAACwAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAEhSZWFkIHRoZSBzdG9yZWQgcm95YWx0eSBjb25maWd1cmF0aW9uIGZvciBgY29sbGVjdGlvbmAgKHJlYWQtb25seSB2aWV3KS4AAAALZ2V0X3JveWFsdHkAAAAAAQAAAAAAAAAKY29sbGVjdGlvbgAAAAAAEwAAAAEAAAPpAAAH0AAAAAdSb3lhbHR5AAAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAL5SZWdpc3RlciBvciB1cGRhdGUgYSByb3lhbHR5IGNvbmZpZ3VyYXRpb24gZm9yIGBjb2xsZWN0aW9uYC4KClJlcXVpcmVzIHRoZSBjb2xsZWN0aW9uJ3MgYXV0aG9yaXphdGlvbiBhbmQgYGJwcyA8PSAxMF8wMDBgCigxMDAlKS4gUmUtcmVnaXN0cmF0aW9uIHVwZGF0ZXMgdGhlIGV4aXN0aW5nIGNvbmZpZ3VyYXRpb24gaW4gcGxhY2UuAAAAAAALc2V0X3JveWFsdHkAAAAAAwAAAAAAAAAKY29sbGVjdGlvbgAAAAAAEwAAAAAAAAAJcmVjaXBpZW50AAAAAAAAEwAAAAAAAAADYnBzAAAAAAQAAAABAAAD6QAAAAIAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAAxZTZXR0bGUgYSBzYWxlIG9mIGBjb2xsZWN0aW9uYCBhdG9taWNhbGx5IGluIGB0b2tlbmAuCgpSZXF1aXJlcyB0aGUgY29sbGVjdGlvbidzIGF1dGhvcml6YXRpb24gKGFzIGBkaXN0cmlidXRlYCBkb2VzKQphbmQgdGhlIGBwYXllcmAncywgd2hpY2ggY292ZXJzIGJvdGggbmVzdGVkIHRva2VuIHRyYW5zZmVycy4gUmVxdWlyZXMKYGFtb3VudCA+IDBgIGFuZCBhIHN0b3JlZCBjb25maWd1cmF0aW9uLiBUaGUgc3BsaXQgaXMgY29tcHV0ZWQgd2l0aApjaGVja2VkIGFyaXRobWV0aWMsIGFuZCBib3RoIHRyYW5zZmVycyBydW4gKipiZWZvcmUgYW55IHNldHRsZW1lbnQKc3RhdGUgaXMgY29tbWl0dGVkKio6IHRoZSBzZWxsZXIncyBuZXQgZmlyc3QsIHRoZSByb3lhbHR5IHJlY2lwaWVudApsYXN0LCBzbyBhIGZhaWxlZCB0cmFuc2ZlciBjYW4gbmV2ZXIgbGVhdmUgdGhlIHJveWFsdHkgcmVjaXBpZW50CnBhcnRpYWxseSBwYWlkLiBBIGBEaXNhYmxlZGAgb3IgemVyby1icHMgY29uZmlndXJhdGlvbiBzZXR0bGVzIHRoZQpmdWxsIGFtb3VudCB0byB0aGUgc2VsbGVyIGluIGEgc2luZ2xlIHRyYW5zZmVyLiBUb2tlbiBmYWlsdXJlcyBhcmUKYnVja2V0ZWQgaW50byBbYEZvcmdlRXJyb3I6OlRva2VuVHJhbnNmZXJGYWlsZWRgXSwgYW5kIGFueSByZXR1cm5lZAplcnJvciByb2xscyB0aGUgd2hvbGUgaW52b2NhdGlvbiBiYWNrIOKAlCBpbmNsdWRpbmcgYW4gZWFybGllcgpzdWNjZXNzZnVsIHRyYW5zZmVyIOKAlCBzbyByZXRyeWluZyBhZnRlciBhIGZhaWx1cmUgbmV2ZXIgZG91YmxlLXBheXMuAAAAAAALc2V0dGxlX3NhbGUAAAAABQAAAAAAAAAKY29sbGVjdGlvbgAAAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAAAAAAVwYXllcgAAAAAAABMAAAAAAAAABnNlbGxlcgAAAAAAEwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAQAAA+kAAAfQAAAAClNldHRsZW1lbnQAAAAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAIBSZWFkIHRoZSBjdW11bGF0aXZlIHNldHRsZW1lbnQgdG90YWxzIGZvciBgY29sbGVjdGlvbmAgKHJlYWQtb25seQp2aWV3KS4gYE5vdEZvdW5kYCB1bnRpbCB0aGUgY29sbGVjdGlvbiBzZXR0bGVzIGl0cyBmaXJzdCBzYWxlLgAAABZnZXRfc2V0dGxlbWVudF9zdW1tYXJ5AAAAAAABAAAAAAAAAApjb2xsZWN0aW9uAAAAAAATAAAAAQAAA+kAAAfQAAAAEVNldHRsZW1lbnRTdW1tYXJ5AAAAAAAH0AAAAApGb3JnZUVycm9yAAA=",
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
    distribute: this.txFromJSON<Result<i128>>,
        get_royalty: this.txFromJSON<Result<Royalty>>,
        set_royalty: this.txFromJSON<Result<void>>,
        settle_sale: this.txFromJSON<Result<Settlement>>,
        get_settlement_summary: this.txFromJSON<Result<SettlementSummary>>
  }
}