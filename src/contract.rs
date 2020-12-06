use crate::coin_helpers::assert_sent_sufficient_coin;

use crate::msg::{
    CreateListingResponse, HandleMsg, InitMsg, ListingResponse, QueryMsg, TokenStakeResponse,
};
use crate::state::{
    bank, bank_read, config, config_read, listing, listing_read, Listing, BidStatus, State, Bidder,
};
use cosmwasm_std::{
    coin, to_binary, Api, Attribute, BankMsg, Binary, CanonicalAddr, Coin, CosmosMsg, Env, Extern,
    HandleResponse, HandleResult, HumanAddr, InitResponse, InitResult, Querier, StdError,
    StdResult, Storage, Uint128, MessageInfo, NftMsg,
};


// pub const VOTING_TOKEN: &str = "voting_token";
pub const DEFAULT_END_HEIGHT_BLOCKS: &u64 = &100_800_u64;
const MIN_STAKE_AMOUNT: u128 = 1;
const MIN_DESC_LENGTH: usize = 3;
const MAX_DESC_LENGTH: usize = 64;

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    info: MessageInfo,
    msg: InitMsg,
) -> InitResult {
    let state = State {
        denom: msg.denom.to_string(),
        owner: deps.api.canonical_address(&info.sender)?,
        listing_count: 0,
        staked_tokens: Uint128::zero(),
    };

    config(&mut deps.storage).save(&state)?;

    Ok(InitResponse::default())
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    info: MessageInfo,
    msg: HandleMsg,
) -> StdResult<HandleResponse> {
    match msg {
        // HandleMsg::StakeVotingTokens {} => stake_voting_tokens(deps, env, info),
        HandleMsg::WithdrawTokens { amount } => withdraw_tokens(deps, env, info, amount),
        HandleMsg::Bid {
            listing_id,
            price
        } => bid(deps, env, info, listing_id, price),
        HandleMsg::CloseBid { listing_id } => end_listing(deps, env, info, listing_id),
        HandleMsg::List {
            minimum_bid,
            start_height,
            end_height,
            description,
        } => create_listing(
            deps,
            env,
            info,
            minimum_bid,
            start_height,
            end_height,
            description,
        ),
    }
}

pub fn stake_voting_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    info: MessageInfo,
) -> HandleResult {
    let sender_address_raw = deps.api.canonical_address(&info.sender)?;
    let key = sender_address_raw.as_slice();

    let mut token_manager = bank_read(&deps.storage).may_load(key)?.unwrap_or_default();

    let mut state = config(&mut deps.storage).load()?;

    assert_sent_sufficient_coin(
        &info.sent_funds,
        Some(coin(MIN_STAKE_AMOUNT, &state.denom)),
    )?;
    let sent_funds = info
        .sent_funds
        .iter()
        .find(|coin| coin.denom.eq(&state.denom))
        .unwrap();

    token_manager.token_balance =  token_manager.token_balance + sent_funds.amount;

    state.staked_tokens = state.staked_tokens + sent_funds.amount;

    config(&mut deps.storage).save(&state)?;

    bank(&mut deps.storage).save(key, &token_manager)?;

    Ok(HandleResponse::default())
}

// Withdraw amount if not staked. By default all funds will be withdrawn.
pub fn withdraw_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    info: MessageInfo,
    amount: Option<Uint128>,
) -> HandleResult {
    let sender_address_raw = deps.api.canonical_address(&info.sender)?;
    let contract_address_raw = deps.api.canonical_address(&env.contract.address)?;
    let key = sender_address_raw.as_slice();

    if let Some(mut token_manager) = bank_read(&deps.storage).may_load(key)? {
        let largest_staked = locked_amount(&sender_address_raw, deps);
        let withdraw_amount = match amount {
            Some(amount) => Some(amount.u128()),
            None => Some(token_manager.token_balance.u128()),
        }
        .unwrap();
        if largest_staked + withdraw_amount > token_manager.token_balance.u128() {
            Err(StdError::generic_err(
                "User is trying to withdraw too many tokens.",
            ))
        } else {
            let balance = token_manager.token_balance.u128() - withdraw_amount;
            token_manager.token_balance = Uint128::from(balance);

            bank(&mut deps.storage).save(key, &token_manager)?;

            let mut state = config(&mut deps.storage).load()?;
            let staked_tokens = state.staked_tokens.u128() - withdraw_amount;
            state.staked_tokens = Uint128::from(staked_tokens);
            config(&mut deps.storage).save(&state)?;

            send_tokens(
                &deps.api,
                &contract_address_raw,
                &sender_address_raw,
                vec![coin(withdraw_amount, &state.denom)],
                "approve",
            )
        }
    } else {
        Err(StdError::generic_err("Nothing staked"))
    }
}

