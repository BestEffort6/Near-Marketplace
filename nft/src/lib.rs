/*!
Non-Fungible Token implementation with JSON serialization.
NOTES:
  - The maximum balance value is limited by U128 (2**128 - 1).
  - JSON calls should pass U128 as a base-10 string. E.g. "100".
  - The contract optimizes the inner trie structure by hashing account IDs. It will prevent some
    abuse of deep tries. Shouldn't be an issue, once NEAR clients implement full hashing of keys.
  - The contract tracks the change in storage before and after the call. If the storage increases,
    the contract requires the caller of the contract to attach enough deposit to the function call
    to cover the storage cost.
    This is done to prevent a denial of service attack on the contract by taking all available storage.
    If the storage decreases, the contract will issue a refund for the cost of the released storage.
    The unused tokens from the attached deposit are also refunded, so it's safe to
    attach more deposit than required.
  - To prevent the deployed contract from being modified or deleted, it should not have any access
    keys on its account.
*/
use near_contract_standards::non_fungible_token::approval::NonFungibleTokenApproval;
use near_contract_standards::non_fungible_token::core::{
    NonFungibleTokenCore, NonFungibleTokenResolver,
};
use near_contract_standards::non_fungible_token::enumeration::NonFungibleTokenEnumeration;
use near_contract_standards::non_fungible_token::metadata::{
    NFTContractMetadata, NonFungibleTokenMetadataProvider, TokenMetadata,
};
use near_contract_standards::non_fungible_token::NonFungibleToken;
use near_contract_standards::non_fungible_token::events::NftMint;
use near_contract_standards::non_fungible_token::{Token, TokenId};
use near_contract_standards::fungible_token::{receiver, Balance};
use near_sdk::assert_one_yocto;
use near_sdk::serde::{Serialize, Deserialize};
use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap, UnorderedSet};
use near_sdk::json_types::U128;
use near_sdk::{
    env, near_bindgen, require, AccountId, BorshStorageKey, PanicOnDefault, Promise, PromiseOrValue, NearToken, Gas, 
    serde_json::json,
};
use std::collections::HashMap;

mod ft_balances;

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Payout {
    pub payout: HashMap<AccountId, U128>,
}


#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
pub struct Contract {
    pub tokens: NonFungibleToken,

    pub metadata: LazyOption<NFTContractMetadata>,

    pub index: u128,

    pub total_supply: u128,

    pub mint_price: u128,
    
    //which fungible token can be used to purchase NFTs
    pub mint_currency: Option<AccountId>, 
    
    pub payment_split_percent: u128,

    //keep track of the storage that accounts have payed
    pub storage_deposits: LookupMap<AccountId, u128>,

    //keep track of how many FTs each account has deposited in order to purchase NFTs with
    pub ft_deposits: LookupMap<AccountId, Balance>,

    pub burn_fee: u128,

    pub balances_by_owner: LookupMap<AccountId, Balance>,

    pub holders: UnorderedSet<AccountId>,

    pub treasury: AccountId,

    pub royalty: u128
}

const NEAR_PER_STORAGE: u128 = 10_000_000_000_000_000_000;
//the minimum storage to have a sale on the contract.
const STORAGE_PER_SALE: u128 = 1000 * NEAR_PER_STORAGE;
const VAULT_STORAGE: u128 = 19_800_000_000_000_000_000_000;

