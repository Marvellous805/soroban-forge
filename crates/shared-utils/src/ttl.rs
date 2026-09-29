//! # Persistent-storage TTL helpers
//!
//! Soroban persistent entries expire unless their time-to-live (TTL) is
//! extended, and an expired entry is silently absent on the next read — a
//! liveness trap for any contract that stores long-lived records durably
//! (see the expired-entry failure mode discussed in issue #93). Every
//! settlement contract in this workspace bumps the persistent entry it
//! writes on every state-changing touch, so a single periodic keeper call
//! keeps the records alive.
//!
//! This module is the single source of that policy:
//!
//! - [`DAY_IN_LEDGERS`] — one ledger closes roughly every 5 seconds, so
//!   17,280 ledgers ≈ 1 day;
//! - [`BUMP_AMOUNT`] — the lifetime written on every bump (30 days),
//!   comfortably covering a record between keeper touches;
//! - [`BUMP_THRESHOLD`] — how close to expiry an entry must be before the
//!   bump applies (one day of margin).
//!
//! The threshold/extend pattern is a cheap no-op while the entry is fresh
//! and decisive near expiry: extending a fresh entry would only add rent.
//!
//! [`bump_entry`] is generic over the storage key so a contract can pass
//! its crate-local `DataKey` variants (or any other key) without this
//! crate depending on any contract — the dependency direction stays
//! contract crates → `shared-utils`.
//!
//! The constants match, byte for byte, the copies this helper replaces in
//! `escrow`, `multi-sig-wallet`, `marketplace-royalties`, and
//! `dao-governance`; migrating a contract to it is a pure refactor.

use soroban_sdk::{Env, IntoVal, Val};

/// Number of ledgers in a day (approximately, based on 5-second ledgers).
pub const DAY_IN_LEDGERS: u32 = 17_280;
/// Lifetime applied on every TTL touch: 30 days of ledgers.
pub const BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
/// Bump only when the entry is within this window (one day) of expiring.
pub const BUMP_THRESHOLD: u32 = BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Bump a persistent entry's TTL to the [`BUMP_AMOUNT`] horizon when it
/// falls inside [`BUMP_THRESHOLD`].
///
/// Generic over the key so every contract's `DataKey` variants work
/// unchanged (`soroban_sdk`'s `extend_ttl` takes any `IntoVal<Env, Val>`
/// key). Call this after every write to a persistent entry — the same
/// threshold/extend pattern the contract crates have always used.
pub fn bump_entry<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val>,
{
    env.storage()
        .persistent()
        .extend_ttl(key, BUMP_THRESHOLD, BUMP_AMOUNT);
}

#[cfg(test)]
mod tests {
    use super::{bump_entry, BUMP_AMOUNT, BUMP_THRESHOLD, DAY_IN_LEDGERS};
    use soroban_sdk::testutils::storage::Persistent as _;
    use soroban_sdk::testutils::Ledger as _;
    use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

    /// A contract that owns a persistent entry, so the helper is exercised
    /// exactly as a caller uses it: inside the owning contract's frame
    /// (`env.as_contract`), against a `#[contracttype]` key.
    #[contract]
    struct BumpHost;

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct HostKey(u32);

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Record {
        value: u64,
    }

    #[contractimpl]
    impl BumpHost {
        /// Write a persistent record and bump it with the shared helper.
        fn put(env: Env, id: u32, value: u64) {
            let key = HostKey(id);
            env.storage().persistent().set(&key, &Record { value });
            bump_entry(&env, &key);
        }

        /// Bump without writing, like a keeper `touch_ttl` entrypoint.
        fn touch(env: Env, id: u32) {
            let key = HostKey(id);
            assert!(env.storage().persistent().has(&key));
            bump_entry(&env, &key);
        }

        fn get(env: Env, id: u32) -> Option<Record> {
            env.storage().persistent().get(&HostKey(id))
        }
    }

