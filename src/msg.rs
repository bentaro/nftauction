use crate::state::ListingStatus;
use cosmwasm_std::{HumanAddr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub denom: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    Bid {
        listing_id: u64,
        price: Uint128,
    },
    WithdrawTokens {
        amount: Option<Uint128>,
    },
    List {
        token_id: String,
        denom: String,
        minimum_bid : Uint128,
        start_height: Option<u64>,
        end_height: Option<u64>,
        description: String,
    },
    CloseList {
        listing_id: u64,
    },
    // GetNft {
    //     denom: String,
    //     id: String,
    // },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    TokenStake { address: HumanAddr },
    Listing { listing_id: u64 },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema)]
pub struct ListingResponse {
    pub creator: HumanAddr,
    pub status: ListingStatus,
    pub quorum_percentage: Option<u8>,
    pub end_height: Option<u64>,
    pub start_height: Option<u64>,
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema)]
pub struct CreateListingResponse {
    pub listing_id: u64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema)]
pub struct ListingCountResponse {
    pub listing_count: u64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema)]
pub struct TokenStakeResponse {
    pub token_balance: Uint128,
}
