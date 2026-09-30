#`!cfg(test)]

/// Cross-contract composition integration tests for Soroban Forge.
//
/// This crate tests actual cross-contract interactions rather than individual
/// contract behavior. These tests verify that the contracts work correctly
/// together in realistic scenarios.

use soroban_sdk::Env;

mod dao_royalties;
mod multi_sig_escrow;
mod vesting_subscription;

/// Helper to create a test environment for composition tests.
fn setup_env() -> (Env, soroban_forge_test_utils::TestAccounts) {
    let env = soroban_forge_test_utils::new_env();
    let accounts = soroban_forge_test_utils::TestAccounts::generate(&env);

    (env, accounts)
}

#[test]
fn composition_test_infrastructure() {
    let (env, accounts) = setup_env();

    // Verify basic test infrastructure works
    assert!(accounts.all(&env).len() == 6);
    assert_ne(accounts.user1, accounts.user2);
}
