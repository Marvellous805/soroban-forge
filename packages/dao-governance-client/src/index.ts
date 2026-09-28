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
 * A single governance proposal.
 */
export interface Proposal {
  /**
 * Encoded action to execute on success.
 */
action: Buffer;
  /**
 * Tally of "against" votes (in governance-token units).
 */
against_votes: i128;
  /**
 * Bond amount posted at creation; the exact figure refunded or
 * forfeited on the terminal transition.
 */
bond_amount: i128;
  /**
 * Lifecycle of this proposal's bond. Moves in the same frame as the
 * terminal proposal state, exactly once.
 */
bond_state: BondState;
  /**
 * SEP-41 token this proposal's bond was posted in (the configured
 * token at creation time).
 */
bond_token: string;
  /**
 * Tally of "for" votes (in governance-token units).
 */
for_votes: i128;
  /**
 * Stable identifier assigned at creation.
 */
proposal_id: u64;
  /**
 * Address that created the proposal.
 */
proposer: string;
  /**
 * Current state.
 */
state: ProposalState;
  /**
 * Target contract to invoke on successful execution.
 */
target: string;
  /**
 * Ledger timestamp at which voting closes.
 */
voting_ends: u64;
}

/**
 * Lifecycle state of the bond posted for a proposal.
 */
export type BondState = {tag: "Posted", values: void} | {tag: "Refunded", values: void} | {tag: "Forfeited", values: void};


/**
 * The one-time proposal-bond configuration.
 * 
 * Written once by [`DaoGovernance::configure_bond`] and immutable
 * afterwards; read by `propose` (what to pull) and by the terminal
 * transitions (where a forfeit goes).
 */
export interface BondConfig {
  /**
 * Amount of `token` pulled from the proposer on `propose`.
 */
amount: i128;
  /**
 * SEP-41 token every proposal must post as a bond.
 */
token: string;
  /**
 * Receives bonds forfeited by defeated proposals. Never receives
 * refunds and holds no other authority.
 */
treasury: string;
}

/**
 * Lifecycle state of a governance proposal.
 */