#[derive(BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    NonFungibleToken,
    Metadata,
    TokenMetadata,
    Enumeration,
    Approval,
    StorageDeposits,
    FTDeposits,
    BalancesByOwner,
    Holders,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        owner_id: AccountId, 
        metadata: NFTContractMetadata,
        mint_price: U128,
        mint_currency: Option<AccountId>,
        payment_split_percent: U128,
        total_supply: U128,
        burn_fee: U128,
        treasury: AccountId,
        royalty: U128
    ) -> Self {
        require!(!env::state_exists(), "Already initialized");
        metadata.assert_valid();
        Self {
            tokens: NonFungibleToken::new(
                StorageKey::NonFungibleToken,
                owner_id,
                Some(StorageKey::TokenMetadata),
                Some(StorageKey::Enumeration),
                Some(StorageKey::Approval),
            ),
            metadata: LazyOption::new(StorageKey::Metadata, Some(&metadata)),
            index: 0,
            total_supply: total_supply.0,
            mint_price: mint_price.0,
            mint_currency,
            payment_split_percent: payment_split_percent.0,
            storage_deposits: LookupMap::new(StorageKey::StorageDeposits),
            ft_deposits: LookupMap::new(StorageKey::FTDeposits),
            burn_fee: burn_fee.0,
            balances_by_owner: LookupMap::new(StorageKey::BalancesByOwner),
            holders: UnorderedSet::new(StorageKey::Holders),
            treasury: treasury,
            royalty: royalty.0
        }
    }

    /// Mint a new token with ID=`token_id` belonging to `token_owner_id`.
    ///
    /// Since this example implements metadata, it also requires per-token metadata to be provided
    /// in this call. `self.tokens.mint` will also require it to be Some, since
    /// `StorageKey::TokenMetadata` was provided at initialization.
    ///
    /// `self.tokens.mint` will enforce `predecessor_account_id` to equal the `owner_id` given in
    /// initialization call to `new`.
    #[payable]
    pub fn nft_mint(
        &mut self,
        token_id: TokenId,
        token_owner_id: AccountId,
        token_metadata: TokenMetadata,
    ) -> Token {
        let collection_owner = &self.tokens.owner_id;
        let owner = env::predecessor_account_id(); 
        self.holders.insert(&owner);
        // assert_eq!(owner, self.tokens.owner_id, "Unauthorized");

        let code = include_bytes!("./vault/vault.wasm").to_vec();
        let contract_bytes = code.len() as u128;
        let minimum_needed = NEAR_PER_STORAGE * contract_bytes + VAULT_STORAGE;

        let deposit: u128 = env::attached_deposit().as_yoctonear();
        if let Some(_) = self.mint_currency.clone() {
            let amount = self.ft_deposits_of(owner.clone());
            require!(deposit >= minimum_needed && amount >= self.mint_price, "Insufficient price to mint");
        } else {
            require!(deposit >= self.mint_price + minimum_needed, "Insufficient price to mint");
        }

        let current_id = env::current_account_id();

        let vault_amount = self.mint_price.checked_mul(self.payment_split_percent)
            .unwrap().checked_div(100u128).unwrap();

        let owner_amount = self.mint_price.checked_sub(vault_amount).unwrap();

        // Deploy the vault contract
        let vault_account_id: AccountId = format!("{}.{}", token_id, current_id).parse().unwrap();
        Promise::new(vault_account_id.clone())
            .create_account()
            .deploy_contract(code)
            .transfer(NearToken::from_yoctonear(minimum_needed))
            .function_call(
                // Init the vault contract
                "init".to_string(),
                if let Some(ft_id) = self.mint_currency.clone() {
                    json!({
                        "ft_contract": ft_id.to_string(),
                        "treasury": self.treasury.to_string()
                    })
                } else {
                    json!({
                        "treasury": self.treasury.to_string()
                    })
                }.to_string().into_bytes().to_vec(),
                NearToken::from_millinear(0),
                Gas::from_tgas(20)
            )
            .then(
                Self::ext(env::current_account_id())
                .with_static_gas(Gas::from_tgas(150))
                .resolve_create(
                    vault_account_id,
                    collection_owner,
                    owner_amount,
                    vault_amount
                )
            );
        self.index = self.index.checked_add(1).unwrap();
        if self.total_supply > 0 {
            require!(self.total_supply >= self.index, "Exceeded total supply");
        }

        let token = self.tokens.internal_mint_with_refund(token_id, token_owner_id, Some(token_metadata), None);
        NftMint { owner_id: &token.owner_id, token_ids: &[&token.token_id], memo: None }.emit();
        token
    }
    #[private]
    pub fn resolve_create(
        &mut self,
        vault_account_id:AccountId,
        collection_owner:&AccountId,
        owner_amount: u128,
        vault_amount: u128
    ) -> Promise {
        // Deposit ft or near
        if let Some(ft_id) = self.mint_currency.clone() {
            Promise::new(ft_id.clone()).function_call(
                "storage_deposit".to_string(), 
                json!({
                    "account_id": vault_account_id.to_string()
                }).to_string().into_bytes().to_vec(),
                NearToken::from_millinear(100), 
                Gas::from_tgas(20)
            );
            Promise::new(ft_id.clone()).function_call(
                "ft_transfer_call".to_string(), 
                json!({
                    "receiver_id": vault_account_id.to_string(),
                    "amount": vault_amount.to_string(),
                    "msg": "",
                }).to_string().into_bytes().to_vec(),
                NearToken::from_yoctonear(1),
                Gas::from_tgas(50),
            );
            Promise::new(ft_id.clone()).function_call(
                "ft_transfer".to_string(), 
                json!({
                    "receiver_id": collection_owner.clone().to_string(),
                    "amount": owner_amount.to_string(),
                    "msg": "",
                }).to_string().into_bytes().to_vec(),
                NearToken::from_yoctonear(1),
                Gas::from_tgas(50),
            )
        } else {
            Promise::new(collection_owner.clone()).transfer(NearToken::from_yoctonear(owner_amount));
            Promise::new(vault_account_id.clone()).function_call(
                "deposit_near".to_string(),
                json!({}).to_string().into_bytes().to_vec(),
                NearToken::from_yoctonear(vault_amount),
                Gas::from_tgas(20),
            )
        }
    }
    //Allows users to deposit storage. This is to cover the cost of storing sale objects on the contract
    //Optional account ID is to users can pay for storage for other people.
    #[payable]
    pub fn storage_deposit(&mut self, account_id: Option<AccountId>) {
        //get the account ID to pay for storage for
        let storage_account_id = account_id 
            //convert the valid account ID into an account ID
            .map(|a| a.into())
            //if we didn't specify an account ID, we simply use the caller of the function
            .unwrap_or_else(env::predecessor_account_id);

        //get the deposit value which is how much the user wants to add to their storage
        let deposit: u128 = env::attached_deposit().as_yoctonear();

        //make sure the deposit is greater than or equal to the minimum storage for a sale
        assert!(
            deposit >= STORAGE_PER_SALE,
            "Requires minimum deposit of {}",
            STORAGE_PER_SALE
        );

        //get the balance of the account (if the account isn't in the map we default to a balance of 0)
        let mut balance: u128 = self.storage_deposits.get(&storage_account_id).unwrap_or(0);
        //add the deposit to their balance
        balance += deposit;
        //insert the balance back into the map for that account ID
        self.storage_deposits.insert(&storage_account_id, &balance);
    }

    // Burn an NFT by its token ID
    #[payable]
    pub fn burn(&mut self, token_id: TokenId) {
        let owner = env::predecessor_account_id();

        let token_owner = self.tokens.owner_by_id.get(&token_id).unwrap();

        assert_eq!(owner.clone(), token_owner, "You don't own this NFT");

        // Remove the NFT from the owner's account
        self.tokens.owner_by_id.remove(&token_id);

        // Remove token metadata (if applicable)
        self.tokens
            .token_metadata_by_id
            .as_mut()
            .and_then(|by_id| by_id.remove(&token_id));
        
        // Remove the NFT from the tokens_per_owner map
        let mut removed = false;
        if let Some(tokens_per_owner) = &mut self.tokens.tokens_per_owner {
            let mut owner_tokens = tokens_per_owner.get(&owner).unwrap_or_else(|| {
                env::panic_str("Unable to access tokens per owner in unguarded call.")
            });
            owner_tokens.remove(&token_id);
            if owner_tokens.is_empty() {
                tokens_per_owner.remove(&owner);
                self.holders.remove(&owner);
                removed = true;
            } else {
                tokens_per_owner.insert(&owner, &owner_tokens);
            }
        }
        
        // Remove any approvals associated with this NFT
        self.tokens
            .approvals_by_id
            .as_mut()
            .and_then(|by_id| by_id.remove(&token_id.clone()));

        // Remove next approval ID (if applicable)
        self.tokens
            .next_approval_id_by_id
            .as_mut()
            .and_then(|by_id| by_id.remove(&token_id.clone()));

        // Update Balance for holders
        let mut holders_count: u128 = self.holders.len() as u128;
        if removed == false {
            holders_count -= 1;
        }
        let amount_to_holder: u128 = if holders_count == 0 {
            0u128
        } else { 
            self.mint_price
                .checked_mul(self.payment_split_percent).unwrap()
                .checked_mul(self.burn_fee).unwrap()
                .checked_div(20000u128).unwrap()
                .checked_div(holders_count).unwrap()
        };

        env::log_str(&format!("Total holders count: {}", holders_count));
        env::log_str(&format!("Amount to each holder: {}", amount_to_holder));

        for other in self.holders.iter() {
            if other != owner {
                let mut balance = self.balances_by_owner.get(&other).unwrap_or(0);
                balance = balance.checked_add(amount_to_holder).unwrap();
                self.balances_by_owner.insert(&other, &balance);
            }
        }

        let current_id = env::current_account_id();
        let vault_account_id: AccountId = format!("{}.{}", token_id, current_id).parse().unwrap();

        Promise::new(vault_account_id.clone()).function_call(
            "withdraw".to_string(),
            json!({
                "owner": owner.to_string(),
                "burn_fee": self.burn_fee.to_string(),
            }).to_string().into_bytes().to_vec(),
            NearToken::from_yoctonear(1),
            Gas::from_tgas(100)
        );
    }

    #[payable]
    pub fn withdraw(&mut self) {
        let owner = env::predecessor_account_id();
        let balance: u128 = self.balances_by_owner.get(&owner).unwrap_or(0);

        if balance > 0 {
            // Deposit ft or near
            if let Some(ft_id) = self.mint_currency.clone() {
                Promise::new(ft_id.clone()).function_call(
                    "ft_transfer".to_string(), 
                    json!({
                        "receiver_id": owner.to_string(),
                        "amount": balance.to_string(),
                    }).to_string().into_bytes().to_vec(),
                    NearToken::from_yoctonear(1),
                    Gas::from_tgas(20),
                );
            } else {
                Promise::new(owner.clone()).transfer(NearToken::from_yoctonear(balance));
            }

            self.balances_by_owner.insert(&owner, &0u128).unwrap();
        }
    }

    #[payable]
    pub fn nft_transfer_payout(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        balance: Option<U128>
    ) -> Option<Payout> {
        assert_one_yocto();
        let previous_owner_id =
            self.tokens.owner_by_id.get(&token_id).unwrap_or_else(|| env::panic_str("Token not found"));
        if let Some(tokens_per_owner) = &mut self.tokens.tokens_per_owner {
            let sender_tokens = tokens_per_owner.get(&previous_owner_id).unwrap_or_else(|| {
                env::panic_str("Unable to access tokens per owner in unguarded call.")
            });
            if sender_tokens.len()==1 {
                self.holders.remove(&previous_owner_id);
            };
            let receiver_tokens = tokens_per_owner.get(&receiver_id);
            if receiver_tokens.is_none() {
                self.holders.insert(&receiver_id);
            } else {
                let receiver_tokens = receiver_tokens.unwrap();
                if receiver_tokens.len() == 0 {
                    self.holders.insert(&receiver_id);
                }
            }
        }
        self.tokens.nft_transfer(receiver_id, token_id, approval_id, None);

        let payout = if let Some(balance) = balance {
            let balance_u128: u128 = u128::from(balance);
            let mut payout: Payout = Payout {
                payout: HashMap::new(),
            };
            payout.payout.insert(self.tokens.owner_id.clone(), royalty_to_payout(self.royalty, balance_u128));
            payout.payout.insert(previous_owner_id, royalty_to_payout(10000-self.royalty, balance_u128));
            Some(payout)
        } else {
            None
        };
        payout
    }
    //return how much storage an account has paid for
    pub fn storage_balance_of(&self, account_id: AccountId) -> U128 {
        U128(self.storage_deposits.get(&account_id).unwrap_or(0))
    }

    /// Get the amount of FTs the user has deposited into the contract
    pub fn ft_deposits_of(
        &self,
        account_id: AccountId
    ) -> u128 {
        self.ft_deposits.get(&account_id).unwrap_or(0)
    }

    pub fn index(&self) -> u128 {
        self.index
    }

    pub fn total_supply(&self) -> u128 {
        self.total_supply
    }

    pub fn balance_of(&self, owner: AccountId) -> u128 {
        self.balances_by_owner.get(&owner).unwrap_or(0)
    }

    pub fn total_holders(&self) -> u64 {
        self.holders.len()
    }
}

