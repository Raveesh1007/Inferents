mod hl;
mod nft;
mod receipt;

use near_sdk::json_types::{I64, U64, U128};
use near_sdk::serde_json::{self, Value, json};
use near_sdk::store::{IterableMap, LookupMap, LookupSet, Vector};
use near_sdk::{
    AccountId, Gas, NearToken, Promise, PromiseError, PromiseOrValue, env, log, near, require,
};
use receipt::{Receipt, ReceiptKind, recover_signer};

// Raw storage key, outside the per-model map, so the contract state layout is unchanged.
const GATEWAY_KEY: &[u8] = b"gateway";
const MEDIA_KEY: &[u8] = b"media_base";

pub(crate) fn stored_media_base() -> Option<String> {
    env::storage_read(MEDIA_KEY).and_then(|b| String::from_utf8(b).ok())
}

pub const MAX_COST_PER_REQUEST_MICRO_USD: u64 = 1_000_000;
const DAY_NS: u64 = 86_400_000_000_000;
const EQUITY_MAX_AGE_NS: u64 = 120_000_000_000;

// Share of a mint deposit kept as thinking credits; the rest becomes trading margin at once.
fn default_credits_bps() -> u16 {
    1_000
}

pub fn agent_path(token_id: &str) -> String {
    format!("agent-{token_id}")
}

fn emit(event: &str, data: Value) {
    log!(
        "EVENT_JSON:{}",
        json!({"standard": "inferents", "version": "1.0.0", "event": event, "data": [data]})
    );
}

#[near(serializers = [json])]
pub struct AffinePoint {
    pub affine_point: String,
}

#[near(serializers = [json])]
pub struct Scalar {
    pub scalar: String,
}

#[near(serializers = [json])]
pub struct MpcSignature {
    pub big_r: AffinePoint,
    pub s: Scalar,
    pub recovery_id: u8,
}

#[near(serializers = [json])]
pub struct HlSignature {
    pub r: String,
    pub s: String,
    pub v: u8,
}

