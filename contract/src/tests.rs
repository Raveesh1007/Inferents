use super::*;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::testing_env;

const ME: &str = "contract.testnet";
const AGENT1_PK: &str = "secp256k1:45pb29TbWpHXeX8VGmnpTxpgNBTn4pxQNBnJbcqbr15StuLhmPMhQcBxuek7VeSLjCQR6tn2iQdBB6HnQgmL8vh3";
const MINT_MSG: &str = r#"{"action":"mint","max_leverage":3}"#;
const T0: u64 = 1_000_000_000_000_000_000;

fn acct(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn ctx(who: &str, ts: u64) {
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(acct(ME))
            .predecessor_account_id(acct(who))
            .block_timestamp(ts)
            .attached_deposit(NearToken::from_yoctonear(1))
            .build()
    );
}

fn setup() -> Contract {
    ctx("owner.testnet", T0);
    let mut c = Contract::new(acct("owner.testnet"), acct("usdc.testnet"), acct("v1.signer-prod.testnet"), acct("runner.testnet"), 500, false);
    c.set_min_mint(U64(1_000_000));
    c.set_epoch_len(U64(300_000_000_000));
    c
}

fn minted(c: &mut Contract, amount: u128) -> String {
    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct("alice.testnet"), U128(amount), MINT_MSG.into());
    let id = c.next_id.to_string();
    ctx(ME, T0);
    c.on_hl_address(id.clone(), U128(amount), Ok(AGENT1_PK.into()));
    id
}

fn invariant(c: &Contract) {
    let a = c.accounting();
    assert!(a.reserve_usdc.0 >= a.total_credits.0 + a.treasury_usdc.0, "reserve below credits + treasury");
}

#[test]
fn mint_derives_the_hyperliquid_address_on_chain() {
    let mut c = setup();
    let id = minted(&mut c, 20_000_000);
    let a = c.agent(id.clone()).unwrap();
    assert_eq!(a.hl_address, "0xfeef85ae49ad379e54ebd1fa66ffeb4ab3fea446");
    assert_eq!(a.credits_micro_usd.0, 20_000_000);
    assert_eq!(a.owner, acct("alice.testnet"));
    assert!(a.needs_rotation && a.agent_address.is_none());
    invariant(&c);
}

#[test]
fn failed_address_derivation_rolls_the_mint_back() {
    let mut c = setup();
    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct("alice.testnet"), U128(5_000_000), MINT_MSG.into());
    ctx(ME, T0);
    let unused = c.on_hl_address("1".into(), U128(5_000_000), Err(PromiseError::Failed));
    assert_eq!(unused.0, 5_000_000);
    assert!(c.agent("1".into()).is_none());
    assert_eq!(c.accounting().reserve_usdc.0, 0);
}

#[test]
#[should_panic(expected = "wrong token")]
fn deposits_from_other_tokens_are_rejected() {
    let mut c = setup();
    ctx("fake.testnet", T0);
    let _ = c.ft_on_transfer(acct("alice.testnet"), U128(5_000_000), MINT_MSG.into());
}

#[test]
#[should_panic(expected = "below minimum load")]
fn mint_below_minimum_is_rejected() {
    let mut c = setup();
    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct("alice.testnet"), U128(500_000), MINT_MSG.into());
}

#[test]
fn load_adds_credits() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct("bob.testnet"), U128(3_000_000), format!(r#"{{"action":"load","token_id":"{id}"}}"#));
    assert_eq!(c.agent(id).unwrap().credits_micro_usd.0, 8_000_000);
    invariant(&c);
}

#[test]
fn usage_moves_credits_to_treasury_and_rejects_replays() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("runner.testnet", T0);
    c.report_usage(id.clone(), vec!["r1".into()], U64(60_300));
    let a = c.agent(id.clone()).unwrap();
    assert_eq!(a.credits_micro_usd.0, 5_000_000 - 60_300);
    assert_eq!(c.accounting().treasury_usdc.0, 60_300);
    invariant(&c);
    let replay = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut c2 = setup();
        let id2 = minted(&mut c2, 5_000_000);
        ctx("runner.testnet", T0);
        c2.report_usage(id2.clone(), vec!["r1".into()], U64(1));
        c2.report_usage(id2, vec!["r1".into()], U64(1));
    }));
    assert!(replay.is_err());
}

#[test]
fn usage_overshoot_is_clamped_to_remaining_credits() {
    let mut c = setup();
    let id = minted(&mut c, 1_000_000);
    ctx("runner.testnet", T0);
    c.report_usage(id.clone(), vec!["a".into()], U64(900_000));
    c.report_usage(id.clone(), vec!["b".into()], U64(900_000));
    assert_eq!(c.agent(id).unwrap().credits_micro_usd.0, 0);
    assert_eq!(c.accounting().treasury_usdc.0, 1_000_000);
    invariant(&c);
}

#[test]
#[should_panic(expected = "cost above per-request cap")]
fn usage_above_the_per_request_cap_is_rejected() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("runner.testnet", T0);
    c.report_usage(id, vec!["a".into()], U64(MAX_COST_PER_REQUEST_MICRO_USD + 1));
}

