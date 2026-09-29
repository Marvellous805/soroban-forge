//! SEP-41 token transfer helpers shared across all settlement crates.
//!
//! # Transfer-before-state contract
//!
//! Every helper in this module follows the **transfer-before-state** ordering
//! rule used throughout Soroban Forge: the token transfer is issued *before*
//! any persistent state write in the calling entrypoint.  That way, a failed
//! transfer rolls the whole invocation back via the host's automatic revert,
//! and no state is partially committed.
//!
//! # Error bucketing
//!
//! All token failures — insufficient balance, missing or deauthorized
//! trustline, custom token logic, undeployed token contract, host abort — are
//! collapsed into a single [`ForgeError::TokenTransferFailed`].  The raw error
//! discriminant from the token contract is intentionally discarded: a client
//! receiving `Error(Contract, #N)` cannot tell whether `N` originated in the
//! token or in the calling contract, so forwarding it would be misleading.
//! The underlying reason remains visible in the transaction's **diagnostic
//! events**, which record the full call tree.
//!
//! # Helpers
//!
//! | Helper | Direction |
//! |--------|-----------|
//! | [`transfer_to_contract`] | external address → this contract |
//! | [`transfer_from_contract`] | this contract → external address |
//! | [`transfer_tokens`] | arbitrary `from` → arbitrary `to` (neither end is necessarily this contract) |
//!
//! `transfer_to_contract` and `transfer_from_contract` use
//! `env.current_contract_address()` as one endpoint, which makes them a
//! natural fit for deposit and withdrawal flows.  `transfer_tokens` is the
//! general form used by marketplace royalties, where both the payer and the
//! recipient are external accounts.

use soroban_sdk::{token, Address, Env};

use crate::errors::ForgeError;

/// Move `amount` of `token` from `from` to this contract.
///
/// Calls [`token::TokenClient::try_transfer`] with the current contract as the
/// recipient.  The caller's authorisation on the outer entrypoint covers the
/// nested token invocation — no prior allowance is needed when the holder
/// directly authorises the call.
///
/// # Errors
///
/// Returns [`ForgeError::TokenTransferFailed`] for any token-side failure
/// (insufficient balance, missing trustline, custom token rejection, host
/// abort).  See the [module-level bucketing note](self) for why the raw
/// discriminant is discarded.
///
/// # Ordering
///
/// Always call this function **before** any persistent state write in the
/// enclosing entrypoint — see the [module-level transfer-before-state
/// note](self).
pub fn transfer_to_contract(
    env: &Env,
    token: &Address,
    from: &Address,
    amount: i128,
) -> Result<(), ForgeError> {
    match token::TokenClient::new(env, token).try_transfer(
        from,
        env.current_contract_address(),
        &amount,
    ) {
        Ok(Ok(())) => Ok(()),
        // Token returned a typed error (insufficient balance, missing
        // trustline, custom token logic) or the host aborted (most
        // commonly an undeployed token address).  The raw discriminant is
        // intentionally discarded — see the module-level bucketing note.
        _ => Err(ForgeError::TokenTransferFailed),
    }
}

/// Move `amount` of `token` from this contract to `to`.
///
/// Calls [`token::TokenClient::new`] with this contract as the sender.
///
/// # Errors
///
/// Returns [`ForgeError::TokenTransferFailed`] for any token-side failure.
/// See the [module-level bucketing note](self).
///
/// # Ordering
///
/// Always call this function **before** any persistent state write in the
/// enclosing entrypoint — see the [module-level transfer-before-state
/// note](self).
pub fn transfer_from_contract(
    env: &Env,
    token: &Address,
    to: &Address,
    amount: i128,
) -> Result<(), ForgeError> {
    match token::TokenClient::new(env, token).try_transfer(
        &env.current_contract_address(),
        to,
        &amount,
    ) {
        Ok(Ok(())) => Ok(()),
        _ => Err(ForgeError::TokenTransferFailed),
    }
}

