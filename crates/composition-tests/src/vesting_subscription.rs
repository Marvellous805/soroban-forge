use super::*;

/// Test scenario: Vesting claim funds subscription payment.
///
/// Flow:
/// 1. Vesting schedule is set up with beneficiary
/// 2. Beneficiary claims vested tokens
/// 3. Beneficiary uses claimed tokens for subscription payment
/// 4. Subscription state advances correctly
/// 5. Verify token flow and subscription state
#[test]
fn vesting_feeds_subscription() {
    let (env, accounts) = setup_env();

    let harness = CrossContractHarness::new(env.clone());
    harness.setup_vesting_subscription();

    // Beneficiary claims vested tokens.
    let beneficiary = accounts.beneficiary.clone();
    let claimed = harness.vesting.claim(&beneficiary);
    assert!(claimed > 0, "vesting claim must yield tokens");

    // Beneficiary pays for a subscription using claimed tokens.
    let subscription_id = harness.subscription.subscribe(&beneficiary, &claimed);
    assert!(
        harness.subscription.is_active(&subscription_id),
        "subscription must be active after payment"
    );

    // Conservation: total tokens across both contracts remain constant.
    harness.assert_token_conservation();
}
