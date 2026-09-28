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
 * What a submitted transaction carries.
 * 
 * The kinds encode the custody split documented in the module docs:
 * opaque-payload transactions dispatch (out of scope here) through their
 * `payload` bytes, withdrawal transactions move real tokens through the
 * typed record below, and limit-change transactions retune the per-token
 * rolling withdrawal cap.
 */
export type TxKind = {tag: "Opaque", values: void} | {tag: "Withdrawal", values: readonly [Withdrawal]} | {tag: "LimitChange", values: readonly [LimitChange]} | {tag: "AddOwner", values: readonly [string]} | {tag: "RemoveOwner", values: readonly [string]} | {tag: "SetThreshold", values: readonly [u32]};

/**
 * Lifecycle state of a submitted transaction.
 */
export type TxStatus = {tag: "Pending", values: void} | {tag: "Executed", values: void} | {tag: "Rejected", values: void};


/**
 * A transaction awaiting multi-signature approval.
 */
export interface WalletTx {
  /**
 * Owners that have confirmed so far.
 */
confirmations: Array<string>;
  /**
 * What the transaction carries: an opaque payload or a typed token
 * withdrawal (see [`TxKind`]).
 */
kind: TxKind;
  /**
 * The encoded transaction payload to pass to the target.
 */
payload: Buffer;
  /**
 * Owners that have formally objected so far.
 */
rejections: Array<string>;
  /**
 * Current state.
 */
status: TxStatus;
  /**
 * Address that submitted the transaction.
 */
submitter: string;
  /**
 * The target contract to invoke on execution.
 */
target: string;
  /**
 * Stable identifier assigned at submission time.
 */
tx_id: u64;
}


/**
 * A typed token withdrawal record (see [`TxKind::Withdrawal`] and the
 * custody model in the module docs for why this is a record rather than
 * payload bytes).
 */
export interface Withdrawal {
  /**
 * Amount to move; must be positive.
 */
amount: i128;
  /**
 * Recipient of the tokens.
 */
destination: string;
  /**
 * SEP-41 token to move out of custody.
 */
token: string;
}

/**
 * A threshold-gated change to a token's withdrawal limit, carried by
 * [`TxKind::LimitChange`].
 */
export type LimitChange = {tag: "Set", values: readonly [WithdrawalLimit]} | {tag: "Remove", values: readonly [string]};


/**
 * One withdrawal counted against a token's rolling window.
 * 
 * A withdrawal occupies its window from `submitted_at` until
 * `submitted_at + window_seconds` elapses, whether or not it executes.
 */
export interface WindowEntry {
  /**
 * Amount the withdrawal carries.
 */
amount: i128;
  /**
 * Ledger timestamp at which the withdrawal was submitted.
 */
submitted_at: u64;
}


/**
 * A per-token rolling withdrawal limit: at most `amount` of `token` may
 * leave custody in any `window_seconds` window (see the withdrawal-limits
 * section in the module docs).
 */