#[near(serializers = [json])]
pub struct SignedAction {
    pub action: Value,
    pub nonce: u64,
    pub signature: HlSignature,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RevenueTarget {
    Credits,
    Margin,
}

#[near(serializers = [borsh])]
pub struct OldAgent {
    pub minter: AccountId,
    pub owner: AccountId,
    pub credits: u64,
    pub margin: u64,
    pub epoch_len_ns: u64,
    pub epoch_end_ns: u64,
    pub hl_address: String,
    pub agent_address: Option<String>,
    pub needs_rotation: bool,
    pub max_leverage: u8,
    pub flat_at_epoch_end: bool,
    pub pnl: i64,
    pub key_price: u64,
    pub key_duration_ns: u64,
    pub key_revenue_to: RevenueTarget,
}

#[near(serializers = [borsh])]
pub struct Agent {
    pub minter: AccountId,
    pub owner: AccountId,
    pub credits: u64,
    pub margin: u64,
    pub epoch_len_ns: u64,
    pub epoch_end_ns: u64,
    pub hl_address: String,
    pub agent_address: Option<String>,
    pub needs_rotation: bool,
    pub max_leverage: u8,
    pub flat_at_epoch_end: bool,
    // Realised P&L reported by the runner; unrealised profit is already in `equity`.
    pub pnl: i64,
    pub key_price: u64,
    pub key_duration_ns: u64,
    pub key_revenue_to: RevenueTarget,
    pub strategy_hash: [u8; 32],
    pub model: String,
    pub acquisition_budget: u64,
    pub acquired_by: Option<String>,
    pub equity: u64,
    pub equity_ts_ns: u64,
    pub spent: u64,
}

#[near(serializers = [json])]
pub struct AgentView {
    pub token_id: String,
    pub minter: AccountId,
    pub owner: AccountId,
    pub credits_micro_usd: U64,
    pub margin_micro_usd: U64,
    pub epoch_len_ns: U64,
    pub epoch_end_ns: U64,
    pub hl_address: String,
    pub agent_address: Option<String>,
    pub needs_rotation: bool,
    pub max_leverage: u8,
    pub flat_at_epoch_end: bool,
    pub pnl_micro_usd: I64,
    pub key_price_usdc: U64,
    pub key_duration_ns: U64,
    pub key_revenue_to: RevenueTarget,
    pub strategy_hash: String,
    pub model: String,
    pub acquisition_budget_micro_usd: U64,
    pub acquired_by: Option<String>,
    pub equity_micro_usd: U64,
    pub equity_ts_ns: U64,
    pub spent_micro_usd: U64,
}

#[near(serializers = [json])]
pub struct Accounting {
    pub reserve_usdc: U128,
    pub treasury_usdc: U128,
    pub total_credits: U128,
    pub refundable_usdc: U128,
}

#[near(serializers = [json])]
#[serde(tag = "action", rename_all = "snake_case")]
enum Action {
    Mint {
        owner: AccountId,
        max_leverage: u8,
        #[serde(default = "default_credits_bps")]
        credits_bps: u16,
        model: String,
        #[serde(default)]
        strategy_hash: String,
    },
    Load { owner: AccountId, token_id: String },
    BuyKey { owner: AccountId, token_id: String },
    Buy { owner: AccountId, token_id: String },
}

impl Action {
    fn owner(&self) -> &AccountId {
        match self {
            Action::Mint { owner, .. } | Action::Load { owner, .. } | Action::BuyKey { owner, .. } | Action::Buy { owner, .. } => owner,
        }
    }
}

#[near(serializers = [borsh])]
struct OldContract {
    owner: AccountId,
    mpc: AccountId,
    hl_chain: String,
    runner: AccountId,
    signing_addresses: LookupMap<String, [u8; 20]>,
    receipts: Vector<Receipt>,
    usdc: AccountId,
    royalty_bps: u16,
    min_mint: u64,
    default_epoch_ns: u64,
    next_id: u64,
    agents: IterableMap<String, OldAgent>,
    access: LookupMap<(String, AccountId), u64>,
    seen_requests: LookupSet<String>,
    reserve: u128,
    treasury: u128,
}

#[near(contract_state)]
pub struct Contract {
    owner: AccountId,
    mpc: AccountId,
    hl_chain: String,
    runner: AccountId,
    signing_addresses: LookupMap<String, [u8; 20]>,
    receipts: Vector<Receipt>,
    usdc: AccountId,
    royalty_bps: u16,
    min_mint: u64,
    default_epoch_ns: u64,
    next_id: u64,
    agents: IterableMap<String, Agent>,
    access: LookupMap<(String, AccountId), u64>,
    seen_requests: LookupSet<String>,
    reserve: u128,
    treasury: u128,
    gateway_models: LookupSet<String>,
    balances: LookupMap<AccountId, u128>,
    refundable: u128,
    premium_bps: u16,
}

impl Default for Contract {
    fn default() -> Self {
        env::panic_str("not initialized")
    }
}

#[near]
impl Contract {
    #[init]
    pub fn new(
        owner_id: AccountId,
        usdc_token: AccountId,
        mpc_contract: AccountId,
        runner: AccountId,
        royalty_bps: u16,
        hl_mainnet: bool,
    ) -> Self {
        Self::build(owner_id, mpc_contract, hl_mainnet, runner, usdc_token, royalty_bps, LookupMap::new(b"s"), Vector::new(b"r"))
    }

    #[private]
    #[init(ignore_state)]
    pub fn migrate() -> Self {
        let old: OldContract = env::state_read().unwrap_or_else(|| env::panic_str("no old state"));
        let mut agents = IterableMap::new(b"b");
        for (id, a) in old.agents.iter() {
            agents.insert(
                id.clone(),
                Agent {
                    minter: a.minter.clone(),
                    owner: a.owner.clone(),
                    credits: a.credits,
                    margin: a.margin,
                    epoch_len_ns: a.epoch_len_ns,
                    epoch_end_ns: a.epoch_end_ns,
                    hl_address: a.hl_address.clone(),
                    agent_address: a.agent_address.clone(),
                    needs_rotation: a.needs_rotation,
                    max_leverage: a.max_leverage,
                    flat_at_epoch_end: a.flat_at_epoch_end,
                    pnl: a.pnl,
                    key_price: a.key_price,
                    key_duration_ns: a.key_duration_ns,
                    key_revenue_to: a.key_revenue_to,
                    strategy_hash: [0; 32],
                    model: "deepseek-ai/DeepSeek-V4.1-Flash-1017".into(),
                    acquisition_budget: 0,
                    acquired_by: None,
                    equity: 0,
                    equity_ts_ns: 0,
                    spent: 0,
                },
            );
        }
        Self {
            owner: old.owner,
            mpc: old.mpc,
            hl_chain: old.hl_chain,
            runner: old.runner,
            signing_addresses: old.signing_addresses,
            receipts: old.receipts,
            usdc: old.usdc,
            royalty_bps: old.royalty_bps,
            min_mint: old.min_mint,
            default_epoch_ns: old.default_epoch_ns,
            next_id: old.next_id,
            agents,
            access: old.access,
            seen_requests: old.seen_requests,
            reserve: old.reserve,
            treasury: old.treasury,
            gateway_models: LookupSet::new(b"m"),
            balances: LookupMap::new(b"w"),
            refundable: 0,
            premium_bps: 2_000,
        }
    }