/// validate_description returns an error if the description is invalid
fn validate_description(description: &str) -> StdResult<()> {
    if description.len() < MIN_DESC_LENGTH {
        Err(StdError::generic_err("Description too short"))
    } else if description.len() > MAX_DESC_LENGTH {
        Err(StdError::generic_err("Description too long"))
    } else {
        Ok(())
    }
}

/// validate_quorum_percentage returns an error if the quorum_percentage is invalid
/// (we require 0-100)
fn validate_quorum_percentage(quorum_percentage: Option<u8>) -> StdResult<()> {
    if quorum_percentage.is_some() && quorum_percentage.unwrap() > 100 {
        Err(StdError::generic_err("quorum_percentage must be 0 to 100"))
    } else {
        Ok(())
    }
}

/// validate_end_height returns an error if the listing ends in the past
fn validate_end_height(end_height: Option<u64>, env: Env) -> StdResult<()> {
    if end_height.is_some() && env.block.height >= end_height.unwrap() {
        Err(StdError::generic_err("Listing cannot end in the past"))
    } else {
        Ok(())
    }
}

/// create a new listing
pub fn create_listing<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    info: MessageInfo,
    minimum_bid : Uint128,
    start_height: Option<u64>,
    end_height: Option<u64>,
    description: String,
) -> HandleResult {

    validate_end_height(end_height, env.clone())?;
    validate_description(&description)?;

    let mut state = config(&mut deps.storage).load()?;
    let listing_count = state.listing_count;
    let listing_id = listing_count + 1;
    state.listing_count = listing_id;

    let sender_address_raw = deps.api.canonical_address(&info.sender)?;

    let sent_nfts = info
        .sent_nfts
        .iter()
        .nth(0)
        .unwrap();

    let token_id = sent_nfts.id.to_string();
    let denom = sent_nfts.denom.to_string();

    let new_listing = Listing {
        token_id,
        denom,
        creator: sender_address_raw.clone(),
        status: BidStatus::InProgress,
        highest_bid: Uint128::zero(),
        highest_bidder: sender_address_raw.clone(),
        minimum_bid,
        bidders: vec![],
        bidders_info: vec![],
        start_height,
        end_height: end_height.unwrap_or(env.block.height + DEFAULT_END_HEIGHT_BLOCKS),
        description,
    };
    //
    let key = state.listing_count.to_string();
    listing(&mut deps.storage).save(key.as_bytes(), &new_listing)?;

    config(&mut deps.storage).save(&state)?;

    let r = HandleResponse {
        messages: vec![],
        attributes : vec![
            Attribute { key:"action".to_string(), value:"create_listing".to_string(), },
            // Attribute { key: "creator".to_string(), value: deps.api.human_address(&new_listing.creator)?.to_string(), },
            Attribute { key: "listing_id".to_string(), value: listing_id.to_string(), },
            // Attribute { key: "end_height".to_string(), value: new_listing.end_height.to_string(), },
            // Attribute { key: "start_height".to_string(), value: start_height.unwrap_or(0).to_string(), },
        ],
        data: Some(to_binary(&CreateListingResponse { listing_id })?),
    };

    Ok(r)

}

/*
 * Ends a listing. Only the creator of a given listing can end that listing.
 */
