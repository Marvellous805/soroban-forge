#`!no_std]

/// Shared testing utilities for Soroban Forge contracts.
///
/// Provides lightweight helpers used by contract integration tests:
/// - [`new_env`] to construct a configured [`Env`]
/// - [`TestAccounts`] to obtain deterministic, distinct mock addresses
/// - [`CrossContractHarness`] to exercise cross-contract composition
///   patterns between multiple contracts
///
/// This crate is intended to be used as a `dev-dependency` by contract crates.

use soroban_sdk::Env;

pub mod mocks;
pub mod composition;

pub use mocks::TokenFixture;
pub use mocks::{
    MockTarget, MockTargetClient, RevertingTarget, RevertingTargetClient, TestAccounts,
};
pub use composition::{
    CrossContractHarness, CrossContractSnapshot, CompositionError,
};

/// Create a [`Env`] configured for contract testing.
///
/// Enables [`Env::mock_all_auths`], which lets tests invoke authorised
/// contract functions without manually signing every call. Tests that need to
/// assert authorisation failures should call [`Env::set_auths`] / disable
/// mocking explicitly.
pub fn new_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}