    pub fn set_runner(&mut self, runner: AccountId) {
        self.require_owner_account();
        self.runner = runner;
    }

    pub fn set_signing_address(&mut self, model: String, address: String) {
        self.require_owner_account();
        self.signing_addresses.insert(model, hl::parse_address(&address));
    }

    pub fn set_gateway_signing_address(&mut self, address: String) {
        self.require_owner_account();
        env::storage_write(GATEWAY_KEY, &hl::parse_address(&address));
    }

    // Where the live NFT images are served, for example "https://api.example.com". Token media becomes {base}/nft/{id}.svg.
    pub fn set_media_base(&mut self, url: String) {
        self.require_owner_account();
        require!(url.starts_with("https://") || url.starts_with("http://"), "media base must be an http(s) URL");
        env::storage_write(MEDIA_KEY, url.trim_end_matches('/').as_bytes());
    }

    pub fn media_base(&self) -> Option<String> {
        stored_media_base()
    }

    pub fn gateway_signing_address(&self) -> Option<String> {
        env::storage_read(GATEWAY_KEY).map(|a| format!("0x{}", hex::encode(a)))
    }

    pub fn set_epoch_len(&mut self, epoch_len_ns: U64) {
        self.require_owner_account();
        self.default_epoch_ns = epoch_len_ns.0;
    }

    pub fn set_min_mint(&mut self, min_mint_micro_usd: U64) {
        self.require_owner_account();
        self.min_mint = min_mint_micro_usd.0;
    }

    pub fn withdraw_treasury(&mut self, amount: U128, to: AccountId) -> Promise {
        self.require_owner_account();
        require!(amount.0 <= self.treasury, "exceeds treasury");
        self.treasury -= amount.0;
        self.reserve -= amount.0;
        self.ft_transfer(to, amount.0).then(Self::ext(env::current_account_id()).on_treasury_withdrawn(amount))
    }

    #[private]
    pub fn on_treasury_withdrawn(&mut self, amount: U128, #[callback_result] r: Result<(), PromiseError>) {
        if r.is_err() {
            self.treasury += amount.0;
            self.reserve += amount.0;
        }
    }

    // A payment is never refused: anything that cannot be applied is kept as a refundable balance for the named owner.
    pub fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
        require!(env::predecessor_account_id() == self.usdc, "wrong token");
        self.reserve += amount.0;
        let parsed = serde_json::from_str::<Action>(&msg).map_err(|e| format!("bad msg: {e}"));
        let owner = parsed.as_ref().map_or(sender_id.clone(), |a| a.owner().clone());
        let result = parsed.and_then(|a| self.apply(&owner, amount.0, a));
        match result {
            Ok(p) => p,
            Err(why) => {
                self.hold(&owner, amount.0);
                emit("payment_held", json!({"owner": owner, "amount": amount.0.to_string(), "reason": why}));
                PromiseOrValue::Value(U128(0))
            }
        }
    }

    pub fn withdraw_balance(&mut self) -> Promise {
        let who = env::predecessor_account_id();
        let amount = self.balances.remove(&who).unwrap_or(0);
        require!(amount > 0, "no balance");
        self.refundable -= amount;
        self.reserve -= amount;
        self.ft_transfer(who.clone(), amount).then(Self::ext(env::current_account_id()).with_static_gas(Gas::from_tgas(10)).on_balance_withdrawn(who, U128(amount)))
    }

    #[private]
    pub fn on_balance_withdrawn(&mut self, who: AccountId, amount: U128, #[callback_result] r: Result<(), PromiseError>) {
        if r.is_err() {
            self.hold(&who, amount.0);
            self.reserve += amount.0;
        }
    }

    pub fn balance_of(&self, account: AccountId) -> U128 {
        U128(self.balances.get(&account).copied().unwrap_or(0))
    }

    #[private]
    pub fn on_hl_address(
        &mut self,
        token_id: String,
        amount: U128,
        #[callback_result] pk: Result<String, PromiseError>,
    ) -> U128 {
        let address = pk.ok().and_then(|pk| {
            let b58 = pk.strip_prefix("secp256k1:")?.to_owned();
            let raw = bs58::decode(b58).into_vec().ok()?;
            (raw.len() == 64).then(|| format!("0x{}", hex::encode(&env::keccak256_array(&raw)[12..])))
        });
        match address {
            Some(a) => {
                self.agent_mut(&token_id).hl_address = a;
            }
            None => {
                if let Some(a) = self.agents.remove(&token_id) {
                    self.hold(&a.minter, amount.0);
                }
            }
        }
        U128(0)
    }

