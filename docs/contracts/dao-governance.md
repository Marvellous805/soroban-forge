# DAO Governance Contract

On-chain proposal system with voting, bonded proposal creation, and
permissionless execution of approved opaque actions.

- **Source:** `crates/dao-governance`
- **Client:** `SorobanForgeDaoGovernanceClient` (generated)
- **Related:** [Contract index](./index.md), [Feature Status Matrix](../FEATURE-STATUS.md), [Known Limitations](../KNOWN-LIMITATIONS.md), [Multi-Sig Wallet](./multi-sig-wallet.md)

## Interface

```rust
fn configure_bond(token, amount, treasury) -> Result<(), ForgeError>
fn get_bond_config() -> Result<BondConfig, ForgeError>
fn configure_category_rules(rules) -> Result<(), ForgeError>
fn get_category_rules(category) -> Result<CategoryRules, ForgeError>
fn propose(proposer, target, action, duration) -> Result<u64, ForgeError>
fn propose_with_category(proposer, target, action, category) -> Result<u64, ForgeError>
fn vote(proposal_id, voter, support) -> Result<(), ForgeError>
fn execute(proposal_id) -> Result<(), ForgeError>
fn cancel_proposal(proposal_id, proposer) -> Result<(), ForgeError>
fn get_proposal(proposal_id) -> Result<Proposal, ForgeError>
fn get_proposal_count() -> u64
fn get_proposals(offset, limit) -> Result<Vec<Proposal>, ForgeError>
fn has_voted(proposal_id, voter) -> Result<bool, ForgeError>
fn touch_ttl(proposal_id) -> Result<(), ForgeError>
```

`action` is forwarded as a single `Bytes` argument to the target contract's
`execute` entrypoint. Voting remains one vote per voter. `propose` is retained
as a Standard-category compatibility shortcut; `propose_with_category` selects
the category's configured voting period. After the deadline, the first
`execute` call finalises the vote; a succeeded proposal is dispatched by a
subsequent permissionless `execute` call once its configured execution delay
has elapsed. Only a successful target invocation changes `Succeeded` to
`Executed`. A target revert returns
`ForgeError::ContractInvocationFailed` and leaves the proposal retryable in
`Succeeded`.

## Proposal categories

Call `configure_category_rules` once during deployment with exactly one
`CategoryRules` record for each category. The configuration is permissionless
and immutable; as with the bond setup, the deployer should configure it in the
deployment transaction before opening the contract to callers. Proposals are
rejected with `ForgeError::NotInitialized` until rules are configured.

Each record contains a voting period in seconds, a minimum number of votes
cast for quorum, an approval threshold in basis points, and an execution delay
in seconds. The electorate is not token-weighted, so quorum counts unique
voters. The approval threshold is checked against all votes cast. For example,
`6_667` requires at least 66.67% of votes to support the proposal.

| Category | Example period | Example quorum | Example approval | Example delay |
|---|---:|---:|---:|---:|
| `Standard` | 1 day | 1 | 50.01% | 0 |
| `Financial` | 2 days | 2 | 66.67% | 1 hour |
| `Governance` | 7 days | 3 | 75% | 1 day |
| `Emergency` | 1 hour | 1 | 50.01% | 0 |

These are recommended example values, not hard-coded contract defaults. A
proposal becomes `Defeated` if it misses quorum or its approval threshold.
`execute_after` is recorded as `voting_ends + execution_delay`; a premature
dispatch attempt returns `ForgeError::InvalidInput` without changing the
`Succeeded` proposal.

