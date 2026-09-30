use super:*;

/// Test scenario: DAO governance settles marketplace royalties.
///
/// Flow:
/// 1. DAO proposal is created to settle royalties
/// 2. Proposal receives required votes
/// 3. DAO executes cross-contract call to royalties contract
/// 4. Royalties are distributed to recipients
#[test]
fn dao_settles_royalties() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let governor = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Deploy the royalties contract and initialize with a royalty recipient.
    let royalties_id = env.registerXContract(&marketplace_royalties::WASM);
    let royalties = marketplace_royalties::Client::new(&env, &royalties_id);
    royalties.initialize(&admin, &vec![&env, (recipient.clone(), 5000u32)]);

    // Deploy the DAO contract and create a proposal that targets the royalties contract.
    let dao_id = env.registerXContract(&dao::WASM);
    let dao = dao::Client::new(&env, &dao_id);
    dao.initialize(&governor, &vec![&env, (governor.clone(), 1000u32)]);

    // Proposal that distributes royalties to the recipient.
    let amount = 1,000u32;
    let proposal_id = dao.propose(
        &governor,
        &royalties_id,
        &symbol_short!(&env, "distribute"),
        &vec![&env, (recipient.clone(), amount)],
    );

    // Vote and execute.
    dao.vote(&governor, &proposal_id, &true);
    dao.execute(&governor, &proposal_id);

    // Verify the royalty distribution happened.
    assert_eq!(royalties.get_royalty(&recipient), 5000u32);
    assert_eq!(royalties.get_distributed(&recipient), amount);
}