pub fn end_listing<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    info: MessageInfo,
    listing_id: u64,
) -> HandleResult {
    let key = &listing_id.to_string();
    let mut a_listing = listing(&mut deps.storage).load(key.as_bytes())?;

    let sender_address_raw = deps.api.canonical_address(&info.sender)?;
    if a_listing.creator != sender_address_raw {
        return Err(StdError::generic_err(
            "User is not the creator of the listing.",
        ));
    }

    if a_listing.status != BidStatus::InProgress {
        return Err(StdError::generic_err("Listing is not in progress"));
    }

    if a_listing.start_height.is_some() && a_listing.start_height.unwrap() > env.block.height {
        return Err(StdError::generic_err("Voting period has not started."));
    }

    if a_listing.end_height > env.block.height {
        return Err(StdError::generic_err("Voting period has not expired."));
    }

    let mut rejected_reason = "";
    let mut passed = false;

    if a_listing.minimum_bid <= a_listing.highest_bid {
        a_listing.status = BidStatus::Passed;

    } else {
        rejected_reason = "Bid price not reached minimum";
        a_listing.highest_bidder = a_listing.creator.clone();
        a_listing.status = BidStatus::Rejected;
    }
    listing(&mut deps.storage).save(key.as_bytes(), &a_listing)?;

    let creator_address = &a_listing.creator.clone();
    let bidder_address = &a_listing.highest_bidder.clone();
    let creator_key = creator_address.as_slice();
    let bidder_key = bidder_address.as_slice();
    let token_id = &a_listing.token_id;
    let denom = &a_listing.denom;
    let price = &a_listing.highest_bid;

    let mut creator_token_manager = bank_read(&deps.storage).may_load(creator_key)?.unwrap_or_default();
    let mut bidder_token_manager = bank_read(&deps.storage).may_load(bidder_key)?.unwrap_or_default();

    bidder_token_manager.token_balance = Uint128::from(creator_token_manager.token_balance.u128() - price.u128());
    creator_token_manager.token_balance = creator_token_manager.token_balance + price;

    bank(&mut deps.storage).save(bidder_key, &bidder_token_manager)?;
    bank(&mut deps.storage).save(creator_key, &creator_token_manager)?;

    let contract_address_raw = deps.api.canonical_address(&env.contract.address)?;
    send_nft(
        &deps.api,
        &contract_address_raw,
        &bidder_address,
        token_id.to_string(),
        denom.to_string(),
        "approve",
    );

    for bidder in &a_listing.bidders {
        unlock_tokens(deps, bidder, listing_id)?;
    }

    let attributes = vec![
        Attribute { key: "action".to_string(), value: "end_listing".to_string(), },
        Attribute { key: "listing_id".to_string(), value: listing_id.to_string(), },
        Attribute { key: "rejected_reason".to_string(), value: rejected_reason.to_string(), },
        Attribute { key: "passed".to_string(), value: passed.to_string(), },
    ];

    let r = HandleResponse {
        messages: vec![],
        attributes,
        data: None,
    };
    Ok(r)
}

// unlock bidder's tokens in a given listing
fn unlock_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    bidder: &CanonicalAddr,
    listing_id: u64,
) -> HandleResult {
    let bidder_key = &bidder.as_slice();
    let mut token_manager = bank_read(&deps.storage).load(bidder_key).unwrap();

    // unlock entails removing the mapped listing_id, retaining the rest
    token_manager.locked_tokens.retain(|(k, _)| k != &listing_id);
    bank(&mut deps.storage).save(bidder_key, &token_manager)?;
    Ok(HandleResponse::default())
}

// finds the largest locked amount in participated listings.
fn locked_amount<S: Storage, A: Api, Q: Querier>(
    bidder: &CanonicalAddr,
    deps: &mut Extern<S, A, Q>,
) -> u128 {
    let bidder_key = &bidder.as_slice();
    let token_manager = bank_read(&deps.storage).load(bidder_key).unwrap();
    token_manager
        .locked_tokens
        .iter()
        .map(|(_, v)| v.u128())
        .max()
        .unwrap_or_default()
}

fn has_bidden(bidder: &CanonicalAddr, a_listing: &Listing) -> bool {
    a_listing.bidders.iter().any(|i| i == bidder)
}

// stake token and bid for listing
pub fn bid<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    info: MessageInfo,
    listing_id: u64,
    price: Uint128,
) -> HandleResult {
    let sender_address_raw = deps.api.canonical_address(&info.sender)?;
    let listing_key = &listing_id.to_string();
    let bank_key = sender_address_raw.as_slice();
    let state = config_read(&deps.storage).load()?;

    if listing_id == 0 || state.listing_count > listing_id {
        return Err(StdError::generic_err("Listing does not exist"));
    }

    let mut a_listing = listing(&mut deps.storage).load(listing_key.as_bytes())?;

    if a_listing.status != BidStatus::InProgress {
        return Err(StdError::generic_err("Listing is not in progress"));
    }

    if price <= a_listing.highest_bid {
        return Err(StdError::generic_err("Set price higher than highest bid"));
    }

    if has_bidden(&sender_address_raw, &a_listing) {
        return Err(StdError::generic_err("User has already bidden."));
    }

    let sent_funds = info
        .sent_funds
        .iter()
        .find(|coin| coin.denom.eq(&state.denom))
        .unwrap();

    let mut token_manager = bank_read(&deps.storage).may_load(bank_key)?.unwrap_or_default();

    if token_manager.token_balance + sent_funds.amount < price {
        return Err(StdError::generic_err(
            "User does not have enough staked tokens.",
        ));
    }
    // add sent funds to token manager balance
    token_manager.token_balance =  token_manager.token_balance + sent_funds.amount;
    token_manager.participated_bids.push(listing_id);
    token_manager.locked_tokens.push((listing_id, price));
    bank(&mut deps.storage).save(bank_key, &token_manager)?;

    // mutation for listing state
    a_listing.bidders.push(sender_address_raw.clone());
    let bidder_info = Bidder { bidder: sender_address_raw.clone(), price};
    a_listing.bidders_info.push(bidder_info);
    a_listing.highest_bid = price;
    a_listing.highest_bidder = sender_address_raw.clone();
    listing(&mut deps.storage).save(listing_key.as_bytes(), &a_listing)?;

    let attributes = vec![
        Attribute { key: "action".to_string(), value: "bidden".to_string(), },
        Attribute { key: "listing_id".to_string(), value:  listing_id.to_string(), },
    ];

    let r = HandleResponse {
        messages: vec![],
        attributes,
        data: None,
    };
    Ok(r)
}

