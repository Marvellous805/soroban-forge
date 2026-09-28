//! Randomized invariant suite (proptest) for marketplace royalties.
//!
//! Exercised invariants:
//! 1. Split conservation: `royalty_share + returned_net == amount` for every `amount > 0` and `bps <= 10_000`.
//! 2. Rate boundaries: `bps == 0` returns the full amount and `bps == 10_000` returns zero net.
//! 3. Bounded net: returned net is never negative and never exceeds the amount.
//! 4. Batch conservation: the aggregate of a random batch satisfies `sum(seller_net) + sum(royalty_share) == sum(amount)`.
//! 5. Cap boundary: a batch exceeding `MAX_SETTLE_SALES` is rejected before any transfer.
//! 6. Rollback integrity: a failed batch transfer leaves every balance and the summary untouched.

use crate::{MarketplaceRoyalties, SorobanForgeMarketplaceRoyaltiesClient, MAX_SETTLE_SALES};
use proptest::collection::vec as prop_vec;
use proptest::prelude::*;
use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{Address, Env};
use std::vec::Vec;

const MAX_AMOUNT: i128 = 1_000_000_000_000_000;

struct World {
    env: Env,
    token: Address,
    contract_id: Address,
    collection: Address,
    _recipient: Address,
    seller: Address,
    payer: Address,
}

fn setup_world(bps: u32, mint_amount: i128) -> World {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin);
    let token = sac.address();

    let collection = Address::generate(&env);
    let recipient = Address::generate(&env);
    let seller = Address::generate(&env);
    let payer = Address::generate(&env);

    StellarAssetClient::new(&env, &token).mint(&payer, &mint_amount);

    let contract_id = env.register(MarketplaceRoyalties, ());
    let client = SorobanForgeMarketplaceRoyaltiesClient::new(&env, &contract_id);
    client.set_royalty(&collection, &recipient, &bps);

    World {
        env,
        token,
        contract_id,
        collection,
        _recipient: recipient,
        seller,
        payer,
    }
}

impl World {
    fn client(&self) -> SorobanForgeMarketplaceRoyaltiesClient<'_> {
        SorobanForgeMarketplaceRoyaltiesClient::new(&self.env, &self.contract_id)
    }
}

struct BatchWorld {
    env: Env,
    token: Address,
    contract_id: Address,
    collection: Address,
    payer: Address,
    sellers: Vec<Address>,
    _recipient: Address,
    amounts: Vec<i128>,
}

fn setup_batch_world(bps: u32, amounts: Vec<i128>) -> BatchWorld {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin);
    let token = sac.address();

    let collection = Address::generate(&env);
    let recipient = Address::generate(&env);
    let payer = Address::generate(&env);

    let total: i128 = amounts.iter().sum();
    StellarAssetClient::new(&env, &token).mint(&payer, &total);

    let sellers: Vec<Address> = (0..amounts.len())
        .map(|_| Address::generate(&env))
        .collect();

    let contract_id = env.register(MarketplaceRoyalties, ());
    let client = SorobanForgeMarketplaceRoyaltiesClient::new(&env, &contract_id);
    client.set_royalty(&collection, &recipient, &bps);

    BatchWorld {
        env,
        token,
        contract_id,
        collection,
        payer,
        sellers,
        _recipient: recipient,
        amounts,
    }
}