    fn persistent_ttl(env: &Env, contract: &Address, key: &HostKey) -> u32 {
        env.as_contract(contract, || env.storage().persistent().get_ttl(key))
    }

    /// The helper extends a persistent entry to the 30-day horizon, and the
    /// stored value survives the bump untouched.
    #[test]
    fn bump_extends_persistent_entry_to_the_policy_horizon() {
        let env = Env::default();
        let contract = env.register(BumpHost, ());

        env.as_contract(&contract, || BumpHost::put(env.clone(), 1, 42));
        let key = HostKey(1);
        // The write-time bump sets the full 30-day horizon.
        assert_eq!(persistent_ttl(&env, &contract, &key), BUMP_AMOUNT);

        // Cross the threshold (one day of ledgers): the entry is now inside
        // the bump window.
        env.ledger()
            .with_mut(|l| l.sequence_number += BUMP_THRESHOLD + 1);
        let before = persistent_ttl(&env, &contract, &key);
        assert!(before < BUMP_THRESHOLD);

        env.as_contract(&contract, || BumpHost::touch(env.clone(), 1));

        // The bump renews the entry to the full horizon again.
        assert_eq!(persistent_ttl(&env, &contract, &key), BUMP_AMOUNT);
        // The bump must not alter the entry's value.
        let record: Option<Record> = env.as_contract(&contract, || BumpHost::get(env.clone(), 1));
        assert_eq!(record, Some(Record { value: 42 }));
    }

    /// Below the threshold the bump is a no-op: extending a fresh entry
    /// would only add rent, so nothing is written.
    #[test]
    fn bump_below_threshold_is_a_no_op() {
        let env = Env::default();
        let contract = env.register(BumpHost, ());

        env.as_contract(&contract, || BumpHost::put(env.clone(), 7, 9));
        let key = HostKey(7);
        assert_eq!(persistent_ttl(&env, &contract, &key), BUMP_AMOUNT);

        // Advance well short of the threshold: the entry stays fresh.
        env.ledger().with_mut(|l| l.sequence_number += 1_000);
        let before = persistent_ttl(&env, &contract, &key);
        assert!(before > BUMP_THRESHOLD);

        env.as_contract(&contract, || BumpHost::touch(env.clone(), 7));

        // An extend would snap the TTL back to the full horizon; the no-op
        // leaves the entry exactly where it was.
        assert_eq!(persistent_ttl(&env, &contract, &key), before);
    }

    /// Each key is bumped independently: a second entry stays untouched
    /// when only the first is bumped.
    #[test]
    fn bump_is_per_key() {
        let env = Env::default();
        let contract = env.register(BumpHost, ());

        env.as_contract(&contract, || {
            BumpHost::put(env.clone(), 1, 11);
            BumpHost::put(env.clone(), 2, 22);
        });
        let key1 = HostKey(1);
        let key2 = HostKey(2);

        env.ledger()
            .with_mut(|l| l.sequence_number += BUMP_THRESHOLD + 1);
        env.as_contract(&contract, || BumpHost::touch(env.clone(), 1));

        // The bumped entry is renewed; the untouched one merely decayed.
        assert_eq!(persistent_ttl(&env, &contract, &key1), BUMP_AMOUNT);
        assert_eq!(
            persistent_ttl(&env, &contract, &key2),
            BUMP_AMOUNT - (BUMP_THRESHOLD + 1)
        );
        // And the untouched entry's value is intact.
        let record: Option<Record> = env.as_contract(&contract, || BumpHost::get(env.clone(), 2));
        assert_eq!(record, Some(Record { value: 22 }));
    }

    /// The policy values match the copies this helper consolidates: 30-day
    /// horizon, one-day threshold margin.
    #[test]
    fn policy_values_are_the_consolidated_ones() {
        assert_eq!(DAY_IN_LEDGERS, 17_280);
        assert_eq!(BUMP_AMOUNT, 518_400);
        assert_eq!(BUMP_THRESHOLD, 501_120);
    }
}