#[test]
#[should_panic(expected = "runner only")]
fn only_the_runner_reports_usage() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("alice.testnet", T0);
    c.report_usage(id, vec!["a".into()], U64(1));
}

#[test]
#[should_panic(expected = "epoch not over")]
fn margin_cannot_be_released_mid_epoch() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("runner.testnet", T0 + 1);
    let _ = c.release_margin(id, "deadbeef".repeat(8));
}

#[test]
fn release_margin_burns_unused_credits_and_starts_a_new_epoch() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("runner.testnet", T0);
    c.report_usage(id.clone(), vec!["a".into()], U64(100_000));
    let after_epoch = T0 + 301_000_000_000;
    ctx("runner.testnet", after_epoch);
    let _ = c.release_margin(id.clone(), "deadbeef".repeat(8));
    let a = c.agent(id.clone()).unwrap();
    assert_eq!(a.credits_micro_usd.0, 0);
    assert_eq!(a.epoch_end_ns.0, after_epoch + 300_000_000_000);
    assert_eq!(c.accounting().reserve_usdc.0, 100_000);
    invariant(&c);

    ctx(ME, after_epoch);
    c.on_margin_released(id.clone(), U64(4_900_000), Err(PromiseError::Failed));
    assert_eq!(c.agent(id).unwrap().credits_micro_usd.0, 4_900_000);
    invariant(&c);
}

#[test]
fn transfer_flags_key_rotation_and_royalty_goes_to_the_minter() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("alice.testnet", T0);
    c.agent_mut(&id).needs_rotation = false;
    c.nft_transfer(acct("bob.testnet"), id.clone(), None, None);
    let a = c.agent(id.clone()).unwrap();
    assert_eq!(a.owner, acct("bob.testnet"));
    assert!(a.needs_rotation);
    assert_eq!(a.minter, acct("alice.testnet"));

    let p = c.nft_payout(id.clone(), U128(1_000), Some(2)).payout;
    assert_eq!(p[&acct("alice.testnet")].0, 50);
    assert_eq!(p[&acct("bob.testnet")].0, 950);
    assert_eq!(c.nft_tokens_for_owner(acct("bob.testnet"), None, None).len(), 1);
    assert_eq!(c.nft_tokens_for_owner(acct("alice.testnet"), None, None).len(), 0);
}

#[test]
#[should_panic(expected = "NFT owner only")]
fn only_the_owner_can_transfer() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("mallory.testnet", T0);
    c.nft_transfer(acct("mallory.testnet"), id, None, None);
}

#[test]
fn viewing_keys_grant_time_limited_access_and_route_revenue() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("alice.testnet", T0);
    c.set_key_terms(id.clone(), U64(1_000_000), U64(1_000), RevenueTarget::Margin);
    assert!(!c.has_access(id.clone(), acct("bob.testnet")));
    assert!(c.has_access(id.clone(), acct("alice.testnet")));

    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct("bob.testnet"), U128(1_000_000), format!(r#"{{"action":"buy_key","token_id":"{id}"}}"#));
    assert_eq!(c.agent(id.clone()).unwrap().margin_micro_usd.0, 1_000_000);
    ctx("alice.testnet", T0 + 999);
    assert!(c.has_access(id.clone(), acct("bob.testnet")));
    ctx("alice.testnet", T0 + 1_001);
    assert!(!c.has_access(id, acct("bob.testnet")));
    invariant(&c);
}

#[test]
#[should_panic(expected = "below key price")]
fn viewing_key_below_price_is_rejected() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("alice.testnet", T0);
    c.set_key_terms(id.clone(), U64(1_000_000), U64(1_000), RevenueTarget::Credits);
    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct("bob.testnet"), U128(999_999), format!(r#"{{"action":"buy_key","token_id":"{id}"}}"#));
}

mod receipts {
    use super::*;

    #[test]
    fn real_near_ai_receipt_verifies() {
        let mut c = setup();
        let id = minted(&mut c, 5_000_000);
        let fx: Value = serde_json::from_str(include_str!("../../runner/fixtures/receipt1.json")).unwrap();
        ctx("owner.testnet", T0);
        c.set_signing_address(fx["model"].as_str().unwrap().into(), fx["signing_address"].as_str().unwrap().into());
        ctx("runner.testnet", T0);
        let r = Receipt {
            token_id: id,
            order_id: "o1".into(),
            request_id: fx["request_id"].as_str().unwrap().into(),
            model: fx["model"].as_str().unwrap().into(),
            request_hash: fx["request_hash"].as_str().unwrap().into(),
            response_hash: fx["response_hash"].as_str().unwrap().into(),
            signature: fx["signature"].as_str().unwrap().into(),
            verified: false,
        };
        c.submit_receipt(r.clone());
        assert!(c.receipts(0, 10)[0].verified);

        let mut tampered = r;
        tampered.response_hash = "0".repeat(64);
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.submit_receipt(tampered)));
        assert!(rejected.is_err());
    }
}
