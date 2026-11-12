use super::*;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::testing_env;

const ME: &str = "contract.testnet";
const AGENT1_PK: &str = "secp256k1:45pb29TbWpHXeX8VGmnpTxpgNBTn4pxQNBnJbcqbr15StuLhmPMhQcBxuek7VeSLjCQR6tn2iQdBB6HnQgmL8vh3";
const MODEL: &str = "test/model";
const GATEWAY_MODEL: &str = "test/gateway-model";
const HASH: &str = "11111111111111111111111111111111111111111111111111111111111111aa";

fn mint_msg(owner: &str) -> String {
    format!(r#"{{"action":"mint","owner":"{owner}","max_leverage":3,"credits_bps":10000,"model":"{MODEL}"}}"#)
}
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
    c.set_signing_address(MODEL.into(), "0x2665cce942fa427ef30d87f4b0bff67fb7645462".into());
    c.set_gateway_model(GATEWAY_MODEL.into(), true);
    c
}

fn minted(c: &mut Contract, amount: u128) -> String {
    minted_by(c, "alice.testnet", amount)
}

fn minted_by(c: &mut Contract, owner: &str, amount: u128) -> String {
    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct(owner), U128(amount), mint_msg(owner));
    let id = c.next_id.to_string();
    ctx(ME, T0);
    c.on_hl_address(id.clone(), U128(amount), Ok(AGENT1_PK.into()));
    id
}

fn invariant(c: &Contract) {
    let a = c.accounting();
    assert!(a.reserve_usdc.0 >= a.total_credits.0 + a.treasury_usdc.0 + a.refundable_usdc.0, "reserve below liabilities");
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
fn mint_splits_the_deposit_into_credits_and_margin_and_margin_is_funded_at_once() {
    let mut c = setup();
    ctx("usdc.testnet", T0);
    let msg = format!(r#"{{"action":"mint","owner":"alice.testnet","max_leverage":3,"model":"{MODEL}"}}"#);
    let _ = c.ft_on_transfer(acct("alice.testnet"), U128(20_000_000), msg);
    ctx(ME, T0);
    c.on_hl_address("1".into(), U128(20_000_000), Ok(AGENT1_PK.into()));
    let a = c.agent("1".into()).unwrap();
    assert_eq!((a.credits_micro_usd.0, a.margin_micro_usd.0), (2_000_000, 18_000_000));
    invariant(&c);

    ctx("runner.testnet", T0 + 1);
    let _ = c.fund_margin("1".into(), "deadbeef".repeat(8));
    let a = c.agent("1".into()).unwrap();
    assert_eq!((a.credits_micro_usd.0, a.margin_micro_usd.0), (2_000_000, 0));
    assert_eq!(c.accounting().reserve_usdc.0, 2_000_000);
    invariant(&c);

    ctx(ME, T0 + 1);
    c.on_margin_funded("1".into(), U64(18_000_000), Err(PromiseError::Failed));
    assert_eq!(c.agent("1".into()).unwrap().margin_micro_usd.0, 18_000_000);
    invariant(&c);
}

#[test]
fn failed_address_derivation_rolls_the_mint_back() {
    let mut c = setup();
    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct("alice.testnet"), U128(5_000_000), mint_msg("alice.testnet"));
    ctx(ME, T0);
    assert_eq!(c.on_hl_address("1".into(), U128(5_000_000), Err(PromiseError::Failed)).0, 0);
    assert!(c.agent("1".into()).is_none());
    assert_eq!(c.balance_of(acct("alice.testnet")).0, 5_000_000);
    invariant(&c);
}

#[test]
#[should_panic(expected = "wrong token")]
fn deposits_from_other_tokens_are_rejected() {
    let mut c = setup();
    ctx("fake.testnet", T0);
    let _ = c.ft_on_transfer(acct("alice.testnet"), U128(5_000_000), mint_msg("alice.testnet"));
}

#[test]
fn mint_below_minimum_is_held_as_a_refundable_balance() {
    let mut c = setup();
    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct("bob.testnet"), U128(500_000), mint_msg("alice.testnet"));
    assert_eq!(c.agents(0, 10).len(), 0);
    assert_eq!(c.balance_of(acct("alice.testnet")).0, 500_000);
    invariant(&c);
}

