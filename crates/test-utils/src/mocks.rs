use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{contracttype, Address, Env, Vec};

/// A fixed set of distinct mock accounts for use in tests.
///
/// Addresses are generated from the provided [`Env`] and are stable for the
/// lifetime of that environment, giving tests readable, named participants
/// (deployer, users, validator, arbiter) without hard-coding addresses.
#[contracttype]
pub struct TestAccounts {
    /// Account that deploys contracts and funds operations.
    pub deployer: Address,
    /// First regular user.
    pub user1: Address,
    /// Second regular user.
    pub user2: Address,
    /// Third regular user.
    pub user3: Address,
    /// A validator / signer role.
    pub validator: Address,
    /// A neutral arbiter role.
    pub arbiter: Address,
}

impl TestAccounts {
    /// Generate a fresh set of distinct mock addresses from `env`.
    pub fn generate(env: &Env) -> Self {
        TestAccounts {
            deployer: Address::generate(env),
            user1: Address::generate(env),
            user2: Address::generate(env),
            user3: Address::generate(env),
            validator: Address::generate(env),
            arbiter: Address::generate(env),
        }
    }

    /// All six accounts as a [`Vec`], useful for multi-party setup.
    pub fn all(&self, env: &Env) -> Vec<Address> {
        let mut v = Vec::new(env);
        v.push_back(self.deployer.clone());
        v.push_back(self.user1.clone());
        v.push_back(self.user2.clone());
        v.push_back(self.user3.clone());
        v.push_back(self.validator.clone());
        v.push_back(self.arbiter.clone());
        v
    }
}

/// A shared Stellar Asset Contract (SAC) fixture for cross-contract settlement tests.
///
/// Wraps a registered SAC constructed from an [`Env`] with a generated admin.
///
/// # Example
///
/// ```
/// use soroban_sdk::Env;
/// use soroban_forge_test_utils::mocks::{TestAccounts, TokenFixture};
///
/// let env = Env::default();
/// env.mock_all_auths();
/// let accounts = TestAccounts::generate(&env);
///
/// let token = TokenFixture::new(&env);
/// token.mint(&accounts.user1, &1_000);
/// assert_eq!(token.balance(&accounts.user1), 1_000);
/// ```
pub struct TokenFixture<'a> {
    /// The registered token address.
    pub address: Address,
    /// The admin client for minting tokens.
    pub admin: StellarAssetClient<'a>,
    /// The standard token client for transfers and balances.
    pub client: TokenClient<'a>,
}

impl<'a> TokenFixture<'a> {
    /// Construct a fresh SAC fixture from an `env` with a generated admin.
    pub fn new(env: &'a Env) -> Self {
        let admin_addr = Address::generate(env);
        let sac = env.register_stellar_asset_contract_v2(admin_addr);
        let address = sac.address();
        let admin = StellarAssetClient::new(env, &address);
        let client = TokenClient::new(env, &address);
        
        Self {
            address,
            admin,
            client,
        }
    }

    /// Mint `amount` of the token to `to`.
    pub fn mint(&self, to: &Address, amount: &i128) {
        self.admin.mint(to, amount);
    }

    /// Read the token balance of `account`.
    pub fn balance(&self, account: &Address) -> i128 {
        self.client.balance(account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_fixture_starts_at_zero_and_mints() {
        let env = Env::default();
        env.mock_all_auths();
        let accounts = TestAccounts::generate(&env);
        let token = TokenFixture::new(&env);
        
        assert_eq!(token.balance(&accounts.user1), 0);
        token.mint(&accounts.user1, &1000);
        assert_eq!(token.balance(&accounts.user1), 1000);
    }

    #[test]
    fn token_fixtures_are_independent() {
        let env = Env::default();
        env.mock_all_auths();
        let accounts = TestAccounts::generate(&env);
        let token1 = TokenFixture::new(&env);
        let token2 = TokenFixture::new(&env);
        
        token1.mint(&accounts.user1, &500);
        assert_eq!(token1.balance(&accounts.user1), 500);
        assert_eq!(token2.balance(&accounts.user1), 0);
/// Mock target contract for testing cross-contract invocations.
///
/// Records the execution count and last dispatched payload in instance storage.
#[soroban_sdk::contract]
pub struct MockTarget;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MockTargetKey {
    Count,
    LastPayload,
}

#[soroban_sdk::contractimpl]
impl MockTarget {
    /// Dispatched by contracts executing actions via cross-contract calls.
    pub fn execute(env: Env, payload: soroban_sdk::Bytes) {
        let count: u32 = env
            .storage()
            .instance()
            .get(&MockTargetKey::Count)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&MockTargetKey::Count, &(count + 1));
        env.storage()
            .instance()
            .set(&MockTargetKey::LastPayload, &payload);
    }

    /// Read the number of times `execute` was called.
    pub fn count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&MockTargetKey::Count)
            .unwrap_or(0)
    }

    /// Read the last payload dispatched to `execute`.
    pub fn last_payload(env: Env) -> Option<soroban_sdk::Bytes> {
        env.storage().instance().get(&MockTargetKey::LastPayload)
    }
}

/// A mock target whose `execute` panics, simulating a target revert.
#[soroban_sdk::contract]
pub struct RevertingTarget;

#[soroban_sdk::contractimpl]
impl RevertingTarget {
    /// Panics on invocation, simulating a target revert.
    pub fn execute(_env: Env, _payload: soroban_sdk::Bytes) {
        panic!("target reverted");
    }
}