impl BatchWorld {
    fn client(&self) -> SorobanForgeMarketplaceRoyaltiesClient<'_> {
        SorobanForgeMarketplaceRoyaltiesClient::new(&self.env, &self.contract_id)
    }

    fn batch(&self) -> soroban_sdk::Vec<(Address, i128)> {
        let mut batch = soroban_sdk::Vec::new(&self.env);
        for (seller, amount) in self.sellers.iter().zip(self.amounts.iter()) {
            batch.push_back((seller.clone(), *amount));
        }
        batch
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn prop_split_conservation_and_bounds(
        amount in 1i128..=MAX_AMOUNT,
        bps in 0u32..=10_000_u32,
    ) {
        let w = setup_world(bps, amount);
        let net = w
            .client()
            .distribute(&w.collection, &w.token, &w.payer, &w.seller, &amount);

        let expected_royalty = amount * (bps as i128) / 10_000;

        prop_assert_eq!(net + expected_royalty, amount, "split conservation: net + royalty must equal total amount");
        prop_assert!(net >= 0, "seller net must never be negative");
        prop_assert!(net <= amount, "seller net must never exceed gross amount");
        prop_assert!(expected_royalty >= 0, "royalty share must never be negative");
        prop_assert!(expected_royalty <= amount, "royalty share must never exceed gross amount");
    }

    #[test]
    fn prop_boundary_rates(
        amount in 1i128..=MAX_AMOUNT,
    ) {
        let w_zero = setup_world(0, amount);
        let net_zero = w_zero
            .client()
            .distribute(&w_zero.collection, &w_zero.token, &w_zero.payer, &w_zero.seller, &amount);
        prop_assert_eq!(net_zero, amount, "bps == 0 must return full amount");

        let w_full = setup_world(10_000, amount);
        let net_full = w_full
            .client()
            .distribute(&w_full.collection, &w_full.token, &w_full.payer, &w_full.seller, &amount);
        prop_assert_eq!(net_full, 0, "bps == 10_000 must return 0 net");
    }

    #[test]
    fn prop_atomic_settlement_conservation(
        amount in 1i128..=MAX_AMOUNT,
        bps in 0u32..=10_000_u32,
    ) {
        let w = setup_world(bps, amount);
        let settlement = w.client().settle_sale(&w.collection, &w.token, &w.payer, &w.seller, &amount);

        prop_assert_eq!(settlement.royalty_share + settlement.seller_net, amount, "settled shares must sum to amount");
        prop_assert!(settlement.seller_net >= 0);
        prop_assert!(settlement.royalty_share >= 0);
    }

    #[test]
    fn prop_batch_conservation(
        bps in 0u32..=10_000_u32,
        amounts in prop_vec(1i128..=MAX_AMOUNT, 1..=MAX_SETTLE_SALES as usize),
    ) {
        let w = setup_batch_world(bps, amounts.clone());
        let settled = w.client().settle_sales(&w.collection, &w.token, &w.payer, &w.batch());

        let mut sum_seller_net: i128 = 0;
        let mut sum_royalty_share: i128 = 0;
        let mut sum_amounts: i128 = 0;
        for (i, sale) in settled.iter().enumerate() {
            sum_seller_net += sale.seller_net;
            sum_royalty_share += sale.royalty_share;
            sum_amounts += amounts[i];
            prop_assert_eq!(sale.seller_net + sale.royalty_share, amounts[i], "per-sale conservation: seller_net + royalty_share must equal amount");
        }

        prop_assert_eq!(sum_seller_net + sum_royalty_share, sum_amounts, "batch conservation: sum(seller_net) + sum(royalty_share) must equal sum(amount)");

        let summary = w.client().get_settlement_summary(&w.collection);
        prop_assert_eq!(summary.sales, amounts.len() as u32, "summary sales count must match batch size");
        prop_assert_eq!(summary.gross_volume, sum_amounts, "summary gross_volume must equal total amount");
        prop_assert_eq!(summary.royalties_paid, sum_royalty_share, "summary royalties_paid must equal total royalty share");
    }

    #[test]
    fn prop_batch_cap_boundary(
        bps in 0u32..=10_000_u32,
    ) {
        let n = MAX_SETTLE_SALES as usize + 1;
        let amounts = (0..n).map(|_| 1i128).collect::<Vec<_>>();
        let w = setup_batch_world(bps, amounts);

        // try_settle_sales returns Result<Result<Vec<Settlement>, ConversionError>, Result<ForgeError, InvokeError>>.
        // A cap rejection surfaces as an outer Err containing the contract's Err(ForgeError::InvalidInput).
        let result = w.client().try_settle_sales(&w.collection, &w.token, &w.payer, &w.batch());
        let err = result.unwrap_err().unwrap();
        prop_assert_eq!(err, ForgeError::InvalidInput, "batch over cap must return InvalidInput");

        let tc = TokenClient::new(&w.env, &w.token);
        prop_assert_eq!(tc.balance(&w.payer), n as i128, "payer balance must be untouched on cap rejection");

        let summary_result = w.client().try_get_settlement_summary(&w.collection);
        let summary_err = summary_result.unwrap_err().unwrap();
        prop_assert_eq!(summary_err, ForgeError::NotFound, "summary must not exist after cap rejection");
    }

    #[test]
    fn prop_batch_rollback_integrity(
        bps in 0u32..=10_000_u32,
    ) {
        // Build a world where the payer has enough for one sale but not the batch.
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(admin);
        let token = sac.address();
        let collection = Address::generate(&env);
        let recipient = Address::generate(&env);
        let payer = Address::generate(&env);
        let seller_a = Address::generate(&env);
        let seller_b = Address::generate(&env);

        // Mint only one sale's worth — the batch has two sales so the second transfer fails.
        StellarAssetClient::new(&env, &token).mint(&payer, &1_000_000_000_000_000_i128);

        let contract_id = env.register(MarketplaceRoyalties, ());
        let client = SorobanForgeMarketplaceRoyaltiesClient::new(&env, &contract_id);
        client.set_royalty(&collection, &recipient, &bps);

        let mut batch = soroban_sdk::Vec::new(&env);
        batch.push_back((seller_a.clone(), 1_000_000_000_000_000_i128));
        batch.push_back((seller_b.clone(), 1_000_000_000_000_000_i128));

        let payer_balance_before = StellarAssetClient::new(&env, &token).balance(&payer);
        let recipient_balance_before = StellarAssetClient::new(&env, &token).balance(&recipient);
        let seller_a_balance_before = StellarAssetClient::new(&env, &token).balance(&seller_a);
        let seller_b_balance_before = StellarAssetClient::new(&env, &token).balance(&seller_b);
        let contract_balance_before = StellarAssetClient::new(&env, &token).balance(&contract_id);

        let result = client.try_settle_sales(&collection, &token, &payer, &batch);
        let err = result.unwrap_err().unwrap();
        prop_assert_eq!(err, ForgeError::TokenTransferFailed, "a batch with insufficient balance must fail on transfer");

        // Verify every balance and the summary are untouched by frame rollback.
        prop_assert_eq!(StellarAssetClient::new(&env, &token).balance(&payer), payer_balance_before, "payer balance must be restored on rollback");
        prop_assert_eq!(StellarAssetClient::new(&env, &token).balance(&recipient), recipient_balance_before, "recipient balance must be restored on rollback");
        prop_assert_eq!(StellarAssetClient::new(&env, &token).balance(&seller_a), seller_a_balance_before, "seller_a balance must be restored on rollback");
        prop_assert_eq!(StellarAssetClient::new(&env, &token).balance(&seller_b), seller_b_balance_before, "seller_b balance must be restored on rollback");
        prop_assert_eq!(StellarAssetClient::new(&env, &token).balance(&contract_id), contract_balance_before, "contract balance must be restored on rollback");

        let summary_after = client.try_get_settlement_summary(&collection);
        let summary_after_err = summary_after.unwrap_err().unwrap();
        prop_assert_eq!(summary_after_err, ForgeError::NotFound, "summary must be untouched on rollback");
    }
}
