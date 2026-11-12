use near_sdk::borsh::{BorshDeserialize, BorshSerialize, io};
use near_sdk::{env, near};

// ProviderTee: the model itself ran in a NEAR AI TEE and signed `model:req:resp`.
// Gateway: NEAR AI's TEE gateway signed `req:resp` for a response relayed from an unattested provider. It does not
// cover the model name; the request body (kept off-chain) names the model and hashes to request_hash.
#[near(serializers = [json, borsh])]
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum ReceiptKind {
    #[default]
    ProviderTee,
    Gateway,
}

#[near(serializers = [json])]
#[derive(Clone, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
pub struct Receipt {
    pub token_id: String,
    pub order_id: String,
    pub request_id: String,
    pub model: String,
    pub request_hash: String,
    pub response_hash: String,
    pub signature: String,
    pub verified: bool,
    #[serde(default)]
    pub kind: ReceiptKind,
}

// Receipts stored before `kind` existed end after `verified`; they were all ProviderTee.
impl BorshDeserialize for Receipt {
    fn deserialize_reader<R: io::Read>(r: &mut R) -> io::Result<Self> {
        Ok(Self {
            token_id: BorshDeserialize::deserialize_reader(r)?,
            order_id: BorshDeserialize::deserialize_reader(r)?,
            request_id: BorshDeserialize::deserialize_reader(r)?,
            model: BorshDeserialize::deserialize_reader(r)?,
            request_hash: BorshDeserialize::deserialize_reader(r)?,
            response_hash: BorshDeserialize::deserialize_reader(r)?,
            signature: BorshDeserialize::deserialize_reader(r)?,
            verified: BorshDeserialize::deserialize_reader(r)?,
            kind: ReceiptKind::deserialize_reader(r).unwrap_or_default(),
        })
    }
}

impl Receipt {
    pub fn signed_text(&self) -> String {
        match self.kind {
            ReceiptKind::ProviderTee => format!("{}:{}:{}", self.model, self.request_hash, self.response_hash),
            ReceiptKind::Gateway => format!("{}:{}", self.request_hash, self.response_hash),
        }
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