export type ProposalState = {tag: "Active", values: void} | {tag: "Succeeded", values: void} | {tag: "Defeated", values: void} | {tag: "Executed", values: void} | {tag: "Queued", values: void} | {tag: "Cancelled", values: void};







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
   * Construct and simulate a vote transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Cast a vote on an active proposal.
   * 
   * Requires the voter. Each voter may vote exactly once; voting is closed
   * once the deadline (`voting_ends`) passes.
   */
  vote: ({proposal_id, voter, support}: {proposal_id: u64, voter: string, support: boolean}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a execute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Finalise a proposal once voting has ended, and execute passed proposals.
   * 
   * Callable by anyone after the deadline (permissionless execution), and
   * — on the paths that release a bond — permissionless in the token
   * sense too: contract self-authorization covers the outgoing transfer.
   * 
   * - An `Active` proposal past deadline transitions to `Succeeded` on a
   * strict majority of `for` votes (the bond stays in custody until a
   * terminal transition), or to `Defeated` otherwise — forfeiting the
   * bond to the treasury **before** the state write.
   * - A `Succeeded` proposal performs a real cross-contract call to `target`
   * with `action` (`target.execute(action)`). On successful invocation,
   * it refunds the bond to the proposer, then transitions to the
   * terminal `Executed` state and emits an `Executed` event.
   * - If the target invocation reverts, `ForgeError::ContractInvocationFailed`
   * is returned and the proposal remains `Succeeded` (not `Executed`),
   * leaving target state unchanged and the bond in custody.
   * - If a bond refund/forfeit trans
   */
  execute: ({proposal_id}: {proposal_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a propose transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Create a new proposal and return its stable id.
   * 
   * Requires `duration > 0` and a configured bond. The proposer is
   * authorized at creation time, and their authorization covers the
   * nested bond pull.
   * 
   * Ordering (load-bearing): the bond transfer runs **before** the id
   * counter, the proposal record, and the custody total are written, so a
   * failed transfer leaves no proposal record — see the module docs.
   */
  propose: ({proposer, target, action, duration}: {proposer: string, target: string, action: Buffer, duration: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<u64>>>

  /**
   * Construct and simulate a has_voted transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Check whether `voter` has voted on `proposal_id` (read-only view).
   * 
   * Returns `ForgeError::NotFound` if `proposal_id` does not exist.
   */
  has_voted: ({proposal_id, voter}: {proposal_id: u64, voter: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<boolean>>>

  /**
   * Construct and simulate a get_proposal transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read a stored proposal by id (read-only view).
   */
  get_proposal: ({proposal_id}: {proposal_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Proposal>>>

  /**
   * Construct and simulate a get_proposals transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read a paginated slice of proposals ordered by proposal ID (read-only view).
   * 
   * Bounds clamping:
   * - `limit == 0` returns `ForgeError::InvalidInput`.
   * - If `offset >= total`, returns an empty `Vec`.
   * - Returns at most `limit` items without overflowing.
   */
  get_proposals: ({offset, limit}: {offset: u32, limit: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<Proposal>>>>

  /**
   * Construct and simulate a configure_bond transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Configure the proposal bond for the first and only time.
   * 
   * Permissionless one-shot (see [`SorobanForgeDaoGovernance::configure_bond`]):
   * the deployer calls it in the deploy transaction and the configuration
   * never changes afterwards, so no privileged role exists.
   */
  configure_bond: ({token, amount, treasury}: {token: string, amount: i128, treasury: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a cancel_proposal transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Withdraw an active proposal before it is executed.
   * 
   * Requires the original proposer. A proposal may be cancelled even after
   * voting ends and quorum is met, as long as it has not been executed (or
   * already cancelled). A cancelled proposal is terminal: further votes and
   * execution are rejected.
   * 
   * The bond is refunded to the proposer in this same call: transfer
   * first, then the `Cancelled` state write — a failed refund surfaces
   * `TokenTransferFailed` with the proposal still `Active`.
   * 
   * * [`ForgeError::NotFound`] — no proposal with this id.
   * * [`ForgeError::Unauthorized`] — `proposer` is not the original proposer.
   * * [`ForgeError::InvalidInput`] — the proposal is no longer `Active`.
   * * [`ForgeError::TokenTransferFailed`] — the bond refund failed.
   */
  cancel_proposal: ({proposal_id, proposer}: {proposal_id: u64, proposer: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_bond_config transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Read the bond configuration (read-only view).
   * 
   * Returns `ForgeError::NotInitialized` while the contract has no bond
   * configuration — the same precondition `propose` enforces.
   */
  get_bond_config: (options?: MethodOptions) => Promise<AssembledTransaction<Result<BondConfig>>>

  /**
   * Construct and simulate a get_proposal_count transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Return the total number of proposals created (read-only view).
   */
  get_proposal_count: (options?: MethodOptions) => Promise<AssembledTransaction<u64>>

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
      new ContractSpec([ "AAAAAQAAAB1BIHNpbmdsZSBnb3Zlcm5hbmNlIHByb3Bvc2FsLgAAAAAAAAAAAAAIUHJvcG9zYWwAAAALAAAAJUVuY29kZWQgYWN0aW9uIHRvIGV4ZWN1dGUgb24gc3VjY2Vzcy4AAAAAAAAGYWN0aW9uAAAAAAAOAAAANVRhbGx5IG9mICJhZ2FpbnN0IiB2b3RlcyAoaW4gZ292ZXJuYW5jZS10b2tlbiB1bml0cykuAAAAAAAADWFnYWluc3Rfdm90ZXMAAAAAAAALAAAAYkJvbmQgYW1vdW50IHBvc3RlZCBhdCBjcmVhdGlvbjsgdGhlIGV4YWN0IGZpZ3VyZSByZWZ1bmRlZCBvcgpmb3JmZWl0ZWQgb24gdGhlIHRlcm1pbmFsIHRyYW5zaXRpb24uAAAAAAALYm9uZF9hbW91bnQAAAAACwAAAGhMaWZlY3ljbGUgb2YgdGhpcyBwcm9wb3NhbCdzIGJvbmQuIE1vdmVzIGluIHRoZSBzYW1lIGZyYW1lIGFzIHRoZQp0ZXJtaW5hbCBwcm9wb3NhbCBzdGF0ZSwgZXhhY3RseSBvbmNlLgAAAApib25kX3N0YXRlAAAAAAfQAAAACUJvbmRTdGF0ZQAAAAAAAFhTRVAtNDEgdG9rZW4gdGhpcyBwcm9wb3NhbCdzIGJvbmQgd2FzIHBvc3RlZCBpbiAodGhlIGNvbmZpZ3VyZWQKdG9rZW4gYXQgY3JlYXRpb24gdGltZSkuAAAACmJvbmRfdG9rZW4AAAAAABMAAAAxVGFsbHkgb2YgImZvciIgdm90ZXMgKGluIGdvdmVybmFuY2UtdG9rZW4gdW5pdHMpLgAAAAAAAAlmb3Jfdm90ZXMAAAAAAAALAAAAJ1N0YWJsZSBpZGVudGlmaWVyIGFzc2lnbmVkIGF0IGNyZWF0aW9uLgAAAAALcHJvcG9zYWxfaWQAAAAABgAAACJBZGRyZXNzIHRoYXQgY3JlYXRlZCB0aGUgcHJvcG9zYWwuAAAAAAAIcHJvcG9zZXIAAAATAAAADkN1cnJlbnQgc3RhdGUuAAAAAAAFc3RhdGUAAAAAAAfQAAAADVByb3Bvc2FsU3RhdGUAAAAAAAAyVGFyZ2V0IGNvbnRyYWN0IHRvIGludm9rZSBvbiBzdWNjZXNzZnVsIGV4ZWN1dGlvbi4AAAAAAAZ0YXJnZXQAAAAAABMAAAAoTGVkZ2VyIHRpbWVzdGFtcCBhdCB3aGljaCB2b3RpbmcgY2xvc2VzLgAAAAt2b3RpbmdfZW5kcwAAAAAG",
        "AAAAAgAAADJMaWZlY3ljbGUgc3RhdGUgb2YgdGhlIGJvbmQgcG9zdGVkIGZvciBhIHByb3Bvc2FsLgAAAAAAAAAAAAlCb25kU3RhdGUAAAAAAAADAAAAAAAAAFBIZWxkIGJ5IHRoaXMgY29udHJhY3Qgc2luY2UgYHByb3Bvc2VgOyB0aGUgb25seSBzdGF0ZSBhIGxpdmUKcHJvcG9zYWwgY2FuIGJlIGluLgAAAAZQb3N0ZWQAAAAAAAAAAABYUmV0dXJuZWQgdG8gdGhlIHByb3Bvc2VyIHdoZW4gdGhlIHByb3Bvc2FsIHJlYWNoZWQgYEV4ZWN1dGVkYCBvcgpgQ2FuY2VsbGVkYCAodGVybWluYWwpLgAAAAhSZWZ1bmRlZAAAAAAAAABMU2VudCB0byB0aGUgY29uZmlndXJlZCB0cmVhc3VyeSB3aGVuIHRoZSBwcm9wb3NhbCB3YXMgYERlZmVhdGVkYAoodGVybWluYWwpLgAAAAlGb3JmZWl0ZWQAAAA=",
        "AAAAAQAAAM9UaGUgb25lLXRpbWUgcHJvcG9zYWwtYm9uZCBjb25maWd1cmF0aW9uLgoKV3JpdHRlbiBvbmNlIGJ5IFtgRGFvR292ZXJuYW5jZTo6Y29uZmlndXJlX2JvbmRgXSBhbmQgaW1tdXRhYmxlCmFmdGVyd2FyZHM7IHJlYWQgYnkgYHByb3Bvc2VgICh3aGF0IHRvIHB1bGwpIGFuZCBieSB0aGUgdGVybWluYWwKdHJhbnNpdGlvbnMgKHdoZXJlIGEgZm9yZmVpdCBnb2VzKS4AAAAAAAAAAApCb25kQ29uZmlnAAAAAAADAAAAOEFtb3VudCBvZiBgdG9rZW5gIHB1bGxlZCBmcm9tIHRoZSBwcm9wb3NlciBvbiBgcHJvcG9zZWAuAAAABmFtb3VudAAAAAAACwAAADBTRVAtNDEgdG9rZW4gZXZlcnkgcHJvcG9zYWwgbXVzdCBwb3N0IGFzIGEgYm9uZC4AAAAFdG9rZW4AAAAAAAATAAAAZFJlY2VpdmVzIGJvbmRzIGZvcmZlaXRlZCBieSBkZWZlYXRlZCBwcm9wb3NhbHMuIE5ldmVyIHJlY2VpdmVzCnJlZnVuZHMgYW5kIGhvbGRzIG5vIG90aGVyIGF1dGhvcml0eS4AAAAIdHJlYXN1cnkAAAAT",
        "AAAAAAAAAJRDYXN0IGEgdm90ZSBvbiBhbiBhY3RpdmUgcHJvcG9zYWwuCgpSZXF1aXJlcyB0aGUgdm90ZXIuIEVhY2ggdm90ZXIgbWF5IHZvdGUgZXhhY3RseSBvbmNlOyB2b3RpbmcgaXMgY2xvc2VkCm9uY2UgdGhlIGRlYWRsaW5lIChgdm90aW5nX2VuZHNgKSBwYXNzZXMuAAAABHZvdGUAAAADAAAAAAAAAAtwcm9wb3NhbF9pZAAAAAAGAAAAAAAAAAV2b3RlcgAAAAAAABMAAAAAAAAAB3N1cHBvcnQAAAAAAQAAAAEAAAPpAAAAAgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAgAAAClMaWZlY3ljbGUgc3RhdGUgb2YgYSBnb3Zlcm5hbmNlIHByb3Bvc2FsLgAAAAAAAAAAAAANUHJvcG9zYWxTdGF0ZQAAAAAAAAYAAAAAAAAAEE9wZW4gZm9yIHZvdGluZy4AAAAGQWN0aXZlAAAAAAAAAAAAIUFwcHJvdmVkIGFuZCByZWFkeSBmb3IgZXhlY3V0aW9uLgAAAAAAAAlTdWNjZWVkZWQAAAAAAAAAAAAAFFJlamVjdGVkIG9yIGV4cGlyZWQuAAAACERlZmVhdGVkAAAAAAAAACpTdWNjZXNzZnVsbHkgZXhlY3V0ZWQgb24tY2hhaW4gKHRlcm1pbmFsKS4AAAAAAAhFeGVjdXRlZAAAAAAAAAAxUXVldWVkIGZvciBkZWxheWVkIGV4ZWN1dGlvbiAob3B0aW9uYWwgdGltZWxvY2spLgAAAAAAAAZRdWV1ZWQAAAAAAAAAAABDV2l0aGRyYXduIGJ5IHRoZSBwcm9wb3NlciBiZWZvcmUgZXhlY3V0aW9uOyB0ZXJtaW5hbCBhbmQgaW1tdXRhYmxlLgAAAAAJQ2FuY2VsbGVkAAAA",
        "AAAAAAAABABGaW5hbGlzZSBhIHByb3Bvc2FsIG9uY2Ugdm90aW5nIGhhcyBlbmRlZCwgYW5kIGV4ZWN1dGUgcGFzc2VkIHByb3Bvc2Fscy4KCkNhbGxhYmxlIGJ5IGFueW9uZSBhZnRlciB0aGUgZGVhZGxpbmUgKHBlcm1pc3Npb25sZXNzIGV4ZWN1dGlvbiksIGFuZArigJQgb24gdGhlIHBhdGhzIHRoYXQgcmVsZWFzZSBhIGJvbmQg4oCUIHBlcm1pc3Npb25sZXNzIGluIHRoZSB0b2tlbgpzZW5zZSB0b286IGNvbnRyYWN0IHNlbGYtYXV0aG9yaXphdGlvbiBjb3ZlcnMgdGhlIG91dGdvaW5nIHRyYW5zZmVyLgoKLSBBbiBgQWN0aXZlYCBwcm9wb3NhbCBwYXN0IGRlYWRsaW5lIHRyYW5zaXRpb25zIHRvIGBTdWNjZWVkZWRgIG9uIGEKc3RyaWN0IG1ham9yaXR5IG9mIGBmb3JgIHZvdGVzICh0aGUgYm9uZCBzdGF5cyBpbiBjdXN0b2R5IHVudGlsIGEKdGVybWluYWwgdHJhbnNpdGlvbiksIG9yIHRvIGBEZWZlYXRlZGAgb3RoZXJ3aXNlIOKAlCBmb3JmZWl0aW5nIHRoZQpib25kIHRvIHRoZSB0cmVhc3VyeSAqKmJlZm9yZSoqIHRoZSBzdGF0ZSB3cml0ZS4KLSBBIGBTdWNjZWVkZWRgIHByb3Bvc2FsIHBlcmZvcm1zIGEgcmVhbCBjcm9zcy1jb250cmFjdCBjYWxsIHRvIGB0YXJnZXRgCndpdGggYGFjdGlvbmAgKGB0YXJnZXQuZXhlY3V0ZShhY3Rpb24pYCkuIE9uIHN1Y2Nlc3NmdWwgaW52b2NhdGlvbiwKaXQgcmVmdW5kcyB0aGUgYm9uZCB0byB0aGUgcHJvcG9zZXIsIHRoZW4gdHJhbnNpdGlvbnMgdG8gdGhlCnRlcm1pbmFsIGBFeGVjdXRlZGAgc3RhdGUgYW5kIGVtaXRzIGFuIGBFeGVjdXRlZGAgZXZlbnQuCi0gSWYgdGhlIHRhcmdldCBpbnZvY2F0aW9uIHJldmVydHMsIGBGb3JnZUVycm9yOjpDb250cmFjdEludm9jYXRpb25GYWlsZWRgCmlzIHJldHVybmVkIGFuZCB0aGUgcHJvcG9zYWwgcmVtYWlucyBgU3VjY2VlZGVkYCAobm90IGBFeGVjdXRlZGApLApsZWF2aW5nIHRhcmdldCBzdGF0ZSB1bmNoYW5nZWQgYW5kIHRoZSBib25kIGluIGN1c3RvZHkuCi0gSWYgYSBib25kIHJlZnVuZC9mb3JmZWl0IHRyYW5zAAAAB2V4ZWN1dGUAAAAAAQAAAAAAAAALcHJvcG9zYWxfaWQAAAAABgAAAAEAAAPpAAAAAgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAY1DcmVhdGUgYSBuZXcgcHJvcG9zYWwgYW5kIHJldHVybiBpdHMgc3RhYmxlIGlkLgoKUmVxdWlyZXMgYGR1cmF0aW9uID4gMGAgYW5kIGEgY29uZmlndXJlZCBib25kLiBUaGUgcHJvcG9zZXIgaXMKYXV0aG9yaXplZCBhdCBjcmVhdGlvbiB0aW1lLCBhbmQgdGhlaXIgYXV0aG9yaXphdGlvbiBjb3ZlcnMgdGhlCm5lc3RlZCBib25kIHB1bGwuCgpPcmRlcmluZyAobG9hZC1iZWFyaW5nKTogdGhlIGJvbmQgdHJhbnNmZXIgcnVucyAqKmJlZm9yZSoqIHRoZSBpZApjb3VudGVyLCB0aGUgcHJvcG9zYWwgcmVjb3JkLCBhbmQgdGhlIGN1c3RvZHkgdG90YWwgYXJlIHdyaXR0ZW4sIHNvIGEKZmFpbGVkIHRyYW5zZmVyIGxlYXZlcyBubyBwcm9wb3NhbCByZWNvcmQg4oCUIHNlZSB0aGUgbW9kdWxlIGRvY3MuAAAAAAAAB3Byb3Bvc2UAAAAABAAAAAAAAAAIcHJvcG9zZXIAAAATAAAAAAAAAAZ0YXJnZXQAAAAAABMAAAAAAAAABmFjdGlvbgAAAAAADgAAAAAAAAAIZHVyYXRpb24AAAAGAAAAAQAAA+kAAAAGAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAINDaGVjayB3aGV0aGVyIGB2b3RlcmAgaGFzIHZvdGVkIG9uIGBwcm9wb3NhbF9pZGAgKHJlYWQtb25seSB2aWV3KS4KClJldHVybnMgYEZvcmdlRXJyb3I6Ok5vdEZvdW5kYCBpZiBgcHJvcG9zYWxfaWRgIGRvZXMgbm90IGV4aXN0LgAAAAAJaGFzX3ZvdGVkAAAAAAAAAgAAAAAAAAALcHJvcG9zYWxfaWQAAAAABgAAAAAAAAAFdm90ZXIAAAAAAAATAAAAAQAAA+kAAAABAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAC5SZWFkIGEgc3RvcmVkIHByb3Bvc2FsIGJ5IGlkIChyZWFkLW9ubHkgdmlldykuAAAAAAAMZ2V0X3Byb3Bvc2FsAAAAAQAAAAAAAAALcHJvcG9zYWxfaWQAAAAABgAAAAEAAAPpAAAH0AAAAAhQcm9wb3NhbAAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAPZSZWFkIGEgcGFnaW5hdGVkIHNsaWNlIG9mIHByb3Bvc2FscyBvcmRlcmVkIGJ5IHByb3Bvc2FsIElEIChyZWFkLW9ubHkgdmlldykuCgpCb3VuZHMgY2xhbXBpbmc6Ci0gYGxpbWl0ID09IDBgIHJldHVybnMgYEZvcmdlRXJyb3I6OkludmFsaWRJbnB1dGAuCi0gSWYgYG9mZnNldCA+PSB0b3RhbGAsIHJldHVybnMgYW4gZW1wdHkgYFZlY2AuCi0gUmV0dXJucyBhdCBtb3N0IGBsaW1pdGAgaXRlbXMgd2l0aG91dCBvdmVyZmxvd2luZy4AAAAAAA1nZXRfcHJvcG9zYWxzAAAAAAAAAgAAAAAAAAAGb2Zmc2V0AAAAAAAEAAAAAAAAAAVsaW1pdAAAAAAAAAQAAAABAAAD6QAAA+oAAAfQAAAACFByb3Bvc2FsAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAQRDb25maWd1cmUgdGhlIHByb3Bvc2FsIGJvbmQgZm9yIHRoZSBmaXJzdCBhbmQgb25seSB0aW1lLgoKUGVybWlzc2lvbmxlc3Mgb25lLXNob3QgKHNlZSBbYFNvcm9iYW5Gb3JnZURhb0dvdmVybmFuY2U6OmNvbmZpZ3VyZV9ib25kYF0pOgp0aGUgZGVwbG95ZXIgY2FsbHMgaXQgaW4gdGhlIGRlcGxveSB0cmFuc2FjdGlvbiBhbmQgdGhlIGNvbmZpZ3VyYXRpb24KbmV2ZXIgY2hhbmdlcyBhZnRlcndhcmRzLCBzbyBubyBwcml2aWxlZ2VkIHJvbGUgZXhpc3RzLgAAAA5jb25maWd1cmVfYm9uZAAAAAAAAwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAACHRyZWFzdXJ5AAAAEwAAAAEAAAPpAAAAAgAAB9AAAAAKRm9yZ2VFcnJvcgAA",
        "AAAAAAAAAu9XaXRoZHJhdyBhbiBhY3RpdmUgcHJvcG9zYWwgYmVmb3JlIGl0IGlzIGV4ZWN1dGVkLgoKUmVxdWlyZXMgdGhlIG9yaWdpbmFsIHByb3Bvc2VyLiBBIHByb3Bvc2FsIG1heSBiZSBjYW5jZWxsZWQgZXZlbiBhZnRlcgp2b3RpbmcgZW5kcyBhbmQgcXVvcnVtIGlzIG1ldCwgYXMgbG9uZyBhcyBpdCBoYXMgbm90IGJlZW4gZXhlY3V0ZWQgKG9yCmFscmVhZHkgY2FuY2VsbGVkKS4gQSBjYW5jZWxsZWQgcHJvcG9zYWwgaXMgdGVybWluYWw6IGZ1cnRoZXIgdm90ZXMgYW5kCmV4ZWN1dGlvbiBhcmUgcmVqZWN0ZWQuCgpUaGUgYm9uZCBpcyByZWZ1bmRlZCB0byB0aGUgcHJvcG9zZXIgaW4gdGhpcyBzYW1lIGNhbGw6IHRyYW5zZmVyCmZpcnN0LCB0aGVuIHRoZSBgQ2FuY2VsbGVkYCBzdGF0ZSB3cml0ZSDigJQgYSBmYWlsZWQgcmVmdW5kIHN1cmZhY2VzCmBUb2tlblRyYW5zZmVyRmFpbGVkYCB3aXRoIHRoZSBwcm9wb3NhbCBzdGlsbCBgQWN0aXZlYC4KCiogW2BGb3JnZUVycm9yOjpOb3RGb3VuZGBdIOKAlCBubyBwcm9wb3NhbCB3aXRoIHRoaXMgaWQuCiogW2BGb3JnZUVycm9yOjpVbmF1dGhvcml6ZWRgXSDigJQgYHByb3Bvc2VyYCBpcyBub3QgdGhlIG9yaWdpbmFsIHByb3Bvc2VyLgoqIFtgRm9yZ2VFcnJvcjo6SW52YWxpZElucHV0YF0g4oCUIHRoZSBwcm9wb3NhbCBpcyBubyBsb25nZXIgYEFjdGl2ZWAuCiogW2BGb3JnZUVycm9yOjpUb2tlblRyYW5zZmVyRmFpbGVkYF0g4oCUIHRoZSBib25kIHJlZnVuZCBmYWlsZWQuAAAAAA9jYW5jZWxfcHJvcG9zYWwAAAAAAgAAAAAAAAALcHJvcG9zYWxfaWQAAAAABgAAAAAAAAAIcHJvcG9zZXIAAAATAAAAAQAAA+kAAAACAAAH0AAAAApGb3JnZUVycm9yAAA=",
        "AAAAAAAAAK5SZWFkIHRoZSBib25kIGNvbmZpZ3VyYXRpb24gKHJlYWQtb25seSB2aWV3KS4KClJldHVybnMgYEZvcmdlRXJyb3I6Ok5vdEluaXRpYWxpemVkYCB3aGlsZSB0aGUgY29udHJhY3QgaGFzIG5vIGJvbmQKY29uZmlndXJhdGlvbiDigJQgdGhlIHNhbWUgcHJlY29uZGl0aW9uIGBwcm9wb3NlYCBlbmZvcmNlcy4AAAAAAA9nZXRfYm9uZF9jb25maWcAAAAAAAAAAAEAAAPpAAAH0AAAAApCb25kQ29uZmlnAAAAAAfQAAAACkZvcmdlRXJyb3IAAA==",
        "AAAAAAAAAD5SZXR1cm4gdGhlIHRvdGFsIG51bWJlciBvZiBwcm9wb3NhbHMgY3JlYXRlZCAocmVhZC1vbmx5IHZpZXcpLgAAAAAAEmdldF9wcm9wb3NhbF9jb3VudAAAAAAAAAAAAAEAAAAG",
        "AAAABQAAAAAAAAAAAAAACFByb3Bvc2VkAAAAAQAAAAhwcm9wb3NlZAAAAAIAAAAAAAAAC3Byb3Bvc2FsX2lkAAAAAAYAAAABAAAAAAAAAARkYXRhAAAH0AAAAAhQcm9wb3NhbAAAAAAAAAAC",
        "AAAABQAAAAAAAAAAAAAACFZvdGVDYXN0AAAAAQAAAAl2b3RlX2Nhc3QAAAAAAAADAAAAAAAAAAtwcm9wb3NhbF9pZAAAAAAGAAAAAQAAAAAAAAAFdm90ZXIAAAAAAAATAAAAAAAAAAAAAAAHc3VwcG9ydAAAAAABAAAAAAAAAAI=",
        "AAAABQAAAAAAAAAAAAAACUZpbmFsaXNlZAAAAAAAAAEAAAAJZmluYWxpc2VkAAAAAAAABAAAAAAAAAALcHJvcG9zYWxfaWQAAAAABgAAAAEAAAAAAAAABXN0YXRlAAAAAAAH0AAAAA1Qcm9wb3NhbFN0YXRlAAAAAAAAAAAAAAAAAAAJZm9yX3ZvdGVzAAAAAAAACwAAAAAAAAAAAAAADWFnYWluc3Rfdm90ZXMAAAAAAAALAAAAAAAAAAI=",
        "AAAABQAAADRBIGJvbmQgZW50ZXJlZCBjdXN0b2R5IHdpdGggdGhlIHByb3Bvc2FsJ3MgY3JlYXRpb24uAAAAAAAAAApCb25kUG9zdGVkAAAAAAABAAAAC2JvbmRfcG9zdGVkAAAAAAMAAAAAAAAAC3Byb3Bvc2FsX2lkAAAAAAYAAAABAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAAg==",
        "AAAABQAAAINBIGJvbmQgbGVmdCBjdXN0b2R5IG9uIGEgdGVybWluYWwgdHJhbnNpdGlvbjogYmFjayB0byB0aGUgcHJvcG9zZXIKKGBmb3JmZWl0ZWQgPT0gZmFsc2VgKSBvciB0byB0aGUgdHJlYXN1cnkgKGBmb3JmZWl0ZWQgPT0gdHJ1ZWApLgAAAAAAAAAADEJvbmRSZWxlYXNlZAAAAAEAAAANYm9uZF9yZWxlYXNlZAAAAAAAAAUAAAAAAAAAC3Byb3Bvc2FsX2lkAAAAAAYAAAABAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAAAAAAAJ0bwAAAAAAEwAAAAAAAAAAAAAACWZvcmZlaXRlZAAAAAAAAAEAAAAAAAAAAg==",
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
    vote: this.txFromJSON<Result<void>>,
        execute: this.txFromJSON<Result<void>>,
        propose: this.txFromJSON<Result<u64>>,
        has_voted: this.txFromJSON<Result<boolean>>,
        get_proposal: this.txFromJSON<Result<Proposal>>,
        get_proposals: this.txFromJSON<Result<Array<Proposal>>>,
        configure_bond: this.txFromJSON<Result<void>>,
        cancel_proposal: this.txFromJSON<Result<void>>,
        get_bond_config: this.txFromJSON<Result<BondConfig>>,
        get_proposal_count: this.txFromJSON<u64>
  }
}