    pub fn set_gateway_model(&mut self, model: String, enabled: bool) {
        self.require_owner_account();
        if enabled {
            self.gateway_models.insert(model);
        } else {
            self.gateway_models.remove(&model);
        }
    }

    pub fn set_premium_bps(&mut self, premium_bps: u16) {
        self.require_owner_account();
        self.premium_bps = premium_bps;
    }

    pub fn is_model_registered(&self, model: String) -> bool {
        self.signing_addresses.contains_key(&model) || self.gateway_models.contains(&model)
    }

    pub fn report_equity(&mut self, token_id: String, equity_micro_usd: U64, timestamp_ns: U64) {
        self.require_runner();
        let now = env::block_timestamp();
        let a = self.agent_mut(&token_id);
        a.equity = equity_micro_usd.0;
        a.equity_ts_ns = timestamp_ns.0.min(now);
    }

    pub fn set_acquisition_budget(&mut self, token_id: String, budget_micro_usd: U64) {
        self.require_nft_owner(&token_id);
        self.agent_mut(&token_id).acquisition_budget = budget_micro_usd.0;
    }

    pub fn premium_bps(&self) -> u16 {
        self.premium_bps
    }

    pub fn price(&self, token_id: String) -> Option<U128> {
        self.agents.get(&token_id).map(|a| U128(self.price_of(a)))
    }

    pub fn buy_with_credits(&mut self, buyer_token_id: String, target_token_id: String) {
        self.require_runner();
        require!(buyer_token_id != target_token_id, "an agent cannot buy itself");
        let buyer = self.agents.get(&buyer_token_id).unwrap_or_else(|| env::panic_str("no such buyer agent"));
        let buyer_owner = buyer.owner.clone();
        let (budget, credits) = (buyer.acquisition_budget as u128, buyer.credits as u128);
        let price = self.quote(&target_token_id).unwrap_or_else(|e| env::panic_str(&e));
        require!(self.agents.get(&target_token_id).is_some_and(|t| t.owner != buyer_owner), "buyer's owner already owns the target");
        require!(price <= budget, "price above the acquisition budget");
        require!(price <= credits, "not enough credits");
        let b = self.agent_mut(&buyer_token_id);
        b.credits -= price as u64;
        b.acquisition_budget -= price as u64;
        let seller = self.settle_sale(&target_token_id, price);
        self.agent_mut(&target_token_id).acquired_by = Some(buyer_token_id.clone());
        self.move_nft(buyer_owner.clone(), &target_token_id);
        emit("agent_acquired", json!({"token_id": target_token_id, "buyer": buyer_owner, "seller": seller, "price": price.to_string(), "acquired_by": buyer_token_id}));
    }

    pub fn report_usage(&mut self, token_id: String, request_ids: Vec<String>, cost_micro_usd: U64) {
        self.require_runner();
        require!(!request_ids.is_empty(), "no request ids");
        require!(
            cost_micro_usd.0 <= MAX_COST_PER_REQUEST_MICRO_USD * request_ids.len() as u64,
            "cost above per-request cap"
        );
        for id in &request_ids {
            require!(self.seen_requests.insert(id.clone()), "request already reported");
        }
        let a = self.agent_mut(&token_id);
        // The protocol absorbs any overshoot on the last call rather than blocking accounting.
        let charged = cost_micro_usd.0.min(a.credits);
        a.credits -= charged;
        a.spent += charged;
        self.treasury += charged as u128;
        emit("usage_reported", json!({"token_id": token_id, "request_ids": request_ids, "cost": cost_micro_usd.0, "charged": charged}));
    }

    pub fn submit_receipt(&mut self, receipt: Receipt) {
        let mut receipt = receipt;
        self.require_runner();
        require!(self.agents.contains_key(&receipt.token_id), "no such agent");
        // A receipt proves a model call the agent paid for: the request must have been charged, and is stored once.
        require!(self.seen_requests.contains(&receipt.request_id), "request was never charged");
        require!(self.seen_requests.insert(format!("receipt:{}", receipt.request_id)), "receipt already stored");
        let expected: [u8; 20] = match receipt.kind {
            ReceiptKind::ProviderTee => *self.signing_addresses.get(&receipt.model).unwrap_or_else(|| env::panic_str("unknown model")),
            ReceiptKind::Gateway => env::storage_read(GATEWAY_KEY)
                .and_then(|a| a.try_into().ok())
                .unwrap_or_else(|| env::panic_str("gateway key not set")),
        };
        let sig = hex::decode(receipt.signature.trim_start_matches("0x")).unwrap_or_default();
        let signer = recover_signer(&receipt.signed_text(), &sig).unwrap_or_else(|| env::panic_str("bad signature"));
        require!(signer == expected, "signer is not the attested key");
        receipt.verified = true;
        emit("order_receipt", json!({"token_id": receipt.token_id, "order_id": receipt.order_id, "request_id": receipt.request_id, "kind": receipt.kind}));
        self.receipts.push(receipt);
    }