/// Move `amount` of `token` from `from` to `to`.
///
/// Neither endpoint is required to be this contract.  This is the general
/// form used by marketplace royalties, where the payer and each recipient
/// (seller and royalty recipient) are all external accounts.
///
/// The payer's authorisation on the calling entrypoint covers the nested
/// token invocation — no prior allowance is needed when the holder directly
/// authorises the call.
///
/// # Errors
///
/// Returns [`ForgeError::TokenTransferFailed`] for any token-side failure.
/// See the [module-level bucketing note](self).
///
/// # Ordering
///
/// Always call this function **before** any persistent state write in the
/// enclosing entrypoint — see the [module-level transfer-before-state
/// note](self).
pub fn transfer_tokens(
    env: &Env,
    token: &Address,
    from: &Address,
    to: &Address,
    amount: i128,
) -> Result<(), ForgeError> {
    match token::TokenClient::new(env, token).try_transfer(from, to, &amount) {
        Ok(Ok(())) => Ok(()),
        // Token returned a typed error (insufficient balance, missing
        // trustline, custom token logic) or the host aborted (most commonly
        // an undeployed token address).
        _ => Err(ForgeError::TokenTransferFailed),
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::token::StellarAssetClient;
    use soroban_sdk::{contract, contractimpl, Address, Env};

    use super::*;

    // ---------------------------------------------------------------------------
    // Minimal test contracts
    //
    // transfer_to_contract and transfer_from_contract use
    // env.current_contract_address() as one endpoint, so they must be called
    // from inside a real contract invocation.  We define two tiny contracts
    // that proxy each helper and return its Result so tests can assert on it.
    // ---------------------------------------------------------------------------

    /// Proxy for transfer_to_contract: pulls `amount` of `token` from `from`
    /// into the contract itself.
    #[contract]
    pub struct DepositProxy;

    #[contractimpl]
    impl DepositProxy {
        pub fn run(
            env: Env,
            token: Address,
            from: Address,
            amount: i128,
        ) -> Result<(), ForgeError> {
            // Require the sender's auth at the entrypoint, exactly as escrow's
            // `deposit` does, so mock_all_auths covers the nested token transfer.
            from.require_auth();
            transfer_to_contract(&env, &token, &from, amount)
        }
    }

    /// Proxy for transfer_from_contract: pushes `amount` of `token` from the
    /// contract itself to `to`.
    #[contract]
    pub struct WithdrawProxy;

    #[contractimpl]
    impl WithdrawProxy {
        pub fn run(env: Env, token: Address, to: Address, amount: i128) -> Result<(), ForgeError> {
            transfer_from_contract(&env, &token, &to, amount)
        }
    }

    // ---------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------

    /// Create a minimal test environment with all auths mocked and a SAC token.
    fn setup() -> (Env, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(admin.clone());
        let token = sac.address();
        (env, token)
    }

    // ---------------------------------------------------------------------------
    // transfer_to_contract
    // ---------------------------------------------------------------------------

    #[test]
    fn transfer_to_contract_moves_tokens() {
        let (env, token) = setup();
        let contract_id = env.register(DepositProxy, ());
        let client = DepositProxyClient::new(&env, &contract_id);
        let sender = Address::generate(&env);
        let token_admin = StellarAssetClient::new(&env, &token);
        token_admin.mint(&sender, &500);

        // run() panics if the contract returns Err, so reaching the balance
        // assertions is proof that transfer_to_contract returned Ok(()).
        client.run(&token, &sender, &500);

        let token_client = soroban_sdk::token::Client::new(&env, &token);
        assert_eq!(token_client.balance(&contract_id), 500);
        assert_eq!(token_client.balance(&sender), 0);
    }

    #[test]
    fn transfer_to_contract_fails_on_insufficient_balance() {
        let (env, token) = setup();
        let contract_id = env.register(DepositProxy, ());
        let client = DepositProxyClient::new(&env, &contract_id);
        let sender = Address::generate(&env);
        // sender has no balance — the contract returns TokenTransferFailed

        let result = client.try_run(&token, &sender, &100);

        assert!(result.is_err() || result.unwrap().is_err());
    }

    // ---------------------------------------------------------------------------
    // transfer_from_contract
    // ---------------------------------------------------------------------------

    #[test]
    fn transfer_from_contract_moves_tokens() {
        let (env, token) = setup();
        let contract_id = env.register(WithdrawProxy, ());
        let client = WithdrawProxyClient::new(&env, &contract_id);
        let recipient = Address::generate(&env);
        let token_admin = StellarAssetClient::new(&env, &token);
        // Fund the contract address directly.
        token_admin.mint(&contract_id, &300);

        // run() panics if the contract returns Err.
        client.run(&token, &recipient, &300);

        let token_client = soroban_sdk::token::Client::new(&env, &token);
        assert_eq!(token_client.balance(&contract_id), 0);
        assert_eq!(token_client.balance(&recipient), 300);
    }

    #[test]
    fn transfer_from_contract_fails_on_insufficient_balance() {
        let (env, token) = setup();
        let contract_id = env.register(WithdrawProxy, ());
        let client = WithdrawProxyClient::new(&env, &contract_id);
        let recipient = Address::generate(&env);
        // contract has no balance — the contract returns TokenTransferFailed

        let result = client.try_run(&token, &recipient, &50);

        assert!(result.is_err() || result.unwrap().is_err());
    }

    // ---------------------------------------------------------------------------
    // transfer_tokens
    // ---------------------------------------------------------------------------

    #[test]
    fn transfer_tokens_moves_tokens_between_two_external_addresses() {
        let (env, token) = setup();
        let from = Address::generate(&env);
        let to = Address::generate(&env);
        let token_admin = StellarAssetClient::new(&env, &token);
        token_admin.mint(&from, &1000);

        let result = transfer_tokens(&env, &token, &from, &to, 1000);

        assert_eq!(result, Ok(()));

        let token_client = soroban_sdk::token::Client::new(&env, &token);
        assert_eq!(token_client.balance(&from), 0);
        assert_eq!(token_client.balance(&to), 1000);
    }

    #[test]
    fn transfer_tokens_fails_on_insufficient_balance() {
        let (env, token) = setup();
        let from = Address::generate(&env);
        let to = Address::generate(&env);
        // `from` has no balance

        let result = transfer_tokens(&env, &token, &from, &to, 1);

        assert_eq!(result, Err(ForgeError::TokenTransferFailed));
    }

    #[test]
    fn transfer_tokens_partial_amount_leaves_correct_remainder() {
        let (env, token) = setup();
        let from = Address::generate(&env);
        let to = Address::generate(&env);
        let token_admin = StellarAssetClient::new(&env, &token);
        token_admin.mint(&from, &400);

        let result = transfer_tokens(&env, &token, &from, &to, 150);

        assert_eq!(result, Ok(()));

        let token_client = soroban_sdk::token::Client::new(&env, &token);
        assert_eq!(token_client.balance(&from), 250);
        assert_eq!(token_client.balance(&to), 150);
    }
}
