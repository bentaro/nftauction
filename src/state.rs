use cosmwasm_std::{CanonicalAddr, Storage, Uint128};
use cosmwasm_storage::{
    bucket, bucket_read, singleton, singleton_read, Bucket, ReadonlyBucket, ReadonlySingleton,
    Singleton,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

static CONFIG_KEY: &[u8] = b"config";
static LISTING_KEY: &[u8] = b"listing";
static BANK_KEY: &[u8] = b"bank";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub denom: String,
    pub owner: CanonicalAddr,
    pub listing_count: u64,
    pub staked_tokens: Uint128,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct TokenManager {
    pub token_balance: Uint128,             // total staked balance
    pub locked_tokens: Vec<(u64, Uint128)>, //maps listing_id to weight voted
    pub participated_bids: Vec<u64>,       // listing_id
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Bidder {
    pub bidder: CanonicalAddr,
    pub price: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum BidStatus {
    InProgress,
    Tally,
    Passed,
    Rejected,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Listing {
    pub token_id: String,
    pub denom: String,
    pub creator: CanonicalAddr,
    pub status: BidStatus,
    pub highest_bid: Uint128,
    pub highest_bidder: CanonicalAddr,
    pub minimum_bid : Uint128,
    pub bidders : Vec<CanonicalAddr>,
    pub bidders_info : Vec<Bidder>,
    pub start_height: Option<u64>,
    pub end_height: u64,
    pub description: String,
}

pub fn config<S: Storage>(storage: &mut S) -> Singleton<S, State> {
    singleton(storage, CONFIG_KEY)
}

pub fn config_read<S: Storage>(storage: &S) -> ReadonlySingleton<S, State> {
    singleton_read(storage, CONFIG_KEY)
}

pub fn listing<S: Storage>(storage: &mut S) -> Bucket<S, Listing> {
    bucket(storage, LISTING_KEY)
}

pub fn listing_read<S: Storage>(storage: &S) -> ReadonlyBucket<S, Listing> {
    bucket_read(storage, LISTING_KEY)
}

pub fn bank<S: Storage>(storage: &mut S) -> Bucket<S, TokenManager> {
    bucket(storage, BANK_KEY)
}

pub fn bank_read<S: Storage>(storage: &S) -> ReadonlyBucket<S, TokenManager> {
    bucket_read(storage, BANK_KEY)
}
