use crate::*;
/// approval callbacks from NFT Contracts
#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct MarketArgs {
    pub price: Option<U128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<U64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<U64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_auction: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_price: Option<U128>,
}

trait NonFungibleTokenApprovalsReceiver {
    fn nft_on_approve(
        &mut self,
        token_id: TokenId,
        owner_id: AccountId,
        approval_id: u64,
        msg: String,
    );
}

#[near_bindgen]
impl NonFungibleTokenApprovalsReceiver for Marketplace {
    fn nft_on_approve(
        &mut self,
        token_id: TokenId,
        owner_id: AccountId,
        approval_id: u64,
        msg: String,
    ) {
        // enforce cross contract call and owner_id is signer

        let nft_contract_id = env::predecessor_account_id();
        let signer_id = env::signer_account_id();
        assert_ne!(
            env::current_account_id(),
            nft_contract_id,
            "DS: nft_on_approve should only be called via cross-contract call"
        );
        assert_eq!(owner_id, signer_id, "DS: owner_id should be signer_id");

        assert!(
            self.approved_nft_contract_ids.contains(&nft_contract_id),
            "DS: nft_contract_id is not approved"
        );

        let MarketArgs {
            price,
            started_at,
            ended_at,
            is_auction,
            end_price,
        } = near_sdk::serde_json::from_str(&msg).expect("Not valid MarketArgs");

        assert!(price.is_some(), "DS: price not specified");

        let storage_amount = self.storage_minimum_balance().0;
        let owner_paid_storage = self.storage_deposits.get(&signer_id).unwrap_or(0);
        let signer_storage_required =
            (self.get_supply_by_owner_id(signer_id).0 + 1) as u128 * storage_amount;

        if owner_paid_storage < signer_storage_required {
            let notif = format!(
                "Insufficient storage paid: {}, for {} sales at {} rate of per sale",
                owner_paid_storage,
                signer_storage_required / storage_amount,
                storage_amount
            );
            env::log_str(&notif);
            return;
        }
        self.internal_add_market_data(
            owner_id,
            approval_id,
            nft_contract_id,
            token_id,
            price.unwrap(),
            started_at,
            ended_at,
            end_price,
            is_auction,
        );
    }
}