#[near_bindgen]
impl NonFungibleTokenCore for Contract {
    #[payable]
    fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        let owner_id =
            self.tokens.owner_by_id.get(&token_id).unwrap_or_else(|| env::panic_str("Token not found"));
        if let Some(tokens_per_owner) = &mut self.tokens.tokens_per_owner {
            let sender_tokens = tokens_per_owner.get(&owner_id).unwrap_or_else(|| {
                env::panic_str("Unable to access tokens per owner in unguarded call.")
            });
            if sender_tokens.len()==1 {
                self.holders.remove(&owner_id);
            };
            let receiver_tokens = tokens_per_owner.get(&receiver_id);
            if receiver_tokens.is_none() {
                self.holders.insert(&receiver_id);
            } else {
                let receiver_tokens = receiver_tokens.unwrap();
                if receiver_tokens.len() == 0 {
                    self.holders.insert(&receiver_id);
                }
            }
        }
        self.tokens.nft_transfer(receiver_id, token_id, approval_id, memo);
    }

    #[payable]
    fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        let owner_id =
            self.tokens.owner_by_id.get(&token_id).unwrap_or_else(|| env::panic_str("Token not found"));
        if let Some(tokens_per_owner) = &mut self.tokens.tokens_per_owner {
            let sender_tokens = tokens_per_owner.get(&owner_id).unwrap_or_else(|| {
                env::panic_str("Unable to access tokens per owner in unguarded call.")
            });
            if sender_tokens.len()==1 {
                self.holders.remove(&owner_id);
            };
            let receiver_tokens = tokens_per_owner.get(&receiver_id);
            if receiver_tokens.is_none() {
                self.holders.insert(&receiver_id);
            } else {
                let receiver_tokens = receiver_tokens.unwrap();
                if receiver_tokens.len() == 0 {
                    self.holders.insert(&receiver_id);
                }
            }
        }
        self.tokens.nft_transfer_call(receiver_id, token_id, approval_id, memo, msg)
    }

    fn nft_token(&self, token_id: TokenId) -> Option<Token> {
        self.tokens.nft_token(token_id)
    }
}

