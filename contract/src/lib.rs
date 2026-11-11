mod hl;
mod receipt;

use receipt::{Receipt, recover_signer};

use near_sdk::serde_json::json;
use near_sdk::store::{LookupMap, Vector};
use near_sdk::{AccountId, Gas, NearToken, Promise, PromiseError, env, log, near, require};

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

#[near(serializers = [borsh])]
struct OldContract {
    owner: AccountId,
    mpc: AccountId,
    hl_chain: String,
}

#[near(contract_state)]
pub struct Contract {
    owner: AccountId,
    mpc: AccountId,
    hl_chain: String,
    runner: AccountId,
    signing_addresses: LookupMap<String, [u8; 20]>,
    receipts: Vector<Receipt>,
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
            runner: owner_id.clone(),
            owner: owner_id,
            mpc: mpc_contract,
            hl_chain: if hl_mainnet { "Mainnet" } else { "Testnet" }.into(),
            signing_addresses: LookupMap::new(b"s"),
            receipts: Vector::new(b"r"),
        }
    }

    #[private]
    #[init(ignore_state)]
    pub fn migrate() -> Self {
        let old: OldContract = env::state_read().unwrap_or_else(|| env::panic_str("no old state"));
        Self {
            runner: old.owner.clone(),
            owner: old.owner,
            mpc: old.mpc,
            hl_chain: old.hl_chain,
            signing_addresses: LookupMap::new(b"s"),
            receipts: Vector::new(b"r"),
        }
    }

    pub fn set_runner(&mut self, runner: AccountId) {
        require!(env::predecessor_account_id() == self.owner, "owner only");
        self.runner = runner;
    }

    pub fn set_signing_address(&mut self, model: String, address: String) {
        require!(env::predecessor_account_id() == self.owner, "owner only");
        self.signing_addresses.insert(model, hl::parse_address(&address));
    }

    pub fn signing_address(&self, model: String) -> Option<String> {
        self.signing_addresses.get(&model).map(|a| format!("0x{}", hex::encode(a)))
    }

    pub fn submit_receipt(&mut self, mut receipt: Receipt) {
        require!(env::predecessor_account_id() == self.runner, "runner only");
        let expected = self.signing_addresses.get(&receipt.model).unwrap_or_else(|| env::panic_str("unknown model"));
        let sig = hex::decode(receipt.signature.trim_start_matches("0x")).unwrap_or_default();
        let signer = recover_signer(&receipt.signed_text(), &sig).unwrap_or_else(|| env::panic_str("bad signature"));
        require!(&signer == expected, "signer is not the attested model key");
        receipt.verified = true;
        log!(
            "EVENT_JSON:{}",
            json!({"standard": "inferents", "version": "1.0.0", "event": "order_receipt",
                   "data": [{"token_id": receipt.token_id, "order_id": receipt.order_id, "request_id": receipt.request_id}]})
        );
        self.receipts.push(receipt);
    }

    pub fn receipts(&self, from_index: u32, limit: u32) -> Vec<Receipt> {
        self.receipts.iter().skip(from_index as usize).take(limit as usize).cloned().collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::{serde_json, testing_env};

    fn setup() -> (Contract, serde_json::Value) {
        let me: AccountId = "owner.testnet".parse().unwrap();
        testing_env!(VMContextBuilder::new().predecessor_account_id(me.clone()).build());
        let fx: serde_json::Value = serde_json::from_str(include_str!("../../runner/fixtures/receipt1.json")).unwrap();
        let mut c = Contract::new(me, "v1.signer-prod.testnet".parse().unwrap(), false);
        c.set_signing_address(fx["model"].as_str().unwrap().into(), fx["signing_address"].as_str().unwrap().into());
        (c, fx)
    }

    fn receipt(fx: &serde_json::Value) -> Receipt {
        Receipt {
            token_id: "1".into(),
            order_id: "o1".into(),
            request_id: fx["request_id"].as_str().unwrap().into(),
            model: fx["model"].as_str().unwrap().into(),
            request_hash: fx["request_hash"].as_str().unwrap().into(),
            response_hash: fx["response_hash"].as_str().unwrap().into(),
            signature: fx["signature"].as_str().unwrap().into(),
            verified: false,
        }
    }

    #[test]
    fn real_near_ai_receipt_verifies() {
        let (mut c, fx) = setup();
        c.submit_receipt(receipt(&fx));
        let stored = c.receipts(0, 10);
        assert_eq!(stored.len(), 1);
        assert!(stored[0].verified);
    }

    #[test]
    #[should_panic(expected = "signer is not the attested model key")]
    fn tampered_response_hash_is_rejected() {
        let (mut c, fx) = setup();
        let mut r = receipt(&fx);
        r.response_hash = "0".repeat(64);
        c.submit_receipt(r);
    }
}
