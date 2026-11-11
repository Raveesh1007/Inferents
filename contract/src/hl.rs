use near_sdk::env::keccak256_array as keccak;

// User-signed Hyperliquid actions: domain chainId is signatureChainId (0x66eee = Arbitrum Sepolia), verifyingContract is zero.
const SIGNATURE_CHAIN_ID: u64 = 0x66eee;

fn word_u64(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

fn word_address(a: &[u8; 20]) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a);
    w
}

fn domain_separator() -> [u8; 32] {
    let mut enc = Vec::with_capacity(5 * 32);
    enc.extend(keccak(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    ));
    enc.extend(keccak(b"HyperliquidSignTransaction"));
    enc.extend(keccak(b"1"));
    enc.extend(word_u64(SIGNATURE_CHAIN_ID));
    enc.extend([0u8; 32]);
    keccak(&enc)
}

fn digest(struct_hash: [u8; 32]) -> [u8; 32] {
    let mut m = Vec::with_capacity(66);
    m.extend([0x19, 0x01]);
    m.extend(domain_separator());
    m.extend(struct_hash);
    keccak(&m)
}

pub fn parse_address(s: &str) -> [u8; 20] {
    let h = s.strip_prefix("0x").unwrap_or(s);
    let b = hex::decode(h).unwrap_or_default();
    b.try_into().unwrap_or_else(|_| near_sdk::env::panic_str("invalid EVM address"))
}

pub fn approve_agent_digest(chain: &str, agent: &[u8; 20], agent_name: &str, nonce: u64) -> [u8; 32] {
    let mut enc = Vec::with_capacity(5 * 32);
    enc.extend(keccak(
        b"HyperliquidTransaction:ApproveAgent(string hyperliquidChain,address agentAddress,string agentName,uint64 nonce)",
    ));
    enc.extend(keccak(chain.as_bytes()));
    enc.extend(word_address(agent));
    enc.extend(keccak(agent_name.as_bytes()));
    enc.extend(word_u64(nonce));
    digest(keccak(&enc))
}

pub fn usd_class_transfer_digest(chain: &str, amount: &str, to_perp: bool, nonce: u64) -> [u8; 32] {
    let mut enc = Vec::with_capacity(5 * 32);
    enc.extend(keccak(
        b"HyperliquidTransaction:UsdClassTransfer(string hyperliquidChain,string amount,bool toPerp,uint64 nonce)",
    ));
    enc.extend(keccak(chain.as_bytes()));
    enc.extend(keccak(amount.as_bytes()));
    enc.extend(word_u64(to_perp as u64));
    enc.extend(word_u64(nonce));
    digest(keccak(&enc))
}

pub fn withdraw_digest(chain: &str, destination: &[u8; 20], amount: &str, time: u64) -> [u8; 32] {
    // Withdraw's destination is a string in the EIP-712 type, lowercase 0x-hex.
    let dest = format!("0x{}", hex::encode(destination));
    let mut enc = Vec::with_capacity(5 * 32);
    enc.extend(keccak(
        b"HyperliquidTransaction:Withdraw(string hyperliquidChain,string destination,string amount,uint64 time)",
    ));
    enc.extend(keccak(chain.as_bytes()));
    enc.extend(keccak(dest.as_bytes()));
    enc.extend(keccak(amount.as_bytes()));
    enc.extend(word_u64(time));
    digest(keccak(&enc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_dyn_abi::TypedData;

    #[test]
    fn approve_agent_digest_matches_alloy_typed_data() {
        let agent = "0x1234567890abcdef1234567890abcdef12345678";
        let td: TypedData = serde_json::from_value(serde_json::json!({
            "types": {
                "EIP712Domain": [
                    {"name": "name", "type": "string"},
                    {"name": "version", "type": "string"},
                    {"name": "chainId", "type": "uint256"},
                    {"name": "verifyingContract", "type": "address"}
                ],
                "HyperliquidTransaction:ApproveAgent": [
                    {"name": "hyperliquidChain", "type": "string"},
                    {"name": "agentAddress", "type": "address"},
                    {"name": "agentName", "type": "string"},
                    {"name": "nonce", "type": "uint64"}
                ]
            },
            "primaryType": "HyperliquidTransaction:ApproveAgent",
            "domain": {"name": "HyperliquidSignTransaction", "version": "1", "chainId": 421614,
                       "verifyingContract": "0x0000000000000000000000000000000000000000"},
            "message": {"hyperliquidChain": "Testnet", "agentAddress": agent, "agentName": "", "nonce": 1762000000000u64}
        }))
        .unwrap();
        let want = td.eip712_signing_hash().unwrap();
        let got = approve_agent_digest("Testnet", &parse_address(agent), "", 1_762_000_000_000);
        assert_eq!(got, want.0);
    }

    #[test]
    fn usd_class_transfer_digest_matches_alloy_typed_data() {
        let td: TypedData = serde_json::from_value(serde_json::json!({
            "types": {
                "EIP712Domain": [
                    {"name": "name", "type": "string"},
                    {"name": "version", "type": "string"},
                    {"name": "chainId", "type": "uint256"},
                    {"name": "verifyingContract", "type": "address"}
                ],
                "HyperliquidTransaction:UsdClassTransfer": [
                    {"name": "hyperliquidChain", "type": "string"},
                    {"name": "amount", "type": "string"},
                    {"name": "toPerp", "type": "bool"},
                    {"name": "nonce", "type": "uint64"}
                ]
            },
            "primaryType": "HyperliquidTransaction:UsdClassTransfer",
            "domain": {"name": "HyperliquidSignTransaction", "version": "1", "chainId": 421614,
                       "verifyingContract": "0x0000000000000000000000000000000000000000"},
            "message": {"hyperliquidChain": "Testnet", "amount": "49", "toPerp": true, "nonce": 1762000000000u64}
        }))
        .unwrap();
        let want = td.eip712_signing_hash().unwrap();
        assert_eq!(usd_class_transfer_digest("Testnet", "49", true, 1_762_000_000_000), want.0);
    }

    #[test]
    fn withdraw_digest_matches_alloy_typed_data() {
        let dest = "0x1234567890abcdef1234567890abcdef12345678";
        let td: TypedData = serde_json::from_value(serde_json::json!({
            "types": {
                "EIP712Domain": [
                    {"name": "name", "type": "string"},
                    {"name": "version", "type": "string"},
                    {"name": "chainId", "type": "uint256"},
                    {"name": "verifyingContract", "type": "address"}
                ],
                "HyperliquidTransaction:Withdraw": [
                    {"name": "hyperliquidChain", "type": "string"},
                    {"name": "destination", "type": "string"},
                    {"name": "amount", "type": "string"},
                    {"name": "time", "type": "uint64"}
                ]
            },
            "primaryType": "HyperliquidTransaction:Withdraw",
            "domain": {"name": "HyperliquidSignTransaction", "version": "1", "chainId": 421614,
                       "verifyingContract": "0x0000000000000000000000000000000000000000"},
            "message": {"hyperliquidChain": "Testnet", "destination": dest, "amount": "5", "time": 1762000000000u64}
        }))
        .unwrap();
        let want = td.eip712_signing_hash().unwrap();
        assert_eq!(withdraw_digest("Testnet", &parse_address(dest), "5", 1_762_000_000_000), want.0);
    }
}