    pub fn report_pnl(&mut self, token_id: String, pnl_micro_usd: I64) {
        self.require_runner();
        self.agent_mut(&token_id).pnl = pnl_micro_usd.0;
    }

    pub fn release_margin(&mut self, token_id: String, deposit_address: String) -> Promise {
        self.require_runner();
        let to: AccountId = deposit_address.parse().unwrap_or_else(|_| env::panic_str("bad deposit address"));
        let now = env::block_timestamp();
        let a = self.agent_mut(&token_id);
        require!(now >= a.epoch_end_ns, "epoch not over");
        let amount = a.credits + a.margin;
        require!(amount > 0, "nothing to release");
        a.credits = 0;
        a.margin = 0;
        a.epoch_end_ns = now + a.epoch_len_ns;
        self.reserve -= amount as u128;
        emit("margin_released", json!({"token_id": token_id, "amount": amount, "to": to}));
        self.ft_transfer(to, amount as u128)
            .then(Self::ext(env::current_account_id()).with_static_gas(Gas::from_tgas(10)).on_margin_released(token_id, amount.into()))
    }

    #[private]
    pub fn on_margin_released(&mut self, token_id: String, amount: U64, #[callback_result] r: Result<(), PromiseError>) {
        if r.is_err() {
            self.agent_mut(&token_id).credits += amount.0;
            self.reserve += amount.0 as u128;
        }
    }

    pub fn fund_margin(&mut self, token_id: String, deposit_address: String) -> Promise {
        self.require_runner();
        let to: AccountId = deposit_address.parse().unwrap_or_else(|_| env::panic_str("bad deposit address"));
        let a = self.agent_mut(&token_id);
        let amount = a.margin;
        require!(amount > 0, "no margin to fund");
        a.margin = 0;
        self.reserve -= amount as u128;
        emit("margin_funded", json!({"token_id": token_id, "amount": amount, "to": to}));
        self.ft_transfer(to, amount as u128)
            .then(Self::ext(env::current_account_id()).with_static_gas(Gas::from_tgas(10)).on_margin_funded(token_id, amount.into()))
    }

    #[private]
    pub fn on_margin_funded(&mut self, token_id: String, amount: U64, #[callback_result] r: Result<(), PromiseError>) {
        if r.is_err() {
            self.agent_mut(&token_id).margin += amount.0;
            self.reserve += amount.0 as u128;
        }
    }

    pub fn set_key_terms(&mut self, token_id: String, price_usdc: U64, duration_ns: U64, revenue_to: RevenueTarget) {
        self.require_nft_owner(&token_id);
        let a = self.agent_mut(&token_id);
        a.key_price = price_usdc.0;
        a.key_duration_ns = duration_ns.0;
        a.key_revenue_to = revenue_to;
    }

    pub fn has_access(&self, token_id: String, account: AccountId) -> bool {
        let Some(a) = self.agents.get(&token_id) else { return false };
        a.owner == account || self.access.get(&(token_id, account)).is_some_and(|until| *until > env::block_timestamp())
    }

    pub fn approve_agent(&mut self, token_id: String, agent_address: String) -> Promise {
        self.require_owner_or_runner(&token_id);
        let agent = hl::parse_address(&agent_address);
        let nonce = env::block_timestamp_ms();
        // An unnamed approval (agentName "" signed, field omitted) replaces the previous unnamed agent key.
        let hash = hl::approve_agent_digest(&self.hl_chain, &agent, "", nonce);
        let action = json!({
            "type": "approveAgent",
            "signatureChainId": "0x66eee",
            "hyperliquidChain": self.hl_chain,
            "agentAddress": format!("0x{}", hex::encode(agent)),
            "nonce": nonce,
        });
        self.sign_admin(&token_id, hash, action, nonce, Some((token_id.clone(), format!("0x{}", hex::encode(agent)))))
    }

