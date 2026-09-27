use soroban_sdk::{contract, contractimpl, Env};

use crate::errors::ForgeError;

#[contract]
pub struct Contract;

#[contractimpl]
impl Contract {
    pub fn example_method(_env: Env, input: Option<i128>) -> Result<i128, ForgeError> {
        Ok(input.unwrap_or(0))
    }
}
