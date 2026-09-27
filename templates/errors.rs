use soroban_sdk::contracterror;

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum ForgeError {
    InvalidInput = 1,
    NotFound = 2,
    Unauthorized = 3,
}