    pub fn usd_class_transfer(&mut self, token_id: String, amount: String, to_perp: bool) -> Promise {
        self.require_owner_or_runner(&token_id);
        let nonce = env::block_timestamp_ms();
        let hash = hl::usd_class_transfer_digest(&self.hl_chain, &amount, to_perp, nonce);
        let action = json!({
            "type": "usdClassTransfer",
            "signatureChainId": "0x66eee",
            "hyperliquidChain": self.hl_chain,
            "amount": amount,
            "toPerp": to_perp,
            "nonce": nonce,
        });
        self.sign_admin(&token_id, hash, action, nonce, None)
    }

    pub fn withdraw(&mut self, token_id: String, amount: String, destination: String) -> Promise {
        self.require_nft_owner(&token_id);
        let dest = hl::parse_address(&destination);
        let time = env::block_timestamp_ms();
        let hash = hl::withdraw_digest(&self.hl_chain, &dest, &amount, time);
        let action = json!({
            "type": "withdraw3",
            "signatureChainId": "0x66eee",
            "hyperliquidChain": self.hl_chain,
            "destination": format!("0x{}", hex::encode(dest)),
            "amount": amount,
            "time": time,
        });
        self.sign_admin(&token_id, hash, action, time, None)
    }

    #[private]
    pub fn on_signed(
        &mut self,
        action: Value,
        nonce: u64,
        approved: Option<(String, String)>,
        #[callback_result] sig: Result<MpcSignature, PromiseError>,
    ) -> SignedAction {
        let sig = sig.unwrap_or_else(|_| env::panic_str("MPC sign failed"));
        if let Some((token_id, address)) = approved {
            let a = self.agent_mut(&token_id);
            a.agent_address = Some(address.clone());
            a.needs_rotation = false;
            emit("agent_rotated", json!({"token_id": token_id, "agent_address": address}));
        }
        // big_r is a compressed point (02/03 || x); Ethereum r is its x coordinate, v is 27 + recovery id.
        let r = &sig.big_r.affine_point[2..];
        SignedAction {
            action,
            nonce,
            signature: HlSignature { r: format!("0x{r}"), s: format!("0x{}", sig.s.scalar), v: 27 + sig.recovery_id },
        }
    }

    pub fn signing_address(&self, model: String) -> Option<String> {
        self.signing_addresses.get(&model).map(|a| format!("0x{}", hex::encode(a)))
    }

    pub fn receipts(&self, from_index: u32, limit: u32) -> Vec<Receipt> {
        self.receipts.iter().skip(from_index as usize).take(limit as usize).cloned().collect()
    }

    pub fn hl_address(&self, token_id: String) -> Option<String> {
        self.agents.get(&token_id).map(|a| a.hl_address.clone()).filter(|a| !a.is_empty())
    }

    pub fn agent(&self, token_id: String) -> Option<AgentView> {
        self.agents.get(&token_id).map(|a| view(&token_id, a))
    }

    pub fn agents(&self, from_index: u32, limit: u32) -> Vec<AgentView> {
        self.agents.iter().skip(from_index as usize).take(limit as usize).map(|(id, a)| view(id, a)).collect()
    }

    pub fn accounting(&self) -> Accounting {
        let credits: u128 = self.agents.values().map(|a| (a.credits + a.margin) as u128).sum();
        Accounting { reserve_usdc: U128(self.reserve), treasury_usdc: U128(self.treasury), total_credits: U128(credits), refundable_usdc: U128(self.refundable) }
    }
}

impl Contract {
    #[allow(clippy::too_many_arguments)]
    fn build(
        owner: AccountId,
        mpc: AccountId,
        hl_mainnet: bool,
        runner: AccountId,
        usdc: AccountId,
        royalty_bps: u16,
        signing_addresses: LookupMap<String, [u8; 20]>,
        receipts: Vector<Receipt>,
    ) -> Self {
        Self {
            owner,
            mpc,
            hl_chain: if hl_mainnet { "Mainnet" } else { "Testnet" }.into(),
            runner,
            signing_addresses,
            receipts,
            usdc,
            royalty_bps,
            min_mint: 20_000_000,
            default_epoch_ns: 7 * DAY_NS,
            next_id: 0,
            agents: IterableMap::new(b"b"),
            access: LookupMap::new(b"k"),
            seen_requests: LookupSet::new(b"q"),
            reserve: 0,
            treasury: 0,
            gateway_models: LookupSet::new(b"m"),
            balances: LookupMap::new(b"w"),
            refundable: 0,
            premium_bps: 2_000,
        }
    }

    fn hold(&mut self, owner: &AccountId, amount: u128) {
        if amount > 0 {
            self.refundable += amount;
            *self.balances.entry(owner.clone()).or_insert(0) += amount;
        }
    }