fn royalty_to_payout(a: u128, b: Balance) -> U128 {
    U128(a as u128 * b / 10_000u128)
}

#[near_bindgen]
impl NonFungibleTokenResolver for Contract {
    #[private]
    fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: Option<HashMap<AccountId, u64>>,
    ) -> bool {
        self.tokens.nft_resolve_transfer(
            previous_owner_id,
            receiver_id,
            token_id,
            approved_account_ids,
        )
    }
}

#[near_bindgen]
impl NonFungibleTokenApproval for Contract {
    #[payable]
    fn nft_approve(
        &mut self,
        token_id: TokenId,
        account_id: AccountId,
        msg: Option<String>,
    ) -> Option<Promise> {
        self.tokens.nft_approve(token_id, account_id, msg)
    }

    #[payable]
    fn nft_revoke(&mut self, token_id: TokenId, account_id: AccountId) {
        self.tokens.nft_revoke(token_id, account_id);
    }

    #[payable]
    fn nft_revoke_all(&mut self, token_id: TokenId) {
        self.tokens.nft_revoke_all(token_id);
    }

    fn nft_is_approved(
        &self,
        token_id: TokenId,
        approved_account_id: AccountId,
        approval_id: Option<u64>,
    ) -> bool {
        self.tokens.nft_is_approved(token_id, approved_account_id, approval_id)
    }
}

#[near_bindgen]
impl NonFungibleTokenEnumeration for Contract {
    fn nft_total_supply(&self) -> U128 {
        self.tokens.nft_total_supply()
    }

    fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<Token> {
        self.tokens.nft_tokens(from_index, limit)
    }

    fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        self.tokens.nft_supply_for_owner(account_id)
    }

    fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<Token> {
        self.tokens.nft_tokens_for_owner(account_id, from_index, limit)
    }
}

#[near_bindgen]
impl NonFungibleTokenMetadataProvider for Contract {
    fn nft_metadata(&self) -> NFTContractMetadata {
        self.metadata.get().unwrap()
    }
}