The DAO call itself is permissionless after voting has ended. A target's own
`require_auth` is not implicitly satisfied by the DAO's cross-contract call;
targets that require authorization must receive an authorization path that
the target contract accepts (see [Execute dispatch](#execute-dispatch)).

## State machine

Proposals progress through the following states, driven entirely by
`propose`, `vote`, `execute`, and `cancel_proposal`:

| State | Meaning | Entered from | Exits to |
|---|---|---|---|
| `Active` | Voting in progress | `propose` | `Succeeded`, `Defeated`, `Cancelled` |
| `Succeeded` | Quorum and category approval threshold met after the deadline | `execute` (finalisation) | `Executed` |
| `Defeated` | Quorum or category approval threshold missed | `execute` (finalisation) | — (terminal) |
| `Executed` | Target dispatch succeeded | `execute` (dispatch) | — (terminal) |
| `Cancelled` | Withdrawn by the original proposer | `cancel_proposal` | — (terminal) |
| `Queued` | Reserved for an optional timelock; **not reachable** through the public interface | — | — |

```text
propose_with_category (voting_ends = now + category.voting_period)
  --> Active --vote × n--> deadline passes
  --> execute: quorum + approval threshold ? Succeeded : Defeated
  --> wait until execute_after = voting_ends + category.execution_delay
  --> execute (on Succeeded): target.execute(action) --> Executed (terminal)
  --> cancel (proposer only): Cancelled (terminal)
```

Transition rules enforced by the contract:

- `vote` requires the proposal to be `Active` and the deadline not yet
  reached (`ForgeError::DeadlineReached` otherwise). One vote per voter.
- `execute` before the deadline is `ForgeError::InvalidInput`.
- On `Active` past deadline, `execute` finalises: quorum and the category's
  approval threshold must both be met for `Succeeded` (bond stays in custody);
  otherwise → `Defeated` (bond forfeited to the treasury).
- A `Succeeded` proposal is dispatched by a **second, separate** `execute`
  call, no earlier than `execute_after`; dispatch is described in
  [Execute dispatch](#execute-dispatch).
- `Defeated`, `Executed`, `Cancelled`, and still-`Active` proposals reject
  `execute` with `ForgeError::InvalidInput`.
- `cancel_proposal` requires the original proposer
  (`ForgeError::Unauthorized` otherwise) and an `Active` proposal; it works
  even after the deadline and after quorum is met, as long as the proposal
  has not been executed or cancelled.

## Proposal bonds

Every proposal is backed by a bond in a SEP-41 token: paid when the
proposal is created, settled when it reaches a terminal state.

**Configuration.** `configure_bond(token, amount, treasury)` and
`configure_category_rules(rules)` are one-time, permissionless writes — first
caller wins, later calls return `ForgeError::AlreadyInitialized`. Configure
both during deployment before opening the contract to callers. The treasury
address is fixed at bond setup; category rules must contain exactly one valid
record for each category. Proposals return `ForgeError::NotInitialized` until
both configurations exist.

**Posting.** `propose` pulls `amount` of `token` from the proposer into
contract custody *before* any state write, then stores `bond_token`,
`bond_amount`, and `bond_state = Posted` on the proposal and increments the
running custody total (`BondHeld`). A proposer without sufficient balance
fails with `ForgeError::TokenTransferFailed` and no proposal is created.

**Settlement.** Release happens in the same frame as the single terminal
transition — there is no separate refund call, and a bond can only be
released once:

| Transition | Trigger | Bond |
|---|---|---|
| `Active → Succeeded` | quorum and category approval threshold after the deadline | stays in custody (`Posted`) |
| `Succeeded → Executed` | successful target dispatch | refunded to the proposer (`Refunded`) |
| `Active → Defeated` | quorum or category approval threshold missed | forfeited to the configured treasury (`Forfeited`) |
| `Active → Cancelled` | proposer revokes the proposal | refunded to the proposer (`Refunded`) |

`BondState` mirrors this exactly: `Posted` (in custody), `Refunded` (back
with the proposer), or `Forfeited` (paid to the treasury).

**Ordering and failure.** Each path validates first (proposal state,
deadline, checked custody arithmetic), then moves the token, then writes
state and emits events — the same transfer-before-state discipline as the
[escrow contract](./escrow.md). A failed transfer reverts the entire
invocation — including a target dispatch that already ran — so the proposal
stays retryable and the custody total never drifts. Token failures are
bucketed as `ForgeError::TokenTransferFailed`; custody arithmetic that would
cross the `i128` boundary surfaces as `ForgeError::ArithmeticOverflow`. The
outgoing transfers need no external signer: contract self-authorization is
implicit in Soroban.

## Execute dispatch

Delivering an approved action is a two-step process, both steps
permissionless and keyed by `execute(proposal_id)`:

1. **Finalisation** (proposal `Active` past the deadline): the tally is
  frozen. Meeting quorum and the category approval threshold moves the proposal to `Succeeded` — the
   bond remains in custody and **no target call is made yet**. Otherwise the
   proposal becomes `Defeated` and the bond is forfeited.
2. **Dispatch** (proposal `Succeeded`): the contract performs a real
   cross-contract call to `proposal.target`'s `execute` entrypoint with the
   stored `action` payload as its sole argument:

   ```rust
   let args = soroban_sdk::vec![&env, proposal.action.into_val(&env)];
   let result = env.try_invoke_contract::<(), ForgeError>(
       &proposal.target,
       &Symbol::new(&env, "execute"),
       args,
   );
   ```

   - **On success:** the bond is refunded to the proposer, the proposal
     transitions to `Executed`, and `Finalised` + `BondReleased` events are
     emitted. The proposal is now terminal.
   - **On target revert:** `ForgeError::ContractInvocationFailed` is
     returned, the proposal **stays `Succeeded`**, the bond stays in
     custody, and the target's state is unchanged — the dispatch can be
     re-attempted by anyone.
   - **On refund failure:** the whole invocation reverts, including the
     already-executed target dispatch; the proposal stays `Succeeded` and
     retryable.

Because the tuple return is `Result<Result<(), ForgeError>, HostError>`, a
host-level abort (e.g. an undeployed target or a missing entrypoint) is
indistinguishable from a typed revert and both are surfaced as
`ForgeError::ContractInvocationFailed`.

**Target contract shape.** Any contract that exposes a public
`fn execute(env: Env, action: Bytes)` entrypoint can be a DAO target — see
`MockTarget`/`RevertingTarget` in the crate's test suite and the
`AuthCheckingTarget` test, which demonstrates that a target's own
`require_auth` is *not* satisfied by the DAO's call and causes
`ContractInvocationFailed`.

## End-to-end walkthrough

A full lifecycle, from deployment to on-chain effect:

1. **Deploy + configure.** Register `DaoGovernance`, then call
  `configure_bond(&bond_token, &100, &treasury)` and
  `configure_category_rules(&rules)` in the deploy transaction. Both
  configurations are immutable; there is no admin role. Proposals are
  rejected until both calls complete.

2. **Create a proposal.** A member calls
  `propose_with_category(&proposer, &target, &action_payload, &category)`
  (or the Standard-only `propose` compatibility method). The contract
  transfers 100 units of the bond token from the proposer into custody,
  assigns the next stable `proposal_id`, records the proposal as `Active`
  with category-defined `voting_ends` and `execute_after`, and emits `Proposed` and
   `BondPosted`. The proposer's signature covers the nested bond pull.

3. **Vote.** Each member calls `vote(&proposal_id, &voter, &support)` once.
   The contract tallies `for_votes`/`against_votes`, emits `VoteCast`, and
   rejects a second vote for the same voter, votes after the deadline, and
   votes on non-`Active` proposals.

4. **Finalise.** After `voting_ends`, anyone (not just voters — the
  proposal creator or an observer) calls `execute(&proposal_id)`. Meeting
  quorum and the category approval threshold moves the proposal to
  `Succeeded` (`Finalised` event); otherwise it becomes `Defeated` and the bond is forfeited to the
   treasury (`Finalised` + `BondReleased` with `forfeited: true`).

5. **Dispatch.** Anyone calls `execute(&proposal_id)` again. The contract
   invokes `target.execute(action)`. On success it refunds the bond to the
   proposer, marks the proposal `Executed`, and emits `Finalised` +
   `BondReleased` with `forfeited: false`. On failure it stays `Succeeded`
   and can be retried.

6. **Cancel (alternative).** While the proposal is still `Active`, the
   original proposer may call `cancel_proposal(&proposal_id, &proposer)` to
   refund the bond and freeze the proposal as `Cancelled` — even after the
   deadline, as long as it has not been executed.

7. **Keep alive (optional).** A keeper periodically calls
   `touch_ttl(&proposal_id)` to extend the 30-day TTL horizon of long-lived
   proposals (see [Storage & TTL](#storage--ttl-maintenance)).

Indexers reconstruct the full lifecycle from the event stream alone:
`Proposed` → `VoteCast` × n → `Finalised` → (optional `BondReleased`), each
keyed by `proposal_id`.

## Events

The contract emits typed on-chain lifecycle events for indexers and off-chain
monitoring, each keyed by the standard `proposal_id` topic:

- **`Proposed`** — emitted by `propose` once the proposal record and bond
  are in place.
  - Topics: `proposal_id: u64`
  - Data: `data: Proposal` (the full initial record)
- **`VoteCast`** — emitted by `vote` for each accepted vote.
  - Topics: `proposal_id: u64`
  - Data: `voter: Address`, `support: bool`
- **`Finalised`** — emitted by `execute` on every state transition
  (`Active` → `Succeeded`, `Active` → `Defeated`, `Succeeded` → `Executed`).
  - Topics: `proposal_id: u64`
  - Data: `state: ProposalState`, `for_votes: i128`, `against_votes: i128`
- **`BondPosted`** — emitted by `propose` once the bond is in custody.
  - Topics: `proposal_id: u64`
  - Data: `token: Address`, `amount: i128`
- **`BondReleased`** — emitted by `execute` (refund or forfeit) and
  `cancel_proposal` (refund) when a bond leaves custody.
  - Topics: `proposal_id: u64`
  - Data: `token: Address`, `amount: i128`, `to: Address`,
    `forfeited: bool` (`true` = sent to the treasury)

Whenever a bond moves, the bond token's own `transfer` event appears in the
same invocation. Indexers should filter events by the emitting contract
address to separate the DAO's records from the token's.

## Storage & TTL Maintenance

Proposal records (`DataKey::Proposal(u64)`) are stored in persistent storage. `DataKey::Count`, `DataKey::Bond`, `DataKey::BondHeld`, and `DataKey::Vote` entries remain in instance storage.

`propose`, `vote`, `execute`, and `cancel_proposal` extend proposal persistent storage TTL on every write to a 30-day horizon (`30 * DAY_IN_LEDGERS = 518,400` ledgers).

A permissionless public keeper entrypoint `touch_ttl(proposal_id)` allows anyone to bump a proposal's persistent TTL without modifying its state. If the proposal ID does not exist, `touch_ttl` returns `ForgeError::NotFound`.