    fn apply(&mut self, owner: &AccountId, amount: u128, action: Action) -> Result<PromiseOrValue<U128>, String> {
        let amt = u64::try_from(amount).map_err(|_| "amount too large".to_string())?;
        match action {
            Action::Mint { max_leverage, credits_bps, model, strategy_hash, .. } => {
                Ok(PromiseOrValue::Promise(self.mint(owner.clone(), amt, max_leverage, credits_bps, model, &strategy_hash)?))
            }
            Action::Load { token_id, .. } => {
                self.agents.get_mut(&token_id).ok_or("no such agent")?.credits += amt;
                emit("credits_loaded", json!({"token_id": token_id, "amount": amt, "from": owner}));
                Ok(PromiseOrValue::Value(U128(0)))
            }
            Action::BuyKey { token_id, .. } => {
                let now = env::block_timestamp();
                let a = self.agents.get_mut(&token_id).ok_or("no such agent")?;
                if a.key_price == 0 || amt < a.key_price {
                    return Err("below key price".into());
                }
                let duration = a.key_duration_ns;
                match a.key_revenue_to {
                    RevenueTarget::Credits => a.credits += amt,
                    RevenueTarget::Margin => a.margin += amt,
                }
                let key = (token_id.clone(), owner.clone());
                let until = self.access.get(&key).copied().unwrap_or(0).max(now) + duration;
                self.access.insert(key, until);
                emit("key_sold", json!({"token_id": token_id, "buyer": owner, "amount": amt, "until_ns": until.to_string()}));
                Ok(PromiseOrValue::Value(U128(0)))
            }
            Action::Buy { token_id, .. } => {
                let price = self.quote(&token_id)?;
                if amount < price {
                    return Err("below price".into());
                }
                if self.agents.get(&token_id).is_some_and(|a| &a.owner == owner) {
                    return Err("already the owner".into());
                }
                self.hold(owner, amount - price);
                let seller = self.settle_sale(&token_id, price);
                self.move_nft(owner.clone(), &token_id);
                emit("agent_acquired", json!({"token_id": token_id, "buyer": owner, "seller": seller, "price": price.to_string(), "acquired_by": null}));
                Ok(PromiseOrValue::Value(U128(0)))
            }
        }
    }

    fn price_of(&self, a: &Agent) -> u128 {
        let premium = a.pnl.max(0) as u128 * self.premium_bps as u128 / 10_000;
        a.equity as u128 + a.credits as u128 + a.margin as u128 + premium
    }

    // The price is only quotable while the runner's equity report is fresh.
    fn quote(&self, token_id: &str) -> Result<u128, String> {
        let a = self.agents.get(token_id).ok_or("no such agent")?;
        if a.equity_ts_ns == 0 || env::block_timestamp().saturating_sub(a.equity_ts_ns) > EQUITY_MAX_AGE_NS {
            return Err("stale equity report".into());
        }
        Ok(self.price_of(a))
    }

    // Credits the seller and the minter's royalty; the USDC is already in the reserve. Returns the seller.
    fn settle_sale(&mut self, token_id: &str, price: u128) -> AccountId {
        let a = self.agent_mut(token_id);
        a.equity_ts_ns = 0;
        let (seller, minter) = (a.owner.clone(), a.minter.clone());
        let royalty = price * self.royalty_bps as u128 / 10_000;
        self.hold(&seller, price - royalty);
        self.hold(&minter, royalty);
        seller
    }

