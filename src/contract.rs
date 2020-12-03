use crate::coin_helpers::assert_sent_sufficient_coin;

use crate::msg::{
    CreateListingResponse, HandleMsg, InitMsg, ListingResponse, QueryMsg, TokenStakeResponse,
};
use crate::state::{
    bank, bank_read, config, config_read, listing, listing_read, Listing, ListingStatus, State, Bidder,
};
use cosmwasm_std::{
    coin, to_binary, Api, Attribute, BankMsg, Binary, CanonicalAddr, Coin, CosmosMsg, Env, Extern,
    HandleResponse, HandleResult, HumanAddr, InitResponse, InitResult, Querier, StdError,
    StdResult, Storage, Uint128, MessageInfo, NftMsg,
};


pub const VOTING_TOKEN: &str = "voting_token";
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
        HandleMsg::WithdrawTokens { amount } => withdrawtokens(deps, env, info, amount),
        HandleMsg::Bid {
            listing_id,
            price
        } => bid(deps, env, info, listing_id, price),
        HandleMsg::EndListing { listing_id } => end_listing(deps, env, info, listing_id),
        HandleMsg::List {
            token_id,
            denom,
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
        //handle for Nfts
        HandleMsg::GetNft { denom, id, } => get_nft(deps, env, info, denom, id),
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
        let largest_staked = locked_amount(&sender_address_raw,d deps);
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
) -> StdResult<HandleResponse> {
    validate_quorum_percentage(quorum_percentage)?;
    validate_end_height(end_height, env.clone())?;
    validate_description(&description)?;

    let mut state = config(&mut deps.storage).load()?;
    let listing_count = state.listing_count;
    let listing_id = listing_count + 1;
    state.listing_count = listing_id;

    let sender_address_raw = deps.api.canonical_address(&info.sender)?;
    let token_id = &info.sent_nfts.id;
    let denom = &info.sent_nfts.denom;

    let new_listing = Listing {
        token_id,
        denom,
        creator: sender_address_raw,
        status: ListingStatus::InProgress,
        highest_bid: Uint128::zero(),
        minimum_bid,
        bidders: vec![],
        bidders_info: vec![],
        start_height,
        end_height: end_height.unwrap_or(env.block.height + DEFAULT_END_HEIGHT_BLOCKS),
        description,
    };
    let key = state.listing_count.to_string();
    listing(&mut deps.storage).save(key.as_bytes(), &new_listing)?;

    config(&mut deps.storage).save(&state)?;

    let r = HandleResponse {
        messages: vec![],
        attributes : vec![
            Attribute { key:"action".to_string(), value:"create_listing".to_string(), },
            Attribute { key: "creator".to_string(), value: deps.api.human_address(&new_listing.creator)?.to_string(), },
            Attribute { key: "listing_id".to_string(), value: listing_id.to_string(), },
            Attribute { key: "end_height".to_string(), value: new_listing.end_height.to_string(), },
            Attribute { key: "start_height".to_string(), value: start_height.unwrap_or(0).to_string(), },
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

    if a_listing.status != ListingStatus::InProgress {
        return Err(StdError::generic_err("Listing is not in progress"));
    }

    if a_listing.start_height.is_some() && a_listing.start_height.unwrap() > env.block.height {
        return Err(StdError::generic_err("Voting period has not started."));
    }

    if a_listing.end_height > env.block.height {
        return Err(StdError::generic_err("Voting period has not expired."));
    }

    let mut no = 0u128;
    let mut yes = 0u128;

    for voter in &a_listing.voter_info {
        if voter.vote == "yes" {
            yes += voter.weight.u128();
        } else {
            no += voter.weight.u128();
        }
    }
    let tallied_weight = yes + no;

    let listing_status = ListingStatus::Rejected;
    let mut rejected_reason = "";
    let mut passed = false;

    if tallied_weight > 0 {
        let state = config_read(&deps.storage).load()?;

        let staked_weight = deps
            .querier
            .query_balance(&env.contract.address, &state.denom)
            .unwrap()
            .amount
            .u128();

        if staked_weight == 0 {
            return Err(StdError::generic_err("Nothing staked"));
        }

        let quorum = ((tallied_weight / staked_weight) * 100) as u8;
        if a_listing.quorum_percentage.is_some() && quorum < a_listing.quorum_percentage.unwrap() {
            // Quorum: More than quorum_percentage of the total staked tokens at the end of the voting
            // period need to have participated in the vote.
            rejected_reason = "Quorum not reached";
        } else if yes > tallied_weight / 2 {
            //Threshold: More than 50% of the tokens that participated in the vote
            // (after excluding “Abstain” votes) need to have voted in favor of the proposal (“Yes”).
            a_listing.status = ListingStatus::Passed;
            passed = true;
        } else {
            rejected_reason = "Threshold not reached";
        }
    } else {
        rejected_reason = "Quorum not reached";
    }
    a_listing.status = listing_status;
    listing(&mut deps.storage).save(key.as_bytes(), &a_listing)?;

    for voter in &a_listing.voters {
        unlock_tokens(deps, voter, listing_id)?;
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

// unlock voter's tokens in a given listing
fn unlock_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    voter: &CanonicalAddr,
    listing_id: u64,
) -> HandleResult {
    let voter_key = &voter.as_slice();
    let mut token_manager = bank_read(&deps.storage).load(voter_key).unwrap();

    // unlock entails removing the mapped listing_id, retaining the rest
    token_manager.locked_tokens.retain(|(k, _)| k != &listing_id);
    bank(&mut deps.storage).save(voter_key, &token_manager)?;
    Ok(HandleResponse::default())
}

// finds the largest locked amount in participated listings.
fn locked_amount<S: Storage, A: Api, Q: Querier>(
    voter: &CanonicalAddr,
    deps: &mut Extern<S, A, Q>,
) -> u128 {
    let voter_key = &voter.as_slice();
    let token_manager = bank_read(&deps.storage).load(voter_key).unwrap();
    token_manager
        .locked_tokens
        .iter()
        .map(|(_, v)| v.u128())
        .max()
        .unwrap_or_default()
}

fn has_voted(voter: &CanonicalAddr, a_listing: &Listing) -> bool {
    a_listing.voters.iter().any(|i| i == voter)
}

pub fn bid<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    info: MessageInfo,
    listing_id: u64,
    price: Uint128,
) -> HandleResult {
    let sender_address_raw = deps.api.canonical_address(&info.sender)?;
    let listing_key = &listing_id.to_string();
    let state = config_read(&deps.storage).load()?;
    if listing_id == 0 || state.listing_count > listing_id {
        return Err(StdError::generic_err("Listing does not exist"));
    }

    let mut a_listing = listing(&mut deps.storage).load(listing_key.as_bytes())?;

    if a_listing.status != ListingStatus::InProgress {
        return Err(StdError::generic_err("Listing is not in progress"));
    }

    if has_voted(&sender_address_raw, &a_listing) {
        return Err(StdError::generic_err("User has already voted."));
    }

    let key = &sender_address_raw.as_slice();
    let mut token_manager = bank_read(&deps.storage).may_load(key)?.unwrap_or_default();

    if token_manager.token_balance < weight {
        return Err(StdError::generic_err(
            "User does not have enough staked tokens.",
        ));
    }
    token_manager.participated_listings.push(listing_id);
    token_manager.locked_tokens.push((listing_id, price));
    bank(&mut deps.storage).save(key, &token_manager)?;

    a_listing.bidders.push(sender_address_raw.clone());

    let bidder_info = Bidder { sender_address_raw, price};

    a_listing.voter_info.push(voter_info);
    listing(&mut deps.storage).save(listing_key.as_bytes(), &a_listing)?;

    let attributes = vec![
        Attribute { key: "action".to_string(), value: "vote_casted".to_string(), },
        Attribute { key: "listing_id".to_string(), value:  listing_id.to_string(), },
        Attribute { key: "weight".to_string(), value: weight.to_string(), },
        Attribute { key: "voter".to_string(), value: info.sender.to_string(), },
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
        creator: deps.api.human_address(&listing.creator).unwrap(),
        status: listing.status,
        quorum_percentage: listing.quorum_percentage,
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
