# NFT Auction Contract

**nftchain** is a blockchain application built using Cosmos SDK and Tendermint and generated with [Starport](https://github.com/tendermint/starport).

## CLI commands

## Query

```nftchaincli query wasm contract-state smart <contract address> <query json>```

config

```"{\"config\": {}}"```

listing

```"{\"listing\": {\"listing_id\": 1}}"```

staked token balance

```"{\"token_stake\": {\"address\": \"cosmos1xhp3d89fxv54c64lj30gule2d0ajudx20kveha\"}}"```

## Execute

```nftchaincli tx wasm execute <contract address> <query json> --from <transactor address>```

list

```"{\"list\": {\"minimum_bid\": \"10\",\"start_height\": 1,\"end_height\": 300,\"description\":\"first listing\"}}"```

bid 

```"{\"bid\": {\"listing_id\": 1,\"price\": \"50\"}}"```

close bid 

```"{\"close_bid\": {\"listing_id\": 1 }"```
