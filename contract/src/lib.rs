mod hl;

use near_sdk::serde_json::json;
use near_sdk::{AccountId, Gas, NearToken, Promise, PromiseError, env, near, require};

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
    pub action: near_sdk::serde_json::Value,
    pub nonce: u64,
    pub signature: HlSignature,
}

#[near(contract_state)]
pub struct Contract {
    owner: AccountId,
    mpc: AccountId,
    hl_chain: String,
}

impl Default for Contract {
    fn default() -> Self {
        env::panic_str("not initialized")
    }
}

pub fn agent_path(token_id: &str) -> String {
    format!("agent-{token_id}")
}

#[near]
impl Contract {
    #[init]
    pub fn new(owner_id: AccountId, mpc_contract: AccountId, hl_mainnet: bool) -> Self {
        Self {
            owner: owner_id,
            mpc: mpc_contract,
            hl_chain: if hl_mainnet { "Mainnet" } else { "Testnet" }.into(),
        }
    }

    pub fn approve_agent(&mut self, token_id: String, agent_address: String) -> Promise {
        require!(env::predecessor_account_id() == self.owner, "owner only");
        let agent = hl::parse_address(&agent_address);
        let nonce = env::block_timestamp_ms();
        // Unnamed agent: sign with agentName "" and omit the field from the action, which replaces the previous unnamed agent.
        let hash = hl::approve_agent_digest(&self.hl_chain, &agent, "", nonce);
        let action = json!({
            "type": "approveAgent",
            "signatureChainId": "0x66eee",
            "hyperliquidChain": self.hl_chain,
            "agentAddress": format!("0x{}", hex::encode(agent)),
            "nonce": nonce,
        });
        self.mpc_sign(&agent_path(&token_id), hash)
            .then(Self::ext(env::current_account_id()).with_static_gas(Gas::from_tgas(10)).on_signed(action, nonce))
    }

    pub fn usd_class_transfer(&mut self, token_id: String, amount: String, to_perp: bool) -> Promise {
        require!(env::predecessor_account_id() == self.owner, "owner only");
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
        self.mpc_sign(&agent_path(&token_id), hash)
            .then(Self::ext(env::current_account_id()).with_static_gas(Gas::from_tgas(10)).on_signed(action, nonce))
    }

    pub fn withdraw(&mut self, token_id: String, amount: String, destination: String) -> Promise {
        require!(env::predecessor_account_id() == self.owner, "owner only");
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
        self.mpc_sign(&agent_path(&token_id), hash)
            .then(Self::ext(env::current_account_id()).with_static_gas(Gas::from_tgas(10)).on_signed(action, time))
    }

    #[private]
    pub fn on_signed(
        &self,
        action: near_sdk::serde_json::Value,
        nonce: u64,
        #[callback_result] sig: Result<MpcSignature, PromiseError>,
    ) -> SignedAction {
        let sig = sig.unwrap_or_else(|_| env::panic_str("MPC sign failed"));
        // big_r is a compressed point (02/03 || x); Ethereum r is its x coordinate, v is 27 + recovery id.
        let r = &sig.big_r.affine_point[2..];
        SignedAction {
            action,
            nonce,
            signature: HlSignature { r: format!("0x{r}"), s: format!("0x{}", sig.s.scalar), v: 27 + sig.recovery_id },
        }
    }
}

impl Contract {
    fn mpc_sign(&self, path: &str, hash: [u8; 32]) -> Promise {
        let args = json!({ "request": { "path": path, "payload_v2": { "Ecdsa": hex::encode(hash) }, "domain_id": 0 } });
        Promise::new(self.mpc.clone()).function_call(
            "sign",
            args.to_string().into_bytes(),
            NearToken::from_yoctonear(1),
            Gas::from_tgas(50),
        )
    }
}