#[test]
fn load_adds_credits() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct("bob.testnet"), U128(3_000_000), format!(r#"{{"action":"load","owner":"bob.testnet","token_id":"{id}"}}"#));
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
    let _ = c.ft_on_transfer(acct("bob.testnet"), U128(1_000_000), format!(r#"{{"action":"buy_key","owner":"bob.testnet","token_id":"{id}"}}"#));
    assert_eq!(c.agent(id.clone()).unwrap().margin_micro_usd.0, 1_000_000);
    ctx("alice.testnet", T0 + 999);
    assert!(c.has_access(id.clone(), acct("bob.testnet")));
    ctx("alice.testnet", T0 + 1_001);
    assert!(!c.has_access(id, acct("bob.testnet")));
    invariant(&c);
}

#[test]
fn viewing_key_below_price_is_held_for_the_named_owner() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    ctx("alice.testnet", T0);
    c.set_key_terms(id.clone(), U64(1_000_000), U64(1_000), RevenueTarget::Credits);
    ctx("usdc.testnet", T0);
    let _ = c.ft_on_transfer(acct("bob.testnet"), U128(999_999), format!(r#"{{"action":"buy_key","owner":"carol.testnet","token_id":"{id}"}}"#));
    assert_eq!(c.balance_of(acct("carol.testnet")).0, 999_999);
    assert!(!c.has_access(id, acct("carol.testnet")));
    invariant(&c);
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
        c.report_usage(id.clone(), vec![fx["request_id"].as_str().unwrap().into()], U64(60));
        let r = Receipt {
            token_id: id,
            order_id: "o1".into(),
            request_id: fx["request_id"].as_str().unwrap().into(),
            model: fx["model"].as_str().unwrap().into(),
            request_hash: fx["request_hash"].as_str().unwrap().into(),
            response_hash: fx["response_hash"].as_str().unwrap().into(),
            signature: fx["signature"].as_str().unwrap().into(),
            verified: false,
            kind: ReceiptKind::ProviderTee,
        };
        c.submit_receipt(r.clone());
        assert!(c.receipts(0, 10)[0].verified);
        let twice = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.submit_receipt(r.clone())));
        assert!(twice.is_err(), "the same receipt cannot be stored twice");

        let mut tampered = r;
        tampered.response_hash = "0".repeat(64);
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.submit_receipt(tampered)));
        assert!(rejected.is_err());
    }

    #[test]
    fn real_gateway_receipt_verifies_and_the_request_body_names_the_model() {
        let mut c = setup();
        let id = minted(&mut c, 5_000_000);
        let fx: Value = serde_json::from_str(include_str!("../../runner/fixtures/receipt_gateway.json")).unwrap();
        let s = |k: &str| fx[k].as_str().unwrap().to_owned();
        let r = Receipt {
            token_id: id,
            order_id: "o1".into(),
            request_id: s("request_id"),
            model: s("model"),
            request_hash: s("request_hash"),
            response_hash: s("response_hash"),
            signature: s("signature"),
            verified: false,
            kind: ReceiptKind::Gateway,
        };

        ctx("runner.testnet", T0);
        let unset = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut c2 = setup();
            let id2 = minted(&mut c2, 5_000_000);
            ctx("runner.testnet", T0);
            c2.submit_receipt(Receipt { token_id: id2, ..r.clone() });
        }));
        assert!(unset.is_err(), "gateway receipts need the gateway key");

        ctx("owner.testnet", T0);
        c.set_gateway_signing_address(s("signing_address"));
        ctx("runner.testnet", T0);
        let unbilled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.submit_receipt(r.clone())));
        assert!(unbilled.is_err(), "a receipt needs a charged request");
        c.report_usage(r.token_id.clone(), vec![r.request_id.clone()], U64(60));
        c.submit_receipt(r.clone());
        let stored = &c.receipts(0, 10)[0];
        assert!(stored.verified && stored.kind == ReceiptKind::Gateway);

        let body = s("request_body");
        assert_eq!(hex::encode(env::sha256_array(body.as_bytes())), r.request_hash);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["model"], r.model.as_str());

        let as_model = Receipt { kind: ReceiptKind::ProviderTee, ..r };
        ctx("owner.testnet", T0);
        c.set_signing_address(as_model.model.clone(), s("signing_address"));
        ctx("runner.testnet", T0);
        let wrong_kind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.submit_receipt(as_model)));
        assert!(wrong_kind.is_err(), "a gateway signature must not pass as a model signature");
    }

    #[test]
    fn receipts_stored_before_kind_existed_still_decode_as_provider_tee() {
        use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
        #[derive(BorshSerialize)]
        #[borsh(crate = "near_sdk::borsh")]
        struct Old(String, String, String, String, String, String, String, bool);
        let old = Old("1".into(), "o".into(), "r".into(), "m".into(), "q".into(), "p".into(), "0x".into(), true);
        let r = Receipt::try_from_slice(&near_sdk::borsh::to_vec(&old).unwrap()).unwrap();
        assert_eq!(r.kind, ReceiptKind::ProviderTee);
        assert!(r.verified);
    }
}

