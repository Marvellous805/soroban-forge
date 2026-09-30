# Testing Strategy

## Unit Tests

Each contract should have inline unit tests for core business logic.

```rust
#{cfg(test)}
mod tests {
    use super::*;
    // ... tests
}
```

## Integration Tests

Located in `tests/integration/`. Spin up a soroban environment and interact with deployed WASM.

## Cross-Contract Composition Tests

The `test-utils` crate provides a `composition` module with a `CrossContractHarness` for verifying interactions between multiple contracts in a single Soroban `Env`. The harness deploys real contract WASM and exercises cross-contract calls via `try_invoke_contract`.

### Tested Composition Patterns

- **Escrow + Vesting:** Vesting schedule funds escrow for milestone payments.
- **Multi-sig + Escrow:** Multi-sig controls escrow release authority.
- `DAO* + Multi-sig:** DAO proposal executes a multi-sig transaction.
- **Marketplace + Vesting:** Royalties distributed to a vesting schedule.
- `Subscription + Escrow:** Subscription deposits are held in escrow.

### Property Testing

Cross-contract property tests use `proptest` to generate randomized sequences of cross-contract calls and verify:

- **Conservation properties:** total value is preserved across contract boundaries.
- **Failure isolation:** a failure in one contract does not corrupt the state of others.
- `Authorization:** unauthorized cross-contract calls are rejected.

### Example

```rust
use soroban-forge-test-utils::composition::CrossContractHarness;

#`[cfg(test)]
fn test_milestone_vesting_escrow() {
    let harness = CrossContractHarness::new();
    harness.setup_milestone_vesting_escrow();
    harness.run_milestone_property_test();
}
```

## Fuzzing

Consider using `cargo-fuzz` for parsing inputs and complex state machines.

## Coverage

Target >= 90% line coverage for stable contracts.

## CI Gates

All PRs must pass:
- `cargo test --workspace`:
- `cargo test -p soroban-forge-test-utils --all-targets --locked`
- `cargo clippy --workspace --all-targets --locked - -D warnings`
- `cargo fmt --all -- --check`