export interface WithdrawalLimit {
  /**
 * Maximum total that may leave custody inside one window; positive.
 */
amount: i128;
  /**
 * SEP-41 token the limit applies to.
 */
token: string;
  /**
 * Length of the rolling window in seconds; positive.
 */
window_seconds: u64;
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
   * Construct and simulate a get_tx transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read a stored transaction by id (read-only view).
   */
  get_tx: ({tx_id}: {tx_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<WalletTx>>>

  /**
   * Construct and simulate a reject transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Record an owner's formal objection to a pending transaction.
   * 
   * An owner may reject only while the transaction is `Pending`, may not
   * reject twice, and may not reject a transaction they have confirmed
   * (one signal per owner, in one direction). Existing confirmations from
   * other owners do not block a rejection, and a rejection can never be
   * revoked.
   * 
   * Once `rejections.len() >= threshold` the status flips to `Rejected`,
   * which is terminal: no further confirms, executes, or rejects. Below
   * the threshold the tx stays `Pending` but is already blocked from
   * executing (see the module docs).
   */
  reject: ({tx_id, signer}: {tx_id: u64, signer: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a submit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Submit a new transaction for owner approval.
   * 
   * Requires the submitter to be an owner. Returns the stable `tx_id` that
   * confirmations reference.
   */
  submit: ({submitter, target, tx}: {submitter: string, target: string, tx: Buffer}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a balance transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the wallet's custody balance of `token` (read-only view).
   * 
   * Unknown tokens read as zero so the view has no error path.
   */
  balance: ({token}: {token: string}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a confirm transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Record an owner's approval of a pending transaction.
   * 
   * An owner may confirm only once, and only while the transaction is
   * `Pending`. An owner who has already rejected the transaction may not
   * also confirm it (one signal per owner, in one direction — see the
   * module docs for the rejection policy).
   */
  confirm: ({tx_id, signer}: {tx_id: u64, signer: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a deposit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Deposit `amount` of `token` into custody, pulling from `from`.
   * 
   * Ordering: transfer **first**, balance write **second** — see the
   * module docs for why the inverse would be a fund-safety bug.
   */
  deposit: ({token, from, amount}: {token: string, from: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a execute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Execute a transaction once the owner approvals meet the threshold.
   * 
   * Callable by anyone once the threshold is met; otherwise the state
   * transition is rejected.
   * 
   * For typed withdrawal txs this moves the recorded tokens to the
   * recorded destination before the status flips (transfer first, state
   * second — see the module docs). The wallet balance is validated
   * against the recorded amount first (`InsufficientFunds`), so a
   * withdrawal that exceeds custody fails with balances and tx state
   * untouched.
   * 
   * For opaque-payload txs this performs a real cross-contract
   * invocation to the recorded `target` with the stored `payload`.
   * The invocation is attempted **before** the status flips. A
   * target revert surfaces as [`ForgeError::ContractInvocationFailed`]
   * and leaves the transaction un-executed (status stays `Pending`).
   * 
   * Execution is refused while the tx carries **any** rejection, even
   * below the rejection threshold (see the module docs).
   */
  execute: ({tx_id}: {tx_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a is_owner transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Check whether `address` is a member of the owner set (read-only view).
   * 
   * Uninitialized wallets read as `false`.
   */
  is_owner: ({address}: {address: string}, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a add_owner transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Propose adding `new_owner` to the owner set (see the trait docs).
   * 
   * Creates only a pending governance [`TxKind::AddOwner`] tx; the owner
   * set is untouched until the threshold-approved `execute` applies it.
   */
  add_owner: ({submitter, new_owner}: {submitter: string, new_owner: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a touch_ttl transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Permissionless keeper: bump the balance entry's TTL without changing
   * any state (see the trait docs).
   */
  touch_ttl: ({token}: {token: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_owners transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the configured owner set, in initialization order (read-only view).
   * 
   * # Errors
   * 
   * * [`ForgeError::NotInitialized`] — the wallet has no owner set.
   */
  get_owners: (options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<string>>>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Configure the wallet for the first and only time.
   * 
   * Requires a non-empty owner set with no duplicates and
   * `0 < threshold <= owners.len()`. The caller deploys the wallet and
   * initialises it in the same transaction.
   */
  initialize: ({owners, threshold}: {owners: Array<string>, threshold: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_tx_count transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the number of transactions submitted so far (read-only view).
   * 
   * The `Count` counter only advances on successful `submit`/
   * `submit_withdrawal`, so this matches the number of recorded
   * transactions. Uninitialized wallets read as `0`.
   */
  get_tx_count: (options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a remove_owner transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Propose removing `existing_owner` from the owner set (see the trait
   * docs).
   * 
   * Creates only a pending governance [`TxKind::RemoveOwner`] tx; the
   * owner set is untouched until the threshold-approved `execute` applies
   * it. Removing the final owner is rejected at submission so a pending
   * proposal can never leave the wallet ownerless.
   */
  remove_owner: ({submitter, existing_owner}: {submitter: string, existing_owner: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a get_threshold transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the configured approval threshold (read-only view).
   */
  get_threshold: (options?: MethodOptions) => Promise<AssembledTransaction<Result<u32>>>

  /**
   * Construct and simulate a set_threshold transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Propose changing the approval threshold to `new_threshold` (see the
   * trait docs).
   * 
   * Creates only a pending governance [`TxKind::SetThreshold`] tx. The
   * threshold is validated against the current owner set at submission and
   * re-validated against the resulting state at execution.
   */
  set_threshold: ({submitter, new_threshold}: {submitter: string, new_threshold: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a get_rejections transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the rejection list recorded for `tx_id`, in the order the
   * rejections were recorded (read-only twin of `WalletTx::rejections`).
   * 
   * # Errors
   * 
   * * [`ForgeError::NotFound`] — no transaction with id `tx_id`.
   */
  get_rejections: ({tx_id}: {tx_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<string>>>>

  /**
   * Construct and simulate a get_transactions transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read transactions in ascending id order, with a zero-based offset.
   */
  get_transactions: ({offset, limit}: {offset: u32, limit: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<WalletTx>>>>

  /**
   * Construct and simulate a get_window_usage transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read `token`'s current in-window withdrawal total (read-only view).
   * 
   * Expired entries are excluded. Reads as `0` when no limit is
   * configured, because the window is defined by that limit.
   */
  get_window_usage: ({token}: {token: string}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_confirmations transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the confirmation list recorded for `tx_id`, in the order the
   * confirmations were recorded (read-only twin of
   * `WalletTx::confirmations`).
   * 
   * # Errors
   * 
   * * [`ForgeError::NotFound`] — no transaction with id `tx_id`.
   */
  get_confirmations: ({tx_id}: {tx_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<string>>>>

  /**
   * Construct and simulate a submit_withdrawal transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Submit a token withdrawal as a pending tx (see the trait docs).
   */
  submit_withdrawal: ({submitter, token, destination, amount}: {submitter: string, token: string, destination: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a get_withdrawal_limit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read `token`'s configured rolling withdrawal limit (read-only view).
   */
  get_withdrawal_limit: ({token}: {token: string}, options?: MethodOptions) => Promise<AssembledTransaction<Option<WithdrawalLimit>>>

  /**
   * Construct and simulate a set_withdrawal_limit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Propose a rolling withdrawal limit for `token` (see the trait docs).
   * 
   * Only the threshold-approved `execute` applies it, so a
   * partial-threshold proposal changes nothing.
   */
  set_withdrawal_limit: ({submitter, token, amount, window_seconds}: {submitter: string, token: string, amount: i128, window_seconds: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a remove_withdrawal_limit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Propose removing `token`'s rolling withdrawal limit (see the trait
   * docs).
   */
  remove_withdrawal_limit: ({submitter, token}: {submitter: string, token: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a get_transactions_by_status transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read matching transactions, applying offset and limit to the filtered
   * sequence in ascending transaction-id order.
   */
  get_transactions_by_status: ({status, offset, limit}: {status: TxStatus, offset: u32, limit: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<WalletTx>>>>

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
      new ContractSpec([ "AAAAAgAAAVRXaGF0IGEgc3VibWl0dGVkIHRyYW5zYWN0aW9uIGNhcnJpZXMuCgpUaGUga2luZHMgZW5jb2RlIHRoZSBjdXN0b2R5IHNwbGl0IGRvY3VtZW50ZWQgaW4gdGhlIG1vZHVsZSBkb2NzOgpvcGFxdWUtcGF5bG9hZCB0cmFuc2FjdGlvbnMgZGlzcGF0Y2ggKG91dCBvZiBzY29wZSBoZXJlKSB0aHJvdWdoIHRoZWlyCmBwYXlsb2FkYCBieXRlcywgd2l0aGRyYXdhbCB0cmFuc2FjdGlvbnMgbW92ZSByZWFsIHRva2VucyB0aHJvdWdoIHRoZQp0eXBlZCByZWNvcmQgYmVsb3csIGFuZCBsaW1pdC1jaGFuZ2UgdHJhbnNhY3Rpb25zIHJldHVuZSB0aGUgcGVyLXRva2VuCnJvbGxpbmcgd2l0aGRyYXdhbCBjYXAuAAAAAAAAAAZUeEtpbmQAAAAAAAYAAAAAAAAAQ09wYXF1ZS1wYXlsb2FkIHRyYW5zYWN0aW9uOiBgV2FsbGV0VHg6OnBheWxvYWRgIGNhcnJpZXMgdGhlIGludGVudC4AAAAABk9wYXF1ZQAAAAAAAQAAABdUeXBlZCB0b2tlbiB3aXRoZHJhd2FsLgAAAAAKV2l0aGRyYXdhbAAAAAAAAQAAB9AAAAAKV2l0aGRyYXdhbAAAAAAAAQAAAD1UaHJlc2hvbGQtZ2F0ZWQgY2hhbmdlIHRvIGEgdG9rZW4ncyByb2xsaW5nIHdpdGhkcmF3YWwgbGltaXQuAAAAAAAAC0xpbWl0Q2hhbmdlAAAAAAEAAAfQAAAAC0xpbWl0Q2hhbmdlAAAAAAEAAAAoVGhyZXNob2xkLWdhdGVkIGFkZGl0aW9uIG9mIGEgbmV3IG93bmVyLgAAAAhBZGRPd25lcgAAAAEAAAATAAAAAQAAAC1UaHJlc2hvbGQtZ2F0ZWQgcmVtb3ZhbCBvZiBhbiBleGlzdGluZyBvd25lci4AAAAAAAALUmVtb3ZlT3duZXIAAAAAAQAAABMAAAABAAAAMVRocmVzaG9sZC1nYXRlZCBjaGFuZ2UgdG8gdGhlIGFwcHJvdmFsIHRocmVzaG9sZC4AAAAAAAAMU2V0VGhyZXNob2xkAAAAAQAAAAQ=",
        "AAAAAgAAACtMaWZlY3ljbGUgc3RhdGUgb2YgYSBzdWJtaXR0ZWQgdHJhbnNhY3Rpb24uAAAAAAAAAAAIVHhTdGF0dXMAAAADAAAAAAAAAC5PcGVuIGZvciBjb25maXJtYXRpb25zOyB0aHJlc2hvbGQgbm90IHlldCBtZXQuAAAAAAAHUGVuZGluZwAAAAAAAAAAKFRocmVzaG9sZCBtZXQgYW5kIGV4ZWN1dGVkIHN1Y2Nlc3NmdWxseS4AAAAIRXhlY3V0ZWQAAAAAAAAAP1JlamVjdGVkIGJ5IG93bmVycyAocmVhY2hlZCB0aGUgcmVqZWN0aW9uIHRocmVzaG9sZCk7IHRlcm1pbmFsLgAAAAAIUmVqZWN0ZWQ=",
        "AAAAAQAAADBBIHRyYW5zYWN0aW9uIGF3YWl0aW5nIG11bHRpLXNpZ25hdHVyZSBhcHByb3ZhbC4AAAAAAAAACFdhbGxldFR4AAAACAAAACJPd25lcnMgdGhhdCBoYXZlIGNvbmZpcm1lZCBzbyBmYXIuAAAAAAANY29uZmlybWF0aW9ucwAAAAAAA+oAAAATAAAAXVdoYXQgdGhlIHRyYW5zYWN0aW9uIGNhcnJpZXM6IGFuIG9wYXF1ZSBwYXlsb2FkIG9yIGEgdHlwZWQgdG9rZW4Kd2l0aGRyYXdhbCAoc2VlIFtgVHhLaW5kYF0pLgAAAAAAAARraW5kAAAH0AAAAAZUeEtpbmQAAAAAADZUaGUgZW5jb2RlZCB0cmFuc2FjdGlvbiBwYXlsb2FkIHRvIHBhc3MgdG8gdGhlIHRhcmdldC4AAAAAAAdwYXlsb2FkAAAAAA4AAAAqT3duZXJzIHRoYXQgaGF2ZSBmb3JtYWxseSBvYmplY3RlZCBzbyBmYXIuAAAAAAAKcmVqZWN0aW9ucwAAAAAD6gAAABMAAAAOQ3VycmVudCBzdGF0ZS4AAAAAAAZzdGF0dXMAAAAAB9AAAAAIVHhTdGF0dXMAAAAnQWRkcmVzcyB0aGF0IHN1Ym1pdHRlZCB0aGUgdHJhbnNhY3Rpb24uAAAAAAlzdWJtaXR0ZXIAAAAAAAATAAAAK1RoZSB0YXJnZXQgY29udHJhY3QgdG8gaW52b2tlIG9uIGV4ZWN1dGlvbi4AAAAABnRhcmdldAAAAAAAEwAAAC5TdGFibGUgaWRlbnRpZmllciBhc3NpZ25lZCBhdCBzdWJtaXNzaW9uIHRpbWUuAAAAAAAFdHhfaWQAAAAAAAAG",
        "AAAAAQAAAJlBIHR5cGVkIHRva2VuIHdpdGhkcmF3YWwgcmVjb3JkIChzZWUgW2BUeEtpbmQ6OldpdGhkcmF3YWxgXSBhbmQgdGhlCmN1c3RvZHkgbW9kZWwgaW4gdGhlIG1vZHVsZSBkb2NzIGZvciB3aHkgdGhpcyBpcyBhIHJlY29yZCByYXRoZXIgdGhhbgpwYXlsb2FkIGJ5dGVzKS4AAAAAAAAAAAAACldpdGhkcmF3YWwAAAAAAAMAAAAhQW1vdW50IHRvIG1vdmU7IG11c3QgYmUgcG9zaXRpdmUuAAAAAAAABmFtb3VudAAAAAAACwAAABhSZWNpcGllbnQgb2YgdGhlIHRva2Vucy4AAAALZGVzdGluYXRpb24AAAAAEwAAACRTRVAtNDEgdG9rZW4gdG8gbW92ZSBvdXQgb2YgY3VzdG9keS4AAAAFdG9rZW4AAAAAAAAT",
        "AAAAAgAAAFtBIHRocmVzaG9sZC1nYXRlZCBjaGFuZ2UgdG8gYSB0b2tlbidzIHdpdGhkcmF3YWwgbGltaXQsIGNhcnJpZWQgYnkKW2BUeEtpbmQ6OkxpbWl0Q2hhbmdlYF0uAAAAAAAAAAALTGltaXRDaGFuZ2UAAAAAAgAAAAEAAAA+SW5zdGFsbCBvciByZXBsYWNlIGEgbGltaXQgKHZhbGlkYXRlZCBwb3NpdGl2ZSBhdCBzdWJtaXNzaW9uKS4AAAAAAANTZXQAAAAAAQAAB9AAAAAPV2l0aGRyYXdhbExpbWl0AAAAAAEAAABDRHJvcCB0aGUgbGltaXQgZm9yIHRoZSB0b2tlbiwgbWFraW5nIGl0cyB3aXRoZHJhd2FscyB1bmNvbnN0cmFpbmVkLgAAAAAGUmVtb3ZlAAAAAAABAAAAEw==",
        "AAAAAQAAALlPbmUgd2l0aGRyYXdhbCBjb3VudGVkIGFnYWluc3QgYSB0b2tlbidzIHJvbGxpbmcgd2luZG93LgoKQSB3aXRoZHJhd2FsIG9jY3VwaWVzIGl0cyB3aW5kb3cgZnJvbSBgc3VibWl0dGVkX2F0YCB1bnRpbApgc3VibWl0dGVkX2F0ICsgd2luZG93X3NlY29uZHNgIGVsYXBzZXMsIHdoZXRoZXIgb3Igbm90IGl0IGV4ZWN1dGVzLgAAAAAAAAAAAAALV2luZG93RW50cnkAAAAAAgAAAB5BbW91bnQgdGhlIHdpdGhkcmF3YWwgY2Fycmllcy4AAAAAAAZhbW91bnQAAAAAAAsAAAA3TGVkZ2VyIHRpbWVzdGFtcCBhdCB3aGljaCB0aGUgd2l0aGRyYXdhbCB3YXMgc3VibWl0dGVkLgAAAAAMc3VibWl0dGVkX2F0AAAABg==",
        "AAAAAAAAADFSZWFkIGEgc3RvcmVkIHRyYW5zYWN0aW9uIGJ5IGlkIChyZWFkLW9ubHkgdmlldykuAAAAAAAABmdldF90eAAAAAAAAQAAAAAAAAAFdHhfaWQAAAAAAAAGAAAAAQAAA+kAAAfQAAAACFdhbGxldFR4AAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAkRSZWNvcmQgYW4gb3duZXIncyBmb3JtYWwgb2JqZWN0aW9uIHRvIGEgcGVuZGluZyB0cmFuc2FjdGlvbi4KCkFuIG93bmVyIG1heSByZWplY3Qgb25seSB3aGlsZSB0aGUgdHJhbnNhY3Rpb24gaXMgYFBlbmRpbmdgLCBtYXkgbm90CnJlamVjdCB0d2ljZSwgYW5kIG1heSBub3QgcmVqZWN0IGEgdHJhbnNhY3Rpb24gdGhleSBoYXZlIGNvbmZpcm1lZAoob25lIHNpZ25hbCBwZXIgb3duZXIsIGluIG9uZSBkaXJlY3Rpb24pLiBFeGlzdGluZyBjb25maXJtYXRpb25zIGZyb20Kb3RoZXIgb3duZXJzIGRvIG5vdCBibG9jayBhIHJlamVjdGlvbiwgYW5kIGEgcmVqZWN0aW9uIGNhbiBuZXZlciBiZQpyZXZva2VkLgoKT25jZSBgcmVqZWN0aW9ucy5sZW4oKSA+PSB0aHJlc2hvbGRgIHRoZSBzdGF0dXMgZmxpcHMgdG8gYFJlamVjdGVkYCwKd2hpY2ggaXMgdGVybWluYWw6IG5vIGZ1cnRoZXIgY29uZmlybXMsIGV4ZWN1dGVzLCBvciByZWplY3RzLiBCZWxvdwp0aGUgdGhyZXNob2xkIHRoZSB0eCBzdGF5cyBgUGVuZGluZ2AgYnV0IGlzIGFscmVhZHkgYmxvY2tlZCBmcm9tCmV4ZWN1dGluZyAoc2VlIHRoZSBtb2R1bGUgZG9jcykuAAAABnJlamVjdAAAAAAAAgAAAAAAAAAFdHhfaWQAAAAAAAAGAAAAAAAAAAZzaWduZXIAAAAAABMAAAABAAAD6QAAAAIAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAAI1TdWJtaXQgYSBuZXcgdHJhbnNhY3Rpb24gZm9yIG93bmVyIGFwcHJvdmFsLgoKUmVxdWlyZXMgdGhlIHN1Ym1pdHRlciB0byBiZSBhbiBvd25lci4gUmV0dXJucyB0aGUgc3RhYmxlIGB0eF9pZGAgdGhhdApjb25maXJtYXRpb25zIHJlZmVyZW5jZS4AAAAAAAAGc3VibWl0AAAAAAADAAAAAAAAAAlzdWJtaXR0ZXIAAAAAAAATAAAAAAAAAAZ0YXJnZXQAAAAAABMAAAAAAAAAAnR4AAAAAAAOAAAAAQAAA+kAAAAGAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAHpSZWFkIHRoZSB3YWxsZXQncyBjdXN0b2R5IGJhbGFuY2Ugb2YgYHRva2VuYCAocmVhZC1vbmx5IHZpZXcpLgoKVW5rbm93biB0b2tlbnMgcmVhZCBhcyB6ZXJvIHNvIHRoZSB2aWV3IGhhcyBubyBlcnJvciBwYXRoLgAAAAAAB2JhbGFuY2UAAAAAAQAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAQAAAAs=",
        "AAAAAAAAASdSZWNvcmQgYW4gb3duZXIncyBhcHByb3ZhbCBvZiBhIHBlbmRpbmcgdHJhbnNhY3Rpb24uCgpBbiBvd25lciBtYXkgY29uZmlybSBvbmx5IG9uY2UsIGFuZCBvbmx5IHdoaWxlIHRoZSB0cmFuc2FjdGlvbiBpcwpgUGVuZGluZ2AuIEFuIG93bmVyIHdobyBoYXMgYWxyZWFkeSByZWplY3RlZCB0aGUgdHJhbnNhY3Rpb24gbWF5IG5vdAphbHNvIGNvbmZpcm0gaXQgKG9uZSBzaWduYWwgcGVyIG93bmVyLCBpbiBvbmUgZGlyZWN0aW9uIOKAlCBzZWUgdGhlCm1vZHVsZSBkb2NzIGZvciB0aGUgcmVqZWN0aW9uIHBvbGljeSkuAAAAAAdjb25maXJtAAAAAAIAAAAAAAAABXR4X2lkAAAAAAAABgAAAAAAAAAGc2lnbmVyAAAAAAATAAAAAQAAA+kAAAACAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAL5EZXBvc2l0IGBhbW91bnRgIG9mIGB0b2tlbmAgaW50byBjdXN0b2R5LCBwdWxsaW5nIGZyb20gYGZyb21gLgoKT3JkZXJpbmc6IHRyYW5zZmVyICoqZmlyc3QqKiwgYmFsYW5jZSB3cml0ZSAqKnNlY29uZCoqIOKAlCBzZWUgdGhlCm1vZHVsZSBkb2NzIGZvciB3aHkgdGhlIGludmVyc2Ugd291bGQgYmUgYSBmdW5kLXNhZmV0eSBidWcuAAAAAAAHZGVwb3NpdAAAAAADAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAABGZyb20AAAATAAAAAAAAAAZhbW91bnQAAAAAAAsAAAABAAAD6QAAAAIAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAA55FeGVjdXRlIGEgdHJhbnNhY3Rpb24gb25jZSB0aGUgb3duZXIgYXBwcm92YWxzIG1lZXQgdGhlIHRocmVzaG9sZC4KCkNhbGxhYmxlIGJ5IGFueW9uZSBvbmNlIHRoZSB0aHJlc2hvbGQgaXMgbWV0OyBvdGhlcndpc2UgdGhlIHN0YXRlCnRyYW5zaXRpb24gaXMgcmVqZWN0ZWQuCgpGb3IgdHlwZWQgd2l0aGRyYXdhbCB0eHMgdGhpcyBtb3ZlcyB0aGUgcmVjb3JkZWQgdG9rZW5zIHRvIHRoZQpyZWNvcmRlZCBkZXN0aW5hdGlvbiBiZWZvcmUgdGhlIHN0YXR1cyBmbGlwcyAodHJhbnNmZXIgZmlyc3QsIHN0YXRlCnNlY29uZCDigJQgc2VlIHRoZSBtb2R1bGUgZG9jcykuIFRoZSB3YWxsZXQgYmFsYW5jZSBpcyB2YWxpZGF0ZWQKYWdhaW5zdCB0aGUgcmVjb3JkZWQgYW1vdW50IGZpcnN0IChgSW5zdWZmaWNpZW50RnVuZHNgKSwgc28gYQp3aXRoZHJhd2FsIHRoYXQgZXhjZWVkcyBjdXN0b2R5IGZhaWxzIHdpdGggYmFsYW5jZXMgYW5kIHR4IHN0YXRlCnVudG91Y2hlZC4KCkZvciBvcGFxdWUtcGF5bG9hZCB0eHMgdGhpcyBwZXJmb3JtcyBhIHJlYWwgY3Jvc3MtY29udHJhY3QKaW52b2NhdGlvbiB0byB0aGUgcmVjb3JkZWQgYHRhcmdldGAgd2l0aCB0aGUgc3RvcmVkIGBwYXlsb2FkYC4KVGhlIGludm9jYXRpb24gaXMgYXR0ZW1wdGVkICoqYmVmb3JlKiogdGhlIHN0YXR1cyBmbGlwcy4gQQp0YXJnZXQgcmV2ZXJ0IHN1cmZhY2VzIGFzIFtgRm9yZ2VFcnJvcjo6Q29udHJhY3RJbnZvY2F0aW9uRmFpbGVkYF0KYW5kIGxlYXZlcyB0aGUgdHJhbnNhY3Rpb24gdW4tZXhlY3V0ZWQgKHN0YXR1cyBzdGF5cyBgUGVuZGluZ2ApLgoKRXhlY3V0aW9uIGlzIHJlZnVzZWQgd2hpbGUgdGhlIHR4IGNhcnJpZXMgKiphbnkqKiByZWplY3Rpb24sIGV2ZW4KYmVsb3cgdGhlIHJlamVjdGlvbiB0aHJlc2hvbGQgKHNlZSB0aGUgbW9kdWxlIGRvY3MpLgAAAAAAB2V4ZWN1dGUAAAAAAQAAAAAAAAAFdHhfaWQAAAAAAAAGAAAAAQAAA+kAAAACAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAQAAAKpBIHBlci10b2tlbiByb2xsaW5nIHdpdGhkcmF3YWwgbGltaXQ6IGF0IG1vc3QgYGFtb3VudGAgb2YgYHRva2VuYCBtYXkKbGVhdmUgY3VzdG9keSBpbiBhbnkgYHdpbmRvd19zZWNvbmRzYCB3aW5kb3cgKHNlZSB0aGUgd2l0aGRyYXdhbC1saW1pdHMKc2VjdGlvbiBpbiB0aGUgbW9kdWxlIGRvY3MpLgAAAAAAAAAAAA9XaXRoZHJhd2FsTGltaXQAAAAAAwAAAEFNYXhpbXVtIHRvdGFsIHRoYXQgbWF5IGxlYXZlIGN1c3RvZHkgaW5zaWRlIG9uZSB3aW5kb3c7IHBvc2l0aXZlLgAAAAAAAAZhbW91bnQAAAAAAAsAAAAiU0VQLTQxIHRva2VuIHRoZSBsaW1pdCBhcHBsaWVzIHRvLgAAAAAABXRva2VuAAAAAAAAEwAAADJMZW5ndGggb2YgdGhlIHJvbGxpbmcgd2luZG93IGluIHNlY29uZHM7IHBvc2l0aXZlLgAAAAAADndpbmRvd19zZWNvbmRzAAAAAAAG",
        "AAAAAAAAAG5DaGVjayB3aGV0aGVyIGBhZGRyZXNzYCBpcyBhIG1lbWJlciBvZiB0aGUgb3duZXIgc2V0IChyZWFkLW9ubHkgdmlldykuCgpVbmluaXRpYWxpemVkIHdhbGxldHMgcmVhZCBhcyBgZmFsc2VgLgAAAAAACGlzX293bmVyAAAAAQAAAAAAAAAHYWRkcmVzcwAAAAATAAAAAQAAAAE=",
        "AAAAAAAAAMtQcm9wb3NlIGFkZGluZyBgbmV3X293bmVyYCB0byB0aGUgb3duZXIgc2V0IChzZWUgdGhlIHRyYWl0IGRvY3MpLgoKQ3JlYXRlcyBvbmx5IGEgcGVuZGluZyBnb3Zlcm5hbmNlIFtgVHhLaW5kOjpBZGRPd25lcmBdIHR4OyB0aGUgb3duZXIKc2V0IGlzIHVudG91Y2hlZCB1bnRpbCB0aGUgdGhyZXNob2xkLWFwcHJvdmVkIGBleGVjdXRlYCBhcHBsaWVzIGl0LgAAAAAJYWRkX293bmVyAAAAAAAAAgAAAAAAAAAJc3VibWl0dGVyAAAAAAAAEwAAAAAAAAAJbmV3X293bmVyAAAAAAAAEwAAAAEAAAPpAAAABgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAGRQZXJtaXNzaW9ubGVzcyBrZWVwZXI6IGJ1bXAgdGhlIGJhbGFuY2UgZW50cnkncyBUVEwgd2l0aG91dCBjaGFuZ2luZwphbnkgc3RhdGUgKHNlZSB0aGUgdHJhaXQgZG9jcykuAAAACXRvdWNoX3R0bAAAAAAAAAEAAAAAAAAABXRva2VuAAAAAAAAEwAAAAEAAAPpAAAAAgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAJVSZWFkIHRoZSBjb25maWd1cmVkIG93bmVyIHNldCwgaW4gaW5pdGlhbGl6YXRpb24gb3JkZXIgKHJlYWQtb25seSB2aWV3KS4KCiMgRXJyb3JzCgoqIFtgRm9yZ2VFcnJvcjo6Tm90SW5pdGlhbGl6ZWRgXSDigJQgdGhlIHdhbGxldCBoYXMgbm8gb3duZXIgc2V0LgAAAAAAAApnZXRfb3duZXJzAAAAAAAAAAAAAQAAA+kAAAPqAAAAEwAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAANNDb25maWd1cmUgdGhlIHdhbGxldCBmb3IgdGhlIGZpcnN0IGFuZCBvbmx5IHRpbWUuCgpSZXF1aXJlcyBhIG5vbi1lbXB0eSBvd25lciBzZXQgd2l0aCBubyBkdXBsaWNhdGVzIGFuZApgMCA8IHRocmVzaG9sZCA8PSBvd25lcnMubGVuKClgLiBUaGUgY2FsbGVyIGRlcGxveXMgdGhlIHdhbGxldCBhbmQKaW5pdGlhbGlzZXMgaXQgaW4gdGhlIHNhbWUgdHJhbnNhY3Rpb24uAAAAAAppbml0aWFsaXplAAAAAAACAAAAAAAAAAZvd25lcnMAAAAAA+oAAAATAAAAAAAAAAl0aHJlc2hvbGQAAAAAAAAEAAAAAQAAA+kAAAACAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAOpSZWFkIHRoZSBudW1iZXIgb2YgdHJhbnNhY3Rpb25zIHN1Ym1pdHRlZCBzbyBmYXIgKHJlYWQtb25seSB2aWV3KS4KClRoZSBgQ291bnRgIGNvdW50ZXIgb25seSBhZHZhbmNlcyBvbiBzdWNjZXNzZnVsIGBzdWJtaXRgLwpgc3VibWl0X3dpdGhkcmF3YWxgLCBzbyB0aGlzIG1hdGNoZXMgdGhlIG51bWJlciBvZiByZWNvcmRlZAp0cmFuc2FjdGlvbnMuIFVuaW5pdGlhbGl6ZWQgd2FsbGV0cyByZWFkIGFzIGAwYC4AAAAAAAxnZXRfdHhfY291bnQAAAAAAAAAAQAAAAY=",
        "AAAAAAAAAUZQcm9wb3NlIHJlbW92aW5nIGBleGlzdGluZ19vd25lcmAgZnJvbSB0aGUgb3duZXIgc2V0IChzZWUgdGhlIHRyYWl0CmRvY3MpLgoKQ3JlYXRlcyBvbmx5IGEgcGVuZGluZyBnb3Zlcm5hbmNlIFtgVHhLaW5kOjpSZW1vdmVPd25lcmBdIHR4OyB0aGUKb3duZXIgc2V0IGlzIHVudG91Y2hlZCB1bnRpbCB0aGUgdGhyZXNob2xkLWFwcHJvdmVkIGBleGVjdXRlYCBhcHBsaWVzCml0LiBSZW1vdmluZyB0aGUgZmluYWwgb3duZXIgaXMgcmVqZWN0ZWQgYXQgc3VibWlzc2lvbiBzbyBhIHBlbmRpbmcKcHJvcG9zYWwgY2FuIG5ldmVyIGxlYXZlIHRoZSB3YWxsZXQgb3duZXJsZXNzLgAAAAAADHJlbW92ZV9vd25lcgAAAAIAAAAAAAAACXN1Ym1pdHRlcgAAAAAAABMAAAAAAAAADmV4aXN0aW5nX293bmVyAAAAAAATAAAAAQAAA+kAAAAGAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAADhSZWFkIHRoZSBjb25maWd1cmVkIGFwcHJvdmFsIHRocmVzaG9sZCAocmVhZC1vbmx5IHZpZXcpLgAAAA1nZXRfdGhyZXNob2xkAAAAAAAAAAAAAAEAAAPpAAAABAAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAARJQcm9wb3NlIGNoYW5naW5nIHRoZSBhcHByb3ZhbCB0aHJlc2hvbGQgdG8gYG5ld190aHJlc2hvbGRgIChzZWUgdGhlCnRyYWl0IGRvY3MpLgoKQ3JlYXRlcyBvbmx5IGEgcGVuZGluZyBnb3Zlcm5hbmNlIFtgVHhLaW5kOjpTZXRUaHJlc2hvbGRgXSB0eC4gVGhlCnRocmVzaG9sZCBpcyB2YWxpZGF0ZWQgYWdhaW5zdCB0aGUgY3VycmVudCBvd25lciBzZXQgYXQgc3VibWlzc2lvbiBhbmQKcmUtdmFsaWRhdGVkIGFnYWluc3QgdGhlIHJlc3VsdGluZyBzdGF0ZSBhdCBleGVjdXRpb24uAAAAAAANc2V0X3RocmVzaG9sZAAAAAAAAAIAAAAAAAAACXN1Ym1pdHRlcgAAAAAAABMAAAAAAAAADW5ld190aHJlc2hvbGQAAAAAAAAEAAAAAQAAA+kAAAAGAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAM1SZWFkIHRoZSByZWplY3Rpb24gbGlzdCByZWNvcmRlZCBmb3IgYHR4X2lkYCwgaW4gdGhlIG9yZGVyIHRoZQpyZWplY3Rpb25zIHdlcmUgcmVjb3JkZWQgKHJlYWQtb25seSB0d2luIG9mIGBXYWxsZXRUeDo6cmVqZWN0aW9uc2ApLgoKIyBFcnJvcnMKCiogW2BGb3JnZUVycm9yOjpOb3RGb3VuZGBdIOKAlCBubyB0cmFuc2FjdGlvbiB3aXRoIGlkIGB0eF9pZGAuAAAAAAAADmdldF9yZWplY3Rpb25zAAAAAAABAAAAAAAAAAV0eF9pZAAAAAAAAAYAAAABAAAD6QAAA+oAAAATAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAEJSZWFkIHRyYW5zYWN0aW9ucyBpbiBhc2NlbmRpbmcgaWQgb3JkZXIsIHdpdGggYSB6ZXJvLWJhc2VkIG9mZnNldC4AAAAAABBnZXRfdHJhbnNhY3Rpb25zAAAAAgAAAAAAAAAGb2Zmc2V0AAAAAAAEAAAAAAAAAAVsaW1pdAAAAAAAAAQAAAABAAAD6QAAA+oAAAfQAAAACFdhbGxldFR4AAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAALlSZWFkIGB0b2tlbmAncyBjdXJyZW50IGluLXdpbmRvdyB3aXRoZHJhd2FsIHRvdGFsIChyZWFkLW9ubHkgdmlldykuCgpFeHBpcmVkIGVudHJpZXMgYXJlIGV4Y2x1ZGVkLiBSZWFkcyBhcyBgMGAgd2hlbiBubyBsaW1pdCBpcwpjb25maWd1cmVkLCBiZWNhdXNlIHRoZSB3aW5kb3cgaXMgZGVmaW5lZCBieSB0aGF0IGxpbWl0LgAAAAAAABBnZXRfd2luZG93X3VzYWdlAAAAAQAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAQAAAAs=",
        "AAAAAAAAANZSZWFkIHRoZSBjb25maXJtYXRpb24gbGlzdCByZWNvcmRlZCBmb3IgYHR4X2lkYCwgaW4gdGhlIG9yZGVyIHRoZQpjb25maXJtYXRpb25zIHdlcmUgcmVjb3JkZWQgKHJlYWQtb25seSB0d2luIG9mCmBXYWxsZXRUeDo6Y29uZmlybWF0aW9uc2ApLgoKIyBFcnJvcnMKCiogW2BGb3JnZUVycm9yOjpOb3RGb3VuZGBdIOKAlCBubyB0cmFuc2FjdGlvbiB3aXRoIGlkIGB0eF9pZGAuAAAAAAARZ2V0X2NvbmZpcm1hdGlvbnMAAAAAAAABAAAAAAAAAAV0eF9pZAAAAAAAAAYAAAABAAAD6QAAA+oAAAATAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAD9TdWJtaXQgYSB0b2tlbiB3aXRoZHJhd2FsIGFzIGEgcGVuZGluZyB0eCAoc2VlIHRoZSB0cmFpdCBkb2NzKS4AAAAAEXN1Ym1pdF93aXRoZHJhd2FsAAAAAAAABAAAAAAAAAAJc3VibWl0dGVyAAAAAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAAAAAAtkZXN0aW5hdGlvbgAAAAATAAAAAAAAAAZhbW91bnQAAAAAAAsAAAABAAAD6QAAAAYAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAAERSZWFkIGB0b2tlbmAncyBjb25maWd1cmVkIHJvbGxpbmcgd2l0aGRyYXdhbCBsaW1pdCAocmVhZC1vbmx5IHZpZXcpLgAAABRnZXRfd2l0aGRyYXdhbF9saW1pdAAAAAEAAAAAAAAABXRva2VuAAAAAAAAEwAAAAEAAAPoAAAH0AAAAA9XaXRoZHJhd2FsTGltaXQA",
        "AAAAAAAAAKhQcm9wb3NlIGEgcm9sbGluZyB3aXRoZHJhd2FsIGxpbWl0IGZvciBgdG9rZW5gIChzZWUgdGhlIHRyYWl0IGRvY3MpLgoKT25seSB0aGUgdGhyZXNob2xkLWFwcHJvdmVkIGBleGVjdXRlYCBhcHBsaWVzIGl0LCBzbyBhCnBhcnRpYWwtdGhyZXNob2xkIHByb3Bvc2FsIGNoYW5nZXMgbm90aGluZy4AAAAUc2V0X3dpdGhkcmF3YWxfbGltaXQAAAAEAAAAAAAAAAlzdWJtaXR0ZXIAAAAAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAAAAAAOd2luZG93X3NlY29uZHMAAAAAAAYAAAABAAAD6QAAAAYAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAAElQcm9wb3NlIHJlbW92aW5nIGB0b2tlbmAncyByb2xsaW5nIHdpdGhkcmF3YWwgbGltaXQgKHNlZSB0aGUgdHJhaXQKZG9jcykuAAAAAAAAF3JlbW92ZV93aXRoZHJhd2FsX2xpbWl0AAAAAAIAAAAAAAAACXN1Ym1pdHRlcgAAAAAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAEAAAPpAAAABgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAHFSZWFkIG1hdGNoaW5nIHRyYW5zYWN0aW9ucywgYXBwbHlpbmcgb2Zmc2V0IGFuZCBsaW1pdCB0byB0aGUgZmlsdGVyZWQKc2VxdWVuY2UgaW4gYXNjZW5kaW5nIHRyYW5zYWN0aW9uLWlkIG9yZGVyLgAAAAAAABpnZXRfdHJhbnNhY3Rpb25zX2J5X3N0YXR1cwAAAAAAAwAAAAAAAAAGc3RhdHVzAAAAAAfQAAAACFR4U3RhdHVzAAAAAAAAAAZvZmZzZXQAAAAAAAQAAAAAAAAABWxpbWl0AAAAAAAABAAAAAEAAAPpAAAD6gAAB9AAAAAIV2FsbGV0VHgAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAABQAAAAAAAAAAAAAAClR4RXhlY3V0ZWQAAAAAAAEAAAALdHhfZXhlY3V0ZWQAAAAAAwAAAAAAAAAFdHhfaWQAAAAAAAAGAAAAAQAAAAAAAAATY29uZmlybWF0aW9uc19jb3VudAAAAAAEAAAAAAAAAAAAAAAJdGhyZXNob2xkAAAAAAAABAAAAAAAAAAC",
        "AAAABQAAAAAAAAAAAAAAC1R4Q29uZmlybWVkAAAAAAEAAAAMdHhfY29uZmlybWVkAAAAAwAAAAAAAAAFdHhfaWQAAAAAAAAGAAAAAQAAAAAAAAAGc2lnbmVyAAAAAAATAAAAAAAAAAAAAAATY29uZmlybWF0aW9uc19jb3VudAAAAAAEAAAAAAAAAAI=",
        "AAAABQAAAAAAAAAAAAAAC1R4U3VibWl0dGVkAAAAAAEAAAAMdHhfc3VibWl0dGVkAAAAAwAAAAAAAAAFdHhfaWQAAAAAAAAGAAAAAQAAAAAAAAAJc3VibWl0dGVyAAAAAAAAEwAAAAAAAAAAAAAAC3BheWxvYWRfbGVuAAAAAAQAAAAAAAAAAg==",
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
    get_tx: this.txFromJSON<Result<WalletTx>>,
        reject: this.txFromJSON<Result<void>>,
        submit: this.txFromJSON<Result<u64>>,
        balance: this.txFromJSON<i128>,
        confirm: this.txFromJSON<Result<void>>,
        deposit: this.txFromJSON<Result<void>>,
        execute: this.txFromJSON<Result<void>>,
        is_owner: this.txFromJSON<boolean>,
        add_owner: this.txFromJSON<Result<u64>>,
        touch_ttl: this.txFromJSON<Result<void>>,
        get_owners: this.txFromJSON<Result<Array<string>>>,
        initialize: this.txFromJSON<Result<void>>,
        get_tx_count: this.txFromJSON<u64>,
        remove_owner: this.txFromJSON<Result<u64>>,
        get_threshold: this.txFromJSON<Result<u32>>,
        set_threshold: this.txFromJSON<Result<u64>>,
        get_rejections: this.txFromJSON<Result<Array<string>>>,
        get_transactions: this.txFromJSON<Result<Array<WalletTx>>>,
        get_window_usage: this.txFromJSON<i128>,
        get_confirmations: this.txFromJSON<Result<Array<string>>>,
        submit_withdrawal: this.txFromJSON<Result<u64>>,
        get_withdrawal_limit: this.txFromJSON<Option<WithdrawalLimit>>,
        set_withdrawal_limit: this.txFromJSON<Result<u64>>,
        remove_withdrawal_limit: this.txFromJSON<Result<u64>>,
        get_transactions_by_status: this.txFromJSON<Result<Array<WalletTx>>>
  }
}