mod arena {
    use super::*;

    fn hold_msg(c: &mut Contract, sender: &str, amount: u128, msg: &str) {
        ctx("usdc.testnet", T0 + 10_000_000_000);
        let _ = c.ft_on_transfer(acct(sender), U128(amount), msg.to_owned());
    }

    fn priced(c: &mut Contract, id: &str, equity: u64, pnl: i64) {
        ctx("runner.testnet", T0);
        c.report_equity(id.into(), U64(equity), U64(T0));
        c.agent_mut(id).pnl = pnl;
    }

    #[test]
    fn mint_commits_the_strategy_hash_and_model_and_exposes_them_in_nft_metadata() {
        let mut c = setup();
        ctx("usdc.testnet", T0);
        let msg = format!(r#"{{"action":"mint","owner":"alice.testnet","max_leverage":3,"model":"{GATEWAY_MODEL}","strategy_hash":"{HASH}"}}"#);
        let _ = c.ft_on_transfer(acct("relay.testnet"), U128(20_000_000), msg);
        let a = c.agent("1".into()).unwrap();
        assert_eq!((a.strategy_hash.as_str(), a.model.as_str(), a.owner), (HASH, GATEWAY_MODEL, acct("alice.testnet")));
        let extra = c.nft_token("1".into()).unwrap().metadata.extra;
        assert!(extra.contains(HASH) && extra.contains(GATEWAY_MODEL));
    }

    #[test]
    fn unregistered_model_or_bad_hash_is_held_not_refused() {
        let mut c = setup();
        hold_msg(&mut c, "relay.testnet", 5_000_000, r#"{"action":"mint","owner":"alice.testnet","max_leverage":3,"model":"nobody/model"}"#);
        let bad_hash = format!(r#"{{"action":"mint","owner":"alice.testnet","max_leverage":3,"model":"{MODEL}","strategy_hash":"zz"}}"#);
        hold_msg(&mut c, "relay.testnet", 2_000_000, &bad_hash);
        assert_eq!(c.agents(0, 10).len(), 0);
        assert_eq!(c.balance_of(acct("alice.testnet")).0, 7_000_000);
        invariant(&c);
    }

    #[test]
    fn unparseable_messages_are_held_for_the_sender() {
        let mut c = setup();
        hold_msg(&mut c, "bob.testnet", 3_000_000, "not json");
        assert_eq!(c.balance_of(acct("bob.testnet")).0, 3_000_000);
        invariant(&c);
    }

    #[test]
    fn withdraw_balance_pays_out_and_a_failed_transfer_restores_it() {
        let mut c = setup();
        hold_msg(&mut c, "bob.testnet", 3_000_000, "not json");
        ctx("bob.testnet", T0);
        let _ = c.withdraw_balance();
        assert_eq!(c.balance_of(acct("bob.testnet")).0, 0);
        assert_eq!(c.accounting().reserve_usdc.0, 0);
        ctx(ME, T0);
        c.on_balance_withdrawn(acct("bob.testnet"), U128(3_000_000), Err(PromiseError::Failed));
        assert_eq!(c.balance_of(acct("bob.testnet")).0, 3_000_000);
        invariant(&c);
    }

    #[test]
    #[should_panic(expected = "runner only")]
    fn only_the_runner_reports_equity() {
        let mut c = setup();
        let id = minted(&mut c, 5_000_000);
        ctx("alice.testnet", T0);
        c.report_equity(id, U64(1), U64(T0));
    }

    #[test]
    fn price_is_equity_plus_credits_plus_margin_plus_a_premium_on_positive_pnl() {
        let mut c = setup();
        let id = minted(&mut c, 20_000_000);
        priced(&mut c, &id, 5_000_000, 1_000_000);
        assert_eq!(c.price(id.clone()).unwrap().0, 5_000_000 + 20_000_000 + 200_000);
        c.agent_mut(&id).pnl = -3_000_000;
        assert_eq!(c.price(id).unwrap().0, 25_000_000);
    }

    #[test]
    fn buying_pays_the_seller_less_the_royalty_pays_the_minter_and_transfers_the_agent() {
        let mut c = setup();
        let id = minted(&mut c, 20_000_000);
        ctx("alice.testnet", T0);
        c.nft_transfer(acct("bob.testnet"), id.clone(), None, None);
        c.agent_mut(&id).needs_rotation = false;
        priced(&mut c, &id, 5_000_000, 1_000_000);

        let msg = format!(r#"{{"action":"buy","owner":"carol.testnet","token_id":"{id}"}}"#);
        hold_msg(&mut c, "relay.testnet", 26_000_000, &msg);
        let a = c.agent(id.clone()).unwrap();
        assert_eq!(a.owner, acct("carol.testnet"));
        assert!(a.needs_rotation);
        assert_eq!(c.balance_of(acct("bob.testnet")).0, 23_940_000);
        assert_eq!(c.balance_of(acct("alice.testnet")).0, 1_260_000);
        assert_eq!(c.balance_of(acct("carol.testnet")).0, 800_000);
        invariant(&c);

        hold_msg(&mut c, "relay.testnet", 26_000_000, &msg);
        assert_eq!(c.balance_of(acct("carol.testnet")).0, 800_000 + 26_000_000, "a second buy needs a fresh equity report");
        invariant(&c);
    }

    #[test]
    fn buying_with_a_stale_report_or_too_little_is_held_for_the_buyer() {
        let mut c = setup();
        let id = minted(&mut c, 20_000_000);
        priced(&mut c, &id, 5_000_000, 0);
        let msg = format!(r#"{{"action":"buy","owner":"carol.testnet","token_id":"{id}"}}"#);

        hold_msg(&mut c, "carol.testnet", 24_999_999, &msg);
        assert_eq!(c.agent(id.clone()).unwrap().owner, acct("alice.testnet"));
        assert_eq!(c.balance_of(acct("carol.testnet")).0, 24_999_999);

        ctx("usdc.testnet", T0 + 121_000_000_000);
        let _ = c.ft_on_transfer(acct("carol.testnet"), U128(30_000_000), msg);
        assert_eq!(c.agent(id).unwrap().owner, acct("alice.testnet"));
        assert_eq!(c.balance_of(acct("carol.testnet")).0, 24_999_999 + 30_000_000);
        invariant(&c);
    }

    #[test]
    fn an_agent_buys_an_agent_from_its_credits_within_the_owners_budget() {
        let mut c = setup();
        let target = minted_by(&mut c, "alice.testnet", 1_000_000);
        let buyer = minted_by(&mut c, "bob.testnet", 20_000_000);
        priced(&mut c, &target, 2_000_000, 0);

        ctx("bob.testnet", T0);
        c.set_acquisition_budget(buyer.clone(), U64(3_000_000));
        ctx("runner.testnet", T0 + 1);
        c.buy_with_credits(buyer.clone(), target.clone());

        let (t, b) = (c.agent(target.clone()).unwrap(), c.agent(buyer.clone()).unwrap());
        assert_eq!((t.owner, t.acquired_by.as_deref(), t.needs_rotation), (acct("bob.testnet"), Some(buyer.as_str()), true));
        assert_eq!((b.credits_micro_usd.0, b.acquisition_budget_micro_usd.0), (17_000_000, 0));
        assert_eq!(c.balance_of(acct("alice.testnet")).0, 3_000_000);
        invariant(&c);
    }

    #[test]
    #[should_panic(expected = "price above the acquisition budget")]
    fn an_agent_cannot_spend_past_the_acquisition_budget() {
        let mut c = setup();
        let target = minted_by(&mut c, "alice.testnet", 1_000_000);
        let buyer = minted_by(&mut c, "bob.testnet", 20_000_000);
        priced(&mut c, &target, 2_000_000, 0);
        ctx("bob.testnet", T0);
        c.set_acquisition_budget(buyer.clone(), U64(2_999_999));
        ctx("runner.testnet", T0 + 1);
        c.buy_with_credits(buyer, target);
    }

    #[test]
    #[should_panic(expected = "runner only")]
    fn only_the_runner_can_spend_an_agents_credits_on_an_acquisition() {
        let mut c = setup();
        let target = minted_by(&mut c, "alice.testnet", 1_000_000);
        let buyer = minted_by(&mut c, "bob.testnet", 20_000_000);
        ctx("bob.testnet", T0);
        c.buy_with_credits(buyer, target);
    }
}

#[test]
fn migration_carries_old_agents_forward_with_defaults() {
    ctx(ME, T0);
    let mut agents: IterableMap<String, OldAgent> = IterableMap::new(b"a");
    agents.insert(
        "1".into(),
        OldAgent {
            minter: acct("alice.testnet"),
            owner: acct("bob.testnet"),
            credits: 7,
            margin: 3,
            epoch_len_ns: 5,
            epoch_end_ns: 9,
            hl_address: "0xabc".into(),
            agent_address: None,
            needs_rotation: false,
            max_leverage: 3,
            flat_at_epoch_end: true,
            pnl: -4,
            key_price: 0,
            key_duration_ns: 1,
            key_revenue_to: RevenueTarget::Credits,
        },
    );
    let mut old = OldContract {
        owner: acct("owner.testnet"),
        mpc: acct("v1.signer-prod.testnet"),
        hl_chain: "Testnet".into(),
        runner: acct("runner.testnet"),
        signing_addresses: LookupMap::new(b"s"),
        receipts: Vector::new(b"r"),
        usdc: acct("usdc.testnet"),
        royalty_bps: 500,
        min_mint: 1,
        default_epoch_ns: 2,
        next_id: 1,
        agents,
        access: LookupMap::new(b"k"),
        seen_requests: LookupSet::new(b"q"),
        reserve: 10,
        treasury: 0,
    };
    old.agents.flush();
    env::state_write(&old);
    let c = Contract::migrate();
    let a = c.agent("1".into()).unwrap();
    assert_eq!((a.owner, a.credits_micro_usd.0, a.margin_micro_usd.0, a.pnl_micro_usd.0), (acct("bob.testnet"), 7, 3, -4));
    assert_eq!((a.spent_micro_usd.0, a.acquired_by, a.strategy_hash), (0, None, "0".repeat(64)));
    assert_eq!(c.accounting().reserve_usdc.0, 10);
}

#[test]
fn nft_media_points_at_the_live_card_once_a_base_is_set() {
    let mut c = setup();
    let id = minted(&mut c, 5_000_000);
    assert_eq!(c.nft_token(id.clone()).unwrap().metadata.media, None);
    ctx("owner.testnet", T0);
    c.set_media_base("https://api.example.com/".into());
    assert_eq!(c.nft_token(id.clone()).unwrap().metadata.media.as_deref(), Some(format!("https://api.example.com/nft/{id}.svg").as_str()));
    let bad = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.set_media_base("javascript:alert(1)".into())));
    assert!(bad.is_err());
    ctx("alice.testnet", T0);
    let not_owner = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.set_media_base("https://evil.example".into())));
    assert!(not_owner.is_err());
}
