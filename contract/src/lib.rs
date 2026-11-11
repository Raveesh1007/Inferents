mod hl;
mod nft;
mod receipt;

use near_sdk::json_types::{I64, U64, U128};
use near_sdk::serde_json::{self, Value, json};
use near_sdk::store::{IterableMap, LookupMap, LookupSet, Vector};
use near_sdk::{
    AccountId, Gas, NearToken, Promise, PromiseError, PromiseOrValue, env, log, near, require,
};
use receipt::{Receipt, recover_signer};

pub const MAX_COST_PER_REQUEST_MICRO_USD: u64 = 1_000_000;
const DAY_NS: u64 = 86_400_000_000_000;

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
    pub pnl: i64,
    pub key_price: u64,
    pub key_duration_ns: u64,
    pub key_revenue_to: RevenueTarget,
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
}

#[near(serializers = [json])]
pub struct Accounting {
    pub reserve_usdc: U128,
    pub treasury_usdc: U128,
    pub total_credits: U128,
}

#[near(serializers = [json])]
#[serde(tag = "action", rename_all = "snake_case")]
enum Action {
    Mint { max_leverage: u8 },
    Load { token_id: String },
    BuyKey { token_id: String },
}

#[near(serializers = [borsh])]
struct OldContract {
    owner: AccountId,
    mpc: AccountId,
    hl_chain: String,
    runner: AccountId,
    signing_addresses: LookupMap<String, [u8; 20]>,
    receipts: Vector<Receipt>,
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
    pub fn migrate(usdc_token: AccountId) -> Self {
        let old: OldContract = env::state_read().unwrap_or_else(|| env::panic_str("no old state"));
        let hl_mainnet = old.hl_chain == "Mainnet";
        Self::build(old.owner, old.mpc, hl_mainnet, old.runner, usdc_token, 500, old.signing_addresses, old.receipts)
    }

    pub fn set_runner(&mut self, runner: AccountId) {
        self.require_owner_account();
        self.runner = runner;
    }

    pub fn set_signing_address(&mut self, model: String, address: String) {
        self.require_owner_account();
        self.signing_addresses.insert(model, hl::parse_address(&address));
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

    pub fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
        require!(env::predecessor_account_id() == self.usdc, "wrong token");
        let amt = u64::try_from(amount.0).unwrap_or_else(|_| env::panic_str("amount too large"));
        let action: Action = serde_json::from_str(&msg).unwrap_or_else(|_| env::panic_str("bad msg"));
        self.reserve += amount.0;
        match action {
            Action::Mint { max_leverage } => PromiseOrValue::Promise(self.mint(sender_id, amt, max_leverage)),
            Action::Load { token_id } => {
                self.agent_mut(&token_id).credits += amt;
                emit("credits_loaded", json!({"token_id": token_id, "amount": amt, "from": sender_id}));
                PromiseOrValue::Value(U128(0))
            }
            Action::BuyKey { token_id } => {
                let now = env::block_timestamp();
                let a = self.agent_mut(&token_id);
                require!(a.key_price > 0 && amt >= a.key_price, "below key price");
                let duration = a.key_duration_ns;
                match a.key_revenue_to {
                    RevenueTarget::Credits => a.credits += amt,
                    RevenueTarget::Margin => a.margin += amt,
                }
                let key = (token_id.clone(), sender_id.clone());
                let until = self.access.get(&key).copied().unwrap_or(0).max(now) + duration;
                self.access.insert(key, until);
                emit("key_sold", json!({"token_id": token_id, "buyer": sender_id, "amount": amt, "until_ns": until.to_string()}));
                PromiseOrValue::Value(U128(0))
            }
        }
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
                U128(0)
            }
            None => {
                self.agents.remove(&token_id);
                self.reserve -= amount.0;
                amount
            }
        }
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
        self.treasury += charged as u128;
        emit("usage_reported", json!({"token_id": token_id, "request_ids": request_ids, "cost": cost_micro_usd.0, "charged": charged}));
    }

    pub fn submit_receipt(&mut self, receipt: Receipt) {
        let mut receipt = receipt;
        self.require_runner();
        require!(self.agents.contains_key(&receipt.token_id), "no such agent");
        let expected = self.signing_addresses.get(&receipt.model).unwrap_or_else(|| env::panic_str("unknown model"));
        let sig = hex::decode(receipt.signature.trim_start_matches("0x")).unwrap_or_default();
        let signer = recover_signer(&receipt.signed_text(), &sig).unwrap_or_else(|| env::panic_str("bad signature"));
        require!(&signer == expected, "signer is not the attested model key");
        receipt.verified = true;
        emit("order_receipt", json!({"token_id": receipt.token_id, "order_id": receipt.order_id, "request_id": receipt.request_id}));
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
        Accounting { reserve_usdc: U128(self.reserve), treasury_usdc: U128(self.treasury), total_credits: U128(credits) }
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
            agents: IterableMap::new(b"a"),
            access: LookupMap::new(b"k"),
            seen_requests: LookupSet::new(b"q"),
            reserve: 0,
            treasury: 0,
        }
    }

    fn mint(&mut self, minter: AccountId, amount: u64, max_leverage: u8) -> Promise {
        require!(amount >= self.min_mint, "below minimum load");
        require!((1..=40).contains(&max_leverage), "leverage out of range");
        self.next_id += 1;
        let token_id = self.next_id.to_string();
        let epoch_end_ns = env::block_timestamp() + self.default_epoch_ns;
        self.agents.insert(
            token_id.clone(),
            Agent {
                minter: minter.clone(),
                owner: minter.clone(),
                credits: amount,
                margin: 0,
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
            },
        );
        emit("agent_minted", json!({"token_id": token_id, "minter": minter, "credits": amount}));
        let args = json!({"path": agent_path(&token_id), "predecessor": env::current_account_id(), "domain_id": 0});
        Promise::new(self.mpc.clone())
            .function_call("derived_public_key", args.to_string().into_bytes(), NearToken::from_yoctonear(0), Gas::from_tgas(10))
            .then(Self::ext(env::current_account_id()).with_static_gas(Gas::from_tgas(10)).on_hl_address(token_id, U128(amount as u128)))
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
    }
}

#[cfg(test)]
mod tests;
