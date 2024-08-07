use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, UnorderedMap, UnorderedSet};
use near_sdk::json_types::{U128, U64};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{
    assert_one_yocto, env, ext_contract, near_bindgen, serde_json::json, AccountId,
    BorshStorageKey, CryptoHash, Gas, PanicOnDefault, Promise, is_promise_success, promise_result_as_success, NearToken };
use std::collections::HashMap;
use crate::external::*;

mod external;
mod nft_callbacks;

pub const FIVE_MINUTES: u64 = 300000000000;
const DELIMETER: &str = "||";
pub const STORAGE_ADD_MARKET_DATA: u128 = 8590000000000000000000;

const ONE_YOCTONEAR: NearToken = NearToken::from_yoctonear(1);
const GAS_FOR_RESOLVE_PURCHASE: Gas = Gas::from_tgas(115);
const GAS_FOR_NFT_TRANSFER: Gas = Gas::from_tgas(15);

pub type TokenId = String;
pub type ContractAndTokenId = String;
pub type PayoutHashMap = HashMap<AccountId, U128>;

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Payout {
    pub payout: PayoutHashMap,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct MarketplaceConfig {
    pub owner_id: AccountId,
    pub treasury_id: AccountId,
    pub transaction_fee: u16,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct MarketDataJson {
    owner_id: AccountId,
    approval_id: U64,
    nft_contract_id: AccountId,
    token_id: TokenId,
    price: U128,
    bids: Option<Bids>,
    started_at: Option<U64>,
    ended_at: Option<U64>,
    end_price: Option<U128>, // dutch auction
    is_auction: Option<bool>,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Bid {
    pub bidder_id: AccountId,
    pub price: U128,
    pub time: u64
}

pub type Bids = Vec<Bid>;

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct MarketData {
    pub owner_id: AccountId,
    pub approval_id: u64,
    pub nft_contract_id: AccountId,
    pub token_id: TokenId,
    pub price: u128,            // if auction, price becomes starting price
    pub bids: Option<Bids>,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub end_price: Option<u128>, // dutch auction
    pub is_auction: Option<bool>,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Marketplace {
    pub owner_id: AccountId,
    pub treasury_id: AccountId,
    pub approved_nft_contract_ids: UnorderedSet<AccountId>,
    pub storage_deposits: LookupMap<AccountId, u128>,
    pub transaction_fee: u16,
    pub by_owner_id: LookupMap<AccountId, UnorderedSet<TokenId>>,
    pub market: UnorderedMap<ContractAndTokenId, MarketData>,
}

#[derive(BorshStorageKey, BorshSerialize)]
pub enum StorageKey {
    StorageDeposits,
    NFTContractIds,
    ByOwnerId,
    Market,
    ByOwnerIdInner { account_id_hash: CryptoHash },
}

#[near_bindgen]
impl Marketplace {
    #[init]
    pub fn new(
        owner_id: AccountId,
        treasury_id: AccountId,
        approved_nft_contract_ids: Option<Vec<AccountId>>,
        current_fee: u16
    ) -> Self {
        let mut this = Self {
            transaction_fee: current_fee,
            owner_id: owner_id.into(),
            treasury_id: treasury_id.into(),
            storage_deposits: LookupMap::new(StorageKey::StorageDeposits),
            by_owner_id: LookupMap::new(StorageKey::ByOwnerId),
            approved_nft_contract_ids: UnorderedSet::new(StorageKey::NFTContractIds),
            market: UnorderedMap::new(StorageKey::Market),
        };
        add_accounts(
            approved_nft_contract_ids,
            &mut this.approved_nft_contract_ids,
        );
        this
    }
    
    #[payable]
    pub fn buy(&mut self, nft_contract_id: AccountId, token_id: TokenId) {
        let contract_and_token_id = format!("{}{}{}", &nft_contract_id, DELIMETER, token_id);
        let market_data = self.market.get(&contract_and_token_id).expect("DS: Market data doesn't exist");
        let buyer_id = env::predecessor_account_id();
        assert_ne!(
            buyer_id, market_data.owner_id,
            "DS: Cannot buy your own sale"
        );
        assert_eq!(env::attached_deposit().as_yoctonear(), market_data.price, "DS: Insufficient Balance");
        self.internal_process_purchase(nft_contract_id.into(), token_id, buyer_id, env::attached_deposit().as_yoctonear());  
    }
    #[payable]
    pub fn add_bid(
        &mut self,
        nft_contract_id: AccountId,
        token_id: TokenId,
        amount: U128
    ) {
        let contract_and_token_id = format!("{}{}{}", &nft_contract_id, DELIMETER, token_id);
        let mut market_data = self
            .market
            .get(&contract_and_token_id)
            .expect("DS: Token id does not exist");
        let bidder_id = env::predecessor_account_id();
        let current_time = env::block_timestamp();
        if market_data.started_at.is_some() {
            assert!(
                current_time >= market_data.started_at.unwrap(),
                "DS: Sale has not started yet"
            );
        }
        if market_data.ended_at.is_some() {
            assert!(
                current_time <= market_data.ended_at.unwrap(),
                "DS: Sale has ended"
            );
        }
        let remaining_time = market_data.ended_at.unwrap() - current_time;
        if remaining_time <= FIVE_MINUTES {
            let extended_ended_at = market_data.ended_at.unwrap() + FIVE_MINUTES;
            market_data.ended_at = Some(extended_ended_at);

            env::log_str(
                &json!({
                    "type": "extend_auction",
                    "params": {
                        "nft_contract_id": nft_contract_id,
                        "token_id": token_id,
                        "ended_at": extended_ended_at,
                    }
                })
                .to_string(),
            );
        }
        assert_ne!(
            market_data.owner_id, bidder_id,
            "DS: Owner cannot bid their own token"
        );
        assert!(
            env::attached_deposit() >= NearToken::from_yoctonear(amount.into()),
            "DS: attached deposit is less than amount"
        );
        let new_bid = Bid {
            bidder_id: bidder_id.clone(),
            price: amount.into(),
            time: current_time
        };
        let mut bids = market_data.bids.unwrap_or(Vec::new());
        if !bids.is_empty() {
            let current_bid = &bids[bids.len() - 1];

            assert!(
                amount.0 >= current_bid.price.0 + (current_bid.price.0 / 100 * 10),
                "DS: Can't pay less than or equal to current bid price + 10% : {:?}",
                current_bid.price.0 + (current_bid.price.0 / 100 * 10)
            );
            // Retain all elements except account_id
            bids.retain(|bid| {
                if bid.bidder_id == bidder_id {
                    // refund
                    Promise::new(bid.bidder_id.clone()).transfer(NearToken::from_yoctonear(bid.price.0));
                }
                bid.bidder_id != bidder_id
            });
        } else {
            assert!(
                amount.0 >= market_data.price,
                "DS: Can't pay less than starting price: {:?}",
                market_data.price
            );
        }
        market_data.ended_at = Some(current_time + FIVE_MINUTES);
        bids.push(new_bid);
        market_data.bids = Some(bids);
        self.market.insert(&contract_and_token_id, &market_data);
        // Remove first element if bids.length > 50
        let updated_bids = market_data.bids.unwrap_or(Vec::new());
        if updated_bids.len() >= 100 {
            self.internal_cancel_bid(
                nft_contract_id.clone(),
                token_id.clone(),
                updated_bids[0].bidder_id.clone(),
            )
        }
        env::log_str(
            &json!({
                "type": "add_bid",
                "params": {
                    "bidder_id": bidder_id,
                    "nft_contract_id": nft_contract_id,
                    "token_id": token_id,
                    "amount": amount,
                    "ended_at": current_time.to_string()
                }
            })
            .to_string(),
        );
    }

    #[payable]
    pub fn cancel_bid(
        &mut self,
        nft_contract_id: AccountId,
        token_id: TokenId,
        account_id: AccountId,
    ) {
        assert_one_yocto();
        let contract_and_token_id = format!("{}{}{}", &nft_contract_id, DELIMETER, token_id);
        let market_data = self
            .market
            .get(&contract_and_token_id)
            .expect("DS: Token id does not exist");

        let bids = market_data.bids.unwrap();

        assert!(!bids.is_empty(), "DS: Bids data does not exist");

        for x in 0..bids.len() {
            if bids[x].bidder_id == account_id {
                assert!(
                    [bids[x].bidder_id.clone(), self.owner_id.clone()]
                        .contains(&env::predecessor_account_id()),
                    "DS: Bidder or owner only"
                );
            }
        }

        self.internal_cancel_bid(nft_contract_id, token_id, account_id);
    }

    #[payable]
    pub fn accept_bid(&mut self, nft_contract_id: AccountId, token_id: TokenId) {
        assert_one_yocto();
        let contract_and_token_id = format!("{}{}{}", &nft_contract_id, DELIMETER, token_id);
        let mut market_data = self
            .market
            .get(&contract_and_token_id)
            .expect("DS: Token id does not exist");
        let current_time: u64 = env::block_timestamp();

        let mut bids = market_data.bids.unwrap();

        assert!(!bids.is_empty(), "DS: Cannot accept bid with empty bid");

        let selected_bid = bids.remove(bids.len() - 1);

        println!(
            "\nAccept Bid Accounts {:?}, {:?}, {:?}",
            market_data.owner_id.clone(),
            self.owner_id.clone(),
            env::predecessor_account_id()
        );
        assert!(
            [
                market_data.owner_id.clone(),
                self.owner_id.clone(),
                selected_bid.bidder_id.clone()
            ]
            .contains(&env::predecessor_account_id()),
            "DS: Seller, owner or top bidder only"
        );
        if env::predecessor_account_id() != self.owner_id.clone() && market_data.ended_at.is_some()
        {
            assert!(
                current_time >= market_data.ended_at.unwrap(),
                "DS: Auction has not ended yet"
            );
        }
        assert!(
            market_data.end_price.is_none(),
            "DS: Dutch auction does not accept accept_bid"
        );
        for bid in &bids {
            Promise::new(bid.bidder_id.clone()).transfer(NearToken::from_yoctonear(bid.price.0));
        }
        bids.clear();

        market_data.bids = Some(bids);
        self.market.insert(&contract_and_token_id, &market_data);
        self.internal_process_purchase(
            market_data.nft_contract_id,
            token_id,
            selected_bid.bidder_id.clone(),
            selected_bid.price.clone().0,
        );
    }

    #[payable]
    pub fn delete_market_data(&mut self, nft_contract_id: AccountId, token_id: TokenId) {
        assert_one_yocto();
        let contract_and_token_id = format!("{}{}{}", nft_contract_id, DELIMETER, token_id);
        let current_time: u64 = env::block_timestamp();
        let market_data = self.market.get(&contract_and_token_id).expect("DS: Market data does not exist");
        assert!(
            [market_data.owner_id.clone(), self.owner_id.clone()]
                .contains(&env::predecessor_account_id()),
            "DS: Seller or owner only"
        );
        if market_data.is_auction.is_some() && env::predecessor_account_id() == self.owner_id {
          assert!(
            current_time >= market_data.ended_at.unwrap(),
            "DS: Auction has not ended yet"
          );
        }
        self.internal_delete_market_data(&nft_contract_id, &token_id);

        env::log_str(
            &json!({
                "type": "delete_market_data",
                "params": {
                    "owner_id": market_data.owner_id,
                    "nft_contract_id": nft_contract_id,
                    "token_id": token_id,
                }
            })
            .to_string(),
        );
    }

    fn internal_cancel_bid(
        &mut self,
        nft_contract_id: AccountId,
        token_id: TokenId,
        account_id: AccountId
    ) {
        let contract_and_token_id = format!("{}{}{}", &nft_contract_id, DELIMETER, token_id);
        let mut market_data = self
            .market
            .get(&contract_and_token_id)
            .expect("DS: Token id does not exist");
        let mut bids = market_data.bids.unwrap();

        assert!(!bids.is_empty(), "DS: Bids data does not exist");
        for x in 0..bids.len() {
            if bids[x].bidder_id == account_id {
                Promise::new(bids[x].bidder_id.clone()).transfer(NearToken::from_yoctonear(bids[x].price.0));
            }
        }
        bids.retain(|bid| bid.bidder_id != account_id);
        market_data.bids = Some(bids);
        self.market.insert(&contract_and_token_id, &market_data);

        env::log_str(
            &json!({
              "type": "cancel_bid",
              "params": {
                "bidder_id": account_id, "nft_contract_id": nft_contract_id, "token_id": token_id
              }
            })
            .to_string(),
        );
    }

    fn internal_process_purchase(
        &mut self,
        nft_contract_id: AccountId,
        token_id: TokenId,
        buyer_id: AccountId,
        price: u128
    ) -> Promise {
        let market_data = self
            .internal_delete_market_data(&nft_contract_id, &token_id)
            .expect("DS: Sale does not exist");
        ext_contract::ext(nft_contract_id)
            .with_attached_deposit(ONE_YOCTONEAR)
            .with_static_gas(GAS_FOR_NFT_TRANSFER)
            .nft_transfer_payout(
                buyer_id.clone(), 
                token_id, 
                Some(market_data.approval_id),
                Some(price.into()),
            )
        .then(
            Self::ext(env::current_account_id())
            .with_static_gas(GAS_FOR_RESOLVE_PURCHASE)
            .resolve_purchase(
                buyer_id,
                market_data,
                price.into()
            )
        )
    }

    #[private]
    pub fn resolve_purchase(
        &mut self,
        buyer_id: AccountId,
        market_data: MarketData,
        price: U128
    ) -> U128 {
        let payout_option = promise_result_as_success().and_then(|value| {
            let parsed_payout = near_sdk::serde_json::from_slice::<PayoutHashMap>(&value);
            if parsed_payout.is_err() {
                near_sdk::serde_json::from_slice::<Payout>(&value)
                    .ok()
                    .and_then(|payout| {
                        let mut remainder = price.0;
                        for &value in payout.payout.values() {
                            remainder = remainder.checked_sub(value.0)?;
                        }
                        if remainder <= 100 {
                            Some(payout.payout)
                        } else {
                            None
                        }
                    })
            } else {
                parsed_payout.ok().and_then(|payout| {
                    let mut remainder = price.0;
                    for &value in payout.values() {
                        remainder = remainder.checked_sub(value.0)?;
                    }
                    if remainder <= 100 {
                        Some(payout)
                    } else {
                        None
                    }
                })
            }
        });
        let payout = if let Some(payout_option) = payout_option {
            payout_option
        } else {
            if !is_promise_success() {
                Promise::new(buyer_id.clone()).transfer(NearToken::from_yoctonear(u128::from(price.0)));
            } else {
                let treasury_fee: u128 = price.0 * self.transaction_fee as u128 / 10_000u128;
                Promise::new(market_data.owner_id.clone()).transfer(NearToken::from_yoctonear(price.0 - treasury_fee));
                if treasury_fee > 0 {
                    Promise::new(self.treasury_id.clone()).transfer(NearToken::from_yoctonear(treasury_fee));
                }
                env::log_str(
                    &json!({
                        "type": "resolve_purchase",
                        "params": {
                            "owner_id": &market_data.owner_id,
                            "nft_contract_id": &market_data.nft_contract_id,
                            "token_id": &market_data.token_id,
                            "price": price,
                            "buyer_id": buyer_id,
                        }
                    })
                    .to_string(),
                );
            }
            return price
        };
        let treasury_fee: u128 = price.0 * self.transaction_fee as u128 / 10_000u128;
        for (receiver_id, amount) in payout {
            if receiver_id == market_data.owner_id {
                Promise::new(receiver_id).transfer(NearToken::from_yoctonear(amount.0 - treasury_fee));
                Promise::new(self.treasury_id.clone()).transfer(NearToken::from_yoctonear(treasury_fee));
            } else {
                Promise::new(receiver_id).transfer(NearToken::from_yoctonear(amount.0));
            }
        }
        env::log_str(
            &json!({
                "type": "resolve_purchase",
                "params": {
                    "owner_id": &market_data.owner_id,
                    "nft_contract_id": &market_data.nft_contract_id,
                    "token_id": &market_data.token_id,
                    "price": price,
                    "buyer_id": buyer_id,
                }
            })
            .to_string(),
        );
        return price
    }
    
    fn internal_add_market_data(
        &mut self,
        owner_id: AccountId,
        approval_id: u64,
        nft_contract_id: AccountId,
        token_id: TokenId,
        price: U128,
        mut started_at: Option<U64>,
        ended_at: Option<U64>,
        end_price: Option<U128>,
        is_auction: Option<bool>,
    ) {
        let contract_and_token_id = format!("{}{}{}", nft_contract_id, DELIMETER, token_id);
        let bids: Option<Bids> = match is_auction {
            Some(u) => {
                if u {
                    Some(Vec::new())
                } else {
                    None
                }
            }
            None => None,
        };
        let current_time: u64 = env::block_timestamp();
        if started_at.is_some() {
            // if start time is behind that current time, makes it current time
            if started_at.unwrap().0 <= current_time {
                started_at = Some(current_time.into());
            }
            // assert!(started_at.unwrap().0 >= current_time);

            if ended_at.is_some() {
                assert!(started_at.unwrap().0 < ended_at.unwrap().0);
            }
            println!(
                "\n\n\nstarted_at Price {:?},{:?},{:?}\n\n",
                started_at.unwrap(),
                current_time,
                env::block_timestamp()
            );
        }
        if let Some(is_auction) = is_auction {
            if is_auction == true {
                if started_at.is_none() {
                    started_at = Some(U64(current_time));
                }
                assert!(ended_at.is_some(), "DS: Ended at is none");
            }
        }
        if ended_at.is_some() {
            assert!(ended_at.unwrap().0 >= current_time);
        }

        if end_price.is_some() {
            assert!(
                end_price.unwrap().0 < price.0,
                "DS: End price is more than starting price"
            );
        }
        self.market.insert(
            &contract_and_token_id,
            &MarketData {
                owner_id: owner_id.clone().into(),
                approval_id,
                nft_contract_id: nft_contract_id.clone().into(),
                token_id: token_id.clone(),
                price: price.into(),
                bids: bids,
                started_at: match started_at {
                    Some(x) => Some(x.0),
                    None => None,
                },
                ended_at: match ended_at {
                    Some(x) => Some(x.0),
                    None => None,
                },
                end_price: match end_price {
                    Some(x) => Some(x.0),
                    None => None,
                },
                is_auction: is_auction,
            },
        );
        let mut token_ids = self.by_owner_id.get(&owner_id).unwrap_or_else(|| {
            UnorderedSet::new(
                StorageKey::ByOwnerIdInner {
                    account_id_hash: hash_account_id(&owner_id),
                }
            )
        });
        token_ids.insert(&contract_and_token_id);
        self.by_owner_id.insert(&owner_id, &token_ids);
        env::log_str(
            &json!({
                "type": "add_market_data",
                "params": {
                    "owner_id": owner_id,
                    "approval_id": approval_id,
                    "nft_contract_id": nft_contract_id,
                    "token_id": token_id,
                    "price": price,
                    "started_at": started_at,
                    "ended_at": ended_at,
                    "end_price": end_price,
                    "is_auction": is_auction,
                }
            })
            .to_string(),
        );
    }

    fn internal_delete_market_data(
        &mut self,
        nft_contract_id: &AccountId,
        token_id: &TokenId,
    ) -> Option<MarketData> {
        let contract_and_token_id = format!("{}{}{}", &nft_contract_id, DELIMETER, token_id);
        let market_data = self.market.get(&contract_and_token_id);
        if market_data.is_some() {
            self.market.remove(&contract_and_token_id);
        }
        market_data.map(|market_data| {
            let by_owner_id = self.by_owner_id.get(&market_data.owner_id);
            if let Some(mut by_owner_id) = by_owner_id {
                by_owner_id.remove(&contract_and_token_id);
                if by_owner_id.is_empty() {
                    self.by_owner_id.remove(&market_data.owner_id);
                } else {
                    self.by_owner_id.insert(&market_data.owner_id, &by_owner_id);
                }
            }
            market_data
        })
    }

    #[payable]
    pub fn storage_deposit(&mut self, account_id: Option<AccountId>) {
        let storage_account_id = account_id
            .map(|a| a.into())
            .unwrap_or_else(env::predecessor_account_id);
        let deposit = env::attached_deposit();
        assert!(
            deposit.as_yoctonear() >= STORAGE_ADD_MARKET_DATA,
            "Requires minimum deposit of {}",
            STORAGE_ADD_MARKET_DATA
        );

        let mut balance: u128 = self.storage_deposits.get(&storage_account_id).unwrap_or(0);
        balance += deposit.as_yoctonear();
        self.storage_deposits.insert(&storage_account_id, &balance);
    }

    #[payable]
    pub fn storage_withdraw(&mut self) {
        assert_one_yocto();
        let owner_id = env::predecessor_account_id();
        let mut amount = self.storage_deposits.remove(&owner_id).unwrap_or(0);
        let market_data_owner = self.by_owner_id.get(&owner_id);
        let len = market_data_owner.map(|s| s.len()).unwrap_or_default();
        let diff = u128::from(len) * STORAGE_ADD_MARKET_DATA;
        amount -= diff;
        if amount > 0 {
            Promise::new(owner_id.clone()).transfer(NearToken::from_yoctonear(amount));
        }
        if diff > 0 {
            self.storage_deposits.insert(&owner_id, &diff);
        }
    }
    pub fn storage_minimum_balance(&self) -> U128 {
        U128(STORAGE_ADD_MARKET_DATA)
    }
    pub fn get_supply_by_owner_id(&self, account_id: AccountId) -> U64 {
        self.by_owner_id
            .get(&account_id)
            .map_or(0, |by_owner_id| by_owner_id.len())
            .into()
    }
    #[payable]
    pub fn set_treasury(&mut self, treasury_id: AccountId) {
        assert_one_yocto();
        self.assert_owner();
        self.treasury_id = treasury_id;
    }

    #[payable]
    pub fn set_transaction_fee(&mut self, fee: u16) {
        assert_one_yocto();
        self.assert_owner();
        self.transaction_fee = fee;
    }

    #[payable]
    pub fn transfer_ownership(&mut self, owner_id: AccountId) {
        assert_one_yocto();
        self.assert_owner();
        self.owner_id = owner_id;
    }
    // Approved contracts
    #[payable]
    pub fn add_approved_nft_contract_ids(&mut self, nft_contract_ids: Vec<AccountId>) {
        assert_one_yocto();
        self.assert_owner();
        add_accounts(Some(nft_contract_ids), &mut self.approved_nft_contract_ids);
    }

    #[payable]
    pub fn remove_approved_nft_contract_ids(&mut self, nft_contract_ids: Vec<AccountId>) {
        assert_one_yocto();
        self.assert_owner();
        remove_accounts(Some(nft_contract_ids), &mut self.approved_nft_contract_ids);
    }

    pub fn get_config(&self) -> MarketplaceConfig {
        MarketplaceConfig {
            owner_id: self.owner_id.clone(),
            treasury_id: self.treasury_id.clone(),
            transaction_fee: self.transaction_fee
        }
    }

    pub fn approved_nft_contract_ids(&self) -> Vec<AccountId> {
        self.approved_nft_contract_ids.to_vec()
    }

    pub fn get_market_data(self, nft_contract_id: AccountId, token_id: TokenId) -> MarketDataJson {
        let contract_and_token_id = format!("{}{}{}", nft_contract_id, DELIMETER, token_id);
        let market_data = self.market.get(&contract_and_token_id).expect("DS: Market data does not exist");

        let mut price = market_data.price;

        if market_data.is_auction.is_some() && market_data.end_price.is_some() {
            let current_time = env::block_timestamp();
            let end_price = market_data.end_price.unwrap();
            let started_at = market_data.started_at.unwrap();
            let ended_at = market_data.ended_at.unwrap();

            if current_time < started_at {
                // Use current market_data.price
            } else if current_time > ended_at {
                price = end_price;
            } else {
                let time_since_start = current_time - started_at;
                let duration = ended_at - started_at;
                price = price - ((price - end_price) / duration as u128) * time_since_start as u128;
            }
        }

        MarketDataJson {
            owner_id: market_data.owner_id,
            approval_id: market_data.approval_id.into(),
            nft_contract_id: market_data.nft_contract_id,
            token_id: market_data.token_id,
            price: price.into(),
            bids: market_data.bids,
            started_at: market_data.started_at.map(|x| x.into()),
            ended_at: market_data.ended_at.map(|x| x.into()),
            end_price: market_data.end_price.map(|x| x.into()),
            is_auction: market_data.is_auction,
        }
    }

    fn assert_owner(&self) {
        assert_eq!(
            env::predecessor_account_id(),
            self.owner_id,
            "DS: Owner only"
        )
    }
}

fn add_accounts(accounts: Option<Vec<AccountId>>, set: &mut UnorderedSet<AccountId>) {
    accounts.map(|ids| {
        ids.iter().for_each(|id| {
            set.insert(id);
        })
    });
}
fn remove_accounts(accounts: Option<Vec<AccountId>>, set: &mut UnorderedSet<AccountId>) {
    accounts.map(|ids| {
        ids.iter().for_each(|id| {
            set.remove(id);
        })
    });
}
pub fn hash_account_id(account_id: &AccountId) -> CryptoHash {
    let mut hash = CryptoHash::default();
    hash.copy_from_slice(&env::sha256(account_id.as_bytes()));
    hash
}