    fn mint(&mut self, minter: AccountId, amount: u64, max_leverage: u8, credits_bps: u16, model: String, strategy_hash: &str) -> Result<Promise, String> {
        if amount < self.min_mint {
            return Err("below minimum load".into());
        }
        if !(1..=40).contains(&max_leverage) {
            return Err("leverage out of range".into());
        }
        if !(1..=10_000).contains(&credits_bps) {
            return Err("credits_bps out of range".into());
        }
        if !self.is_model_registered(model.clone()) {
            return Err("model not registered".into());
        }
        let hash: [u8; 32] = if strategy_hash.is_empty() {
            [0; 32]
        } else {
            hex::decode(strategy_hash.trim_start_matches("0x")).ok().and_then(|b| b.try_into().ok()).ok_or("strategy_hash must be 32 bytes of hex")?
        };
        let credits = (amount as u128 * credits_bps as u128 / 10_000) as u64;
        self.next_id += 1;
        let token_id = self.next_id.to_string();
        let epoch_end_ns = env::block_timestamp() + self.default_epoch_ns;
        self.agents.insert(
            token_id.clone(),
            Agent {
                minter: minter.clone(),
                owner: minter.clone(),
                credits,
                margin: amount - credits,
                epoch_len_ns: self.default_epoch_ns,
                epoch_end_ns,
                hl_address: String::new(),
                agent_address: None,
                needs_rotation: true,
                max_leverage,
                flat_at_epoch_end: true,
                pnl: 0,
                key_price: 0,
                key_duration_ns: DAY_NS,
                key_revenue_to: RevenueTarget::Credits,
                strategy_hash: hash,
                model: model.clone(),
                acquisition_budget: 0,
                acquired_by: None,
                equity: 0,
                equity_ts_ns: 0,
                spent: 0,
            },
        );
        emit("agent_minted", json!({"token_id": token_id, "minter": minter, "credits": credits, "margin": amount - credits, "model": model, "strategy_hash": hex::encode(hash)}));
        let args = json!({"path": agent_path(&token_id), "predecessor": env::current_account_id(), "domain_id": 0});
        Ok(Promise::new(self.mpc.clone())
            .function_call("derived_public_key", args.to_string().into_bytes(), NearToken::from_yoctonear(0), Gas::from_tgas(10))
            .then(Self::ext(env::current_account_id()).with_static_gas(Gas::from_tgas(10)).on_hl_address(token_id, U128(amount as u128))))
    }

    fn ft_transfer(&self, to: AccountId, amount: u128) -> Promise {
        let args = json!({"receiver_id": to, "amount": U128(amount)});
        Promise::new(self.usdc.clone()).function_call(
            "ft_transfer",
            args.to_string().into_bytes(),
            NearToken::from_yoctonear(1),
            Gas::from_tgas(30),
        )
    }

    fn sign_admin(&self, token_id: &str, hash: [u8; 32], action: Value, nonce: u64, approved: Option<(String, String)>) -> Promise {
        let args = json!({ "request": { "path": agent_path(token_id), "payload_v2": { "Ecdsa": hex::encode(hash) }, "domain_id": 0 } });
        Promise::new(self.mpc.clone())
            .function_call("sign", args.to_string().into_bytes(), NearToken::from_yoctonear(1), Gas::from_tgas(50))
            .then(Self::ext(env::current_account_id()).with_static_gas(Gas::from_tgas(10)).on_signed(action, nonce, approved))
    }

    fn agent_mut(&mut self, token_id: &str) -> &mut Agent {
        self.agents.get_mut(token_id).unwrap_or_else(|| env::panic_str("no such agent"))
    }

    fn require_owner_account(&self) {
        require!(env::predecessor_account_id() == self.owner, "owner only");
    }

    fn require_runner(&self) {
        require!(env::predecessor_account_id() == self.runner, "runner only");
    }

    fn require_nft_owner(&self, token_id: &str) {
        let a = self.agents.get(token_id).unwrap_or_else(|| env::panic_str("no such agent"));
        require!(a.owner == env::predecessor_account_id(), "NFT owner only");
    }

    fn require_owner_or_runner(&self, token_id: &str) {
        let a = self.agents.get(token_id).unwrap_or_else(|| env::panic_str("no such agent"));
        let who = env::predecessor_account_id();
        require!(a.owner == who || self.runner == who, "NFT owner or runner only");
    }
}

fn view(token_id: &str, a: &Agent) -> AgentView {
    AgentView {
        token_id: token_id.to_owned(),
        minter: a.minter.clone(),
        owner: a.owner.clone(),
        credits_micro_usd: a.credits.into(),
        margin_micro_usd: a.margin.into(),
        epoch_len_ns: a.epoch_len_ns.into(),
        epoch_end_ns: a.epoch_end_ns.into(),
        hl_address: a.hl_address.clone(),
        agent_address: a.agent_address.clone(),
        needs_rotation: a.needs_rotation,
        max_leverage: a.max_leverage,
        flat_at_epoch_end: a.flat_at_epoch_end,
        pnl_micro_usd: a.pnl.into(),
        key_price_usdc: a.key_price.into(),
        key_duration_ns: a.key_duration_ns.into(),
        key_revenue_to: a.key_revenue_to,
        strategy_hash: hex::encode(a.strategy_hash),
        model: a.model.clone(),
        acquisition_budget_micro_usd: a.acquisition_budget.into(),
        acquired_by: a.acquired_by.clone(),
        equity_micro_usd: a.equity.into(),
        equity_ts_ns: a.equity_ts_ns.into(),
        spent_micro_usd: a.spent.into(),
    }
}

#[cfg(test)]
mod tests;
