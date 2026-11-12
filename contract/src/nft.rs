use crate::{Agent, Contract, ContractExt, emit, view};
use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, json};
use near_sdk::{AccountId, assert_one_yocto, env, log, near, require};
use std::collections::HashMap;

#[near(serializers = [json])]
pub struct NftContractMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
}

#[near(serializers = [json])]
pub struct TokenMetadata {
    pub title: String,
    pub description: String,
    pub media: Option<String>,
    pub extra: String,
}

#[near(serializers = [json])]
pub struct JsonToken {
    pub token_id: String,
    pub owner_id: AccountId,
    pub metadata: TokenMetadata,
}

#[near(serializers = [json])]
pub struct Payout {
    pub payout: HashMap<AccountId, U128>,
}

fn token(id: &str, a: &Agent) -> JsonToken {
    JsonToken {
        token_id: id.to_owned(),
        owner_id: a.owner.clone(),
        metadata: TokenMetadata {
            title: format!("Agent #{id}"),
            description: "AI trading agent that owns a Hyperliquid account. The image is a live card of its price, account value and position.".into(),
            media: crate::stored_media_base().map(|base| format!("{base}/nft/{id}.svg")),
            extra: serde_json::to_string(&view(id, a)).unwrap_or_default(),
        },
    }
}

#[near]
impl Contract {
    pub fn nft_metadata(&self) -> NftContractMetadata {
        NftContractMetadata { spec: "nft-1.0.0".into(), name: "Inferents Agents".into(), symbol: "AGENT".into() }
    }

    pub fn nft_total_supply(&self) -> U128 {
        U128(self.agents.len() as u128)
    }

    pub fn nft_token(&self, token_id: String) -> Option<JsonToken> {
        self.agents.get(&token_id).map(|a| token(&token_id, a))
    }

    pub fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<JsonToken> {
        self.agents
            .iter()
            .skip(from_index.map_or(0, |i| i.0 as usize))
            .take(limit.unwrap_or(50) as usize)
            .map(|(id, a)| token(id, a))
            .collect()
    }

    // ponytail: linear scan over all agents, add an owner index if supply grows past a few thousand
    pub fn nft_tokens_for_owner(&self, account_id: AccountId, from_index: Option<U128>, limit: Option<u64>) -> Vec<JsonToken> {
        self.agents
            .iter()
            .filter(|(_, a)| a.owner == account_id)
            .skip(from_index.map_or(0, |i| i.0 as usize))
            .take(limit.unwrap_or(50) as usize)
            .map(|(id, a)| token(id, a))
            .collect()
    }

    pub fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        U128(self.agents.values().filter(|a| a.owner == account_id).count() as u128)
    }

    // ponytail: no NEP-178 approvals or nft_transfer_call; marketplaces that need them are out of scope
    #[payable]
    pub fn nft_transfer(&mut self, receiver_id: AccountId, token_id: String, approval_id: Option<u64>, memo: Option<String>) {
        assert_one_yocto();
        require!(approval_id.is_none(), "approvals not supported");
        let _ = memo;
        self.transfer(receiver_id, &token_id);
    }

    pub fn nft_payout(&self, token_id: String, balance: U128, max_len_payout: Option<u32>) -> Payout {
        let a = self.agents.get(&token_id).unwrap_or_else(|| env::panic_str("no such agent"));
        let royalty = balance.0 * self.royalty_bps as u128 / 10_000;
        let mut payout: HashMap<AccountId, U128> = HashMap::new();
        payout.insert(a.minter.clone(), U128(royalty));
        let seller = payout.entry(a.owner.clone()).or_insert(U128(0));
        seller.0 += balance.0 - royalty;
        require!(max_len_payout.is_none_or(|m| payout.len() <= m as usize), "payout longer than max_len_payout");
        Payout { payout }
    }

    #[payable]
    pub fn nft_transfer_payout(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
        balance: U128,
        max_len_payout: Option<u32>,
    ) -> Payout {
        let payout = self.nft_payout(token_id.clone(), balance, max_len_payout);
        self.nft_transfer(receiver_id, token_id, approval_id, memo);
        payout
    }
}

impl Contract {
    fn transfer(&mut self, receiver_id: AccountId, token_id: &str) {
        let sender = env::predecessor_account_id();
        let a = self.agent_mut(token_id);
        require!(a.owner == sender, "NFT owner only");
        require!(a.owner != receiver_id, "receiver is already the owner");
        self.move_nft(receiver_id, token_id);
    }

    pub(crate) fn move_nft(&mut self, receiver_id: AccountId, token_id: &str) {
        let a = self.agent_mut(token_id);
        let old_owner = std::mem::replace(&mut a.owner, receiver_id.clone());
        a.needs_rotation = true;
        log!(
            "EVENT_JSON:{}",
            json!({"standard": "nep171", "version": "1.2.0", "event": "nft_transfer",
                   "data": [{"old_owner_id": old_owner, "new_owner_id": receiver_id, "token_ids": [token_id]}]})
        );
        emit("rotation_required", json!({"token_id": token_id}));
    }
}