fn send_tokens<A: Api>(
    api: &A,
    from_address: &CanonicalAddr,
    to_address: &CanonicalAddr,
    amount: Vec<Coin>,
    action: &str,
) -> HandleResult {
    let from_human = api.human_address(from_address)?;
    let to_human = api.human_address(to_address)?;
    let attributes = vec![Attribute { key: "action".to_string(), value: action.to_string(), }, Attribute { key: "to".to_string(), value: to_human.to_string(), },];

    let r = HandleResponse {
        messages: vec![CosmosMsg::Bank(BankMsg::Send {
            from_address: from_human,
            to_address: to_human,
            amount,
        })],
        attributes,
        data: None,
    };
    Ok(r)
}

fn send_nft<A: Api>(
    api: &A,
    from_address: &CanonicalAddr,
    to_address: &CanonicalAddr,
    token_id: String,
    denom: String,
    action: &str,
) -> HandleResult {
    let from_human = api.human_address(from_address)?;
    let to_human = api.human_address(to_address)?;
    let attributes = vec![Attribute { key: "action".to_string(), value: action.to_string(), }, Attribute { key: "to".to_string(), value: to_human.to_string(), },];

    let r = HandleResponse {
        messages: vec![CosmosMsg::Nft(NftMsg::Transfer {
            sender: from_human,
            recipient: to_human,
            id: token_id.to_string(),
            denom: denom.to_string(),
        })],
        attributes,
        data: None,
    };
    Ok(r)
}

//クエリ値をバイナリデータとして返す
pub fn query<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
    _env: Env,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&config_read(&_deps.storage).load()?),
        QueryMsg::TokenStake { address } => token_balance(_deps, address),
        QueryMsg::Listing { listing_id } => query_listing(_deps, listing_id),
    }
}

fn query_listing<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    listing_id: u64,
) -> StdResult<Binary> {
    let key = &listing_id.to_string();
//listing_readはstateにて定義、バイナリをオブジェクト化
    let listing = match listing_read(&deps.storage).may_load(key.as_bytes())? {
//型マッチしていれば返す
        Some(listing) => Some(listing),
        None => return Err(StdError::generic_err("Listing does not exist")),
    }
    .unwrap();
//listingオブジェクトの情報とメタデータからオブジェクト生成
    let resp = ListingResponse {
        token_id: listing.token_id,
        denom: listing.denom,
        creator: deps.api.human_address(&listing.creator).unwrap(),
        status: listing.status,
        highest_bid: listing.highest_bid,
        highest_bidder: deps.api.human_address(&listing.highest_bidder).unwrap(),
        end_height: Some(listing.end_height),
        start_height: listing.start_height,
        description: listing.description,
    };
//バイナリで返す
    to_binary(&resp)
}

fn token_balance<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    address: HumanAddr,
) -> StdResult<Binary> {
    let key = deps.api.canonical_address(&address).unwrap();

    let token_manager = bank_read(&deps.storage)
        .may_load(key.as_slice())?
        .unwrap_or_default();

    let resp = TokenStakeResponse {
        token_balance: token_manager.token_balance,
    };

    to_binary(&resp)
}

pub fn get_nft<S: Storage, A: Api, Q: Querier>(
    _deps: &mut Extern<S, A, Q>,
    env: Env,
    info: MessageInfo,
    denom: String,
    id: String,
) -> HandleResult {
    let sender_address = info.sender;
    let contract_address = env.contract.address;
    let denom = &denom.to_string();
    let id = &id.to_string();

    let attributes = vec![Attribute { key: "id".to_string(), value: id.to_string(), }, Attribute { key: "to".to_string(), value: sender_address.to_string(), },];

    let r = HandleResponse {
        messages: vec![CosmosMsg::Nft(NftMsg::Transfer {
            sender: contract_address,
            recipient: sender_address,
            denom: denom.to_string(),
            id: id.to_string(),
        })],
        attributes,
        data: None,
    };
    Ok(r)
}
