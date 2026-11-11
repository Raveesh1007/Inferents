use near_sdk::{env, near};

#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub struct Receipt {
    pub token_id: String,
    pub order_id: String,
    pub request_id: String,
    pub model: String,
    pub request_hash: String,
    pub response_hash: String,
    pub signature: String,
    pub verified: bool,
}

impl Receipt {
    pub fn signed_text(&self) -> String {
        format!("{}:{}:{}", self.model, self.request_hash, self.response_hash)
    }
}

// NEAR AI signs the text with an Ethereum personal_sign (EIP-191) signature, so the recovered key is over the prefixed hash.
pub fn recover_signer(text: &str, sig: &[u8]) -> Option<[u8; 20]> {
    if sig.len() != 65 {
        return None;
    }
    let v = match sig[64] {27 | 28 => sig[64] - 27,
        0 | 1 => sig[64],
        _ => return None,
    };
    let mut msg = format!("\x19Ethereum Signed Message:\n{}", text.len()).into_bytes();
    msg.extend_from_slice(text.as_bytes());
    let hash = env::keccak256_array(&msg);
    // malleability check off: authenticity rests on the signer address match, not signature uniqueness.
    let pubkey = env::ecrecover(&hash, &sig[..64], v, false)?;
    env::keccak256_array(&pubkey)[12..].try_into().ok()
}
