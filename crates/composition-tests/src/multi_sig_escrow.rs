use super::*;

/// Test scenario: Multi-sig wallet funds an escrow contract.
///
/// This verifies cross-contract composition between multi-sig and escrow.
///
/// Flow:
/// 1. Multi-sig wallet is initialized with owners and threshold
/// 2. Wallet receives token deposits
/// 3. Owners submit and approve a withdrawal to fund an escrow
/// 4. Escrow is initialized and receives the funds
/// 5. Verify balances and state consistency
#[test]
fn multi_sig_escrow_composition() {
    let (env, accounts) = setup_env();

    // 1. Deploy and initialize the multi-sig wallet.
    let multi_sig_id = env.register(MultiSigContract, ());
    let multi_sig = MultiSigClient::new(&env, &multi_sig_id);
    let owners = vec![&env, accounts[0].clone(), accounts[1].clone(), accounts[2].clone()];
    multi_sig.initialize(&owners, &2);

    // 2. Fund the multi-sig wallet with tokens.
    let token_id = deploy_token(&env, &accounts[0]);
    let token = TokenClient::new(&env, &token_id);
    token.mint(&multi_sig_id, &10_000_000);
    assert_eq!(token.balance(&multi_sig_id), 10_000_000);

    // 3. Deploy and initialize the escrow contract.
    let escrow_id = env.register(EscrowContract, ());
    let escrow = EscrowClient::new(&env, &escrow_id);
    escrow.initialize(
        &multi_sig_id,
        &accounts[3],
        &token_id,
        &5_000_000,
        &None,
    );

    // 4. Owners submit and approve a withdrawal targeting the escrow.
    let tx_id = multi_sig.submit_transaction(
        &accounts[0],
        &escrow_id,
        &5_000_000,
        &token_id,
    );
    multi_sig.approve_transaction(&accounts[0], &tx_id);
    multi_sig.approve_transaction(&accounts[1], &tx_id);

    // 5. Execute withdrawal to fund escrow.
    multi_sig.execute_transaction(&accounts[0], &tx_id);

    // 6. Verify balances and state consistency.
    assert_eq!(token.balance(&escrow_id), 5_000_000);
    assert_eq!(token.balance(&multi_sig_id), 5_000_000);
    assert_eq!(escrow.get_amount(), 5_000_000);
    assert_eq!(escrow.get_status(), EscrowStatus::Funded);
}
