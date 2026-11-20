#![cfg(test)]

extern crate std;

use super::*;
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Ledger as _},
    token, Address, Env,
};

#[contract]
struct MockSwapRouter;

#[contractimpl]
impl MockSwapRouter {
    pub fn swap_exact_tokens_for_tokens(
        env: Env,
        caller: Address,
        from_token: Address,
        to_token: Address,
        recipient: Address,
        amount: i128,
        _min_amount_out: i128,
        deadline: u64,
    ) -> i128 {
        let now = env.ledger().timestamp();
        if deadline < now
            || deadline > now.saturating_add(shared_types::SETTLEMENT_SWAP_DEADLINE_WINDOW_SECONDS)
        {
            panic!("invalid swap deadline");
        }
        let router = env.current_contract_address();
        token::Client::new(&env, &from_token).transfer_from(&router, &caller, &router, &amount);
        let amount_out = amount * 2;
        token::Client::new(&env, &to_token).transfer(&router, &recipient, &amount_out);
        amount_out
    }
}

#[test]
fn test_get_driver_preference_returns_none_when_unset() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let driver = Address::generate(&env);

    // No preference has been recorded yet, so the getter must return None.
    assert_eq!(client.get_driver_preference(&driver), None);
}

/// Issue #459: a driver records their preferred payout asset on-chain and the
/// getter reads it back.
#[test]
fn test_set_and_get_driver_preference() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let driver = Address::generate(&env);
    let to_token = Address::generate(&env);

    client.set_driver_preference(&driver, &to_token);

    assert_eq!(
        client.get_driver_preference(&driver),
        Some(to_token.clone())
    );
}

#[test]
fn test_set_driver_preference_extends_instance_ttl() {
    use soroban_sdk::testutils::Deployer as _;

    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let escrow = Address::generate(&env);
    let driver = Address::generate(&env);
    let to_token = Address::generate(&env);

    client.init(&admin, &escrow);
    env.ledger().with_mut(|ledger| {
        ledger.sequence_number += shared_types::ttl::LEDGER_TTL_EXTEND_TO - 10;
    });

    client.set_driver_preference(&driver, &to_token);

    let instance_ttl = env.deployer().get_contract_instance_ttl(&contract_id);
    assert!(
        instance_ttl >= shared_types::ttl::LEDGER_TTL_THRESHOLD,
        "set_driver_preference must extend the contract instance TTL (got {instance_ttl})"
    );
}

/// Preferences are per driver: setting one must not leak into another's record.
#[test]
fn test_driver_preference_is_scoped_per_driver() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let driver_a = Address::generate(&env);
    let driver_b = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.set_driver_preference(&driver_a, &token_a);
    client.set_driver_preference(&driver_b, &token_b);

    assert_eq!(client.get_driver_preference(&driver_a), Some(token_a));
    assert_eq!(client.get_driver_preference(&driver_b), Some(token_b));
}

/// Setting a preference is an overwrite, so a driver can change their mind.
#[test]
fn test_set_driver_preference_overwrites_previous_value() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let driver = Address::generate(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.set_driver_preference(&driver, &first);
    client.set_driver_preference(&driver, &second);

    assert_eq!(client.get_driver_preference(&driver), Some(second));
}

/// The preference write must be authorised by the driver whose record is
/// written, so a caller whose signature is not authorized cannot set it.
#[test]
fn test_set_driver_preference_requires_driver_auth() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let driver = Address::generate(&env);
    let to_token = Address::generate(&env);

    // No auth is mocked, so the driver's own signature is missing.
    let result = client.try_set_driver_preference(&driver, &to_token);
    assert!(result.is_err());

    assert_eq!(client.get_driver_preference(&driver), None);
}

/// Issue #588: set_escrow_contract requires admin auth and updates the escrow
/// address; get_escrow_contract reads it back.
#[test]
fn test_set_and_get_escrow_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let new_escrow = Address::generate(&env);

    // Initialize first so there is an admin.
    client.init(&admin, &new_escrow);

    // get_escrow_contract returns the address set at init.
    assert_eq!(client.get_escrow_contract(), new_escrow);

    // Update to a new escrow address.
    let newer_escrow = Address::generate(&env);
    client.set_escrow_contract(&admin, &newer_escrow);
    env.ledger().set_timestamp(
        env.ledger().timestamp() + shared_types::PEER_CONTRACT_CHANGE_TIMELOCK_SECONDS,
    );
    client.confirm_escrow_contract();
    assert_eq!(client.get_escrow_contract(), newer_escrow);
}

/// Issue #588: set_escrow_contract is restricted to admin.
#[test]
fn test_set_escrow_contract_requires_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    let new_escrow = Address::generate(&env);

    client.init(&admin, &new_escrow);

    let result = client.try_set_escrow_contract(&non_admin, &new_escrow);
    assert!(result.is_err());
}

// ── init / get_admin coverage ──────────────────────────────────────────────

/// init stores the admin and the initial escrow contract; get_admin and
/// get_escrow_contract read them back.
#[test]
fn test_init_stores_admin_and_escrow_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let escrow = Address::generate(&env);

    client.init(&admin, &escrow);

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_escrow_contract(), escrow);
}

/// A second init must be rejected with AlreadyInitialized so neither the
/// admin nor the escrow address can be silently overwritten.
#[test]
fn test_init_rejects_duplicate_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let escrow = Address::generate(&env);
    client.init(&admin, &escrow);

    let hijacker = Address::generate(&env);
    let hijacked_escrow = Address::generate(&env);
    let result = client.try_init(&hijacker, &hijacked_escrow);
    match result {
        Err(Ok(err)) => assert_eq!(err, TrustchainError::AlreadyInitialized.into()),
        _ => panic!("Expected AlreadyInitialized on duplicate init"),
    }

    // The original state survived the attempted re-initialization.
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_escrow_contract(), escrow);
}

/// get_admin before init fails with NotInitialized instead of returning a
/// zeroed or garbage address.
#[test]
fn test_get_admin_before_init_returns_not_initialized() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let result = client.try_get_admin();
    match result {
        Err(Ok(err)) => assert_eq!(err, TrustchainError::NotInitialized.into()),
        _ => panic!("Expected NotInitialized from get_admin before init"),
    }
}

// ── execute_settlement_swap production guard ───────────────────────────────

/// Calling the stub before init fails with NotInitialized rather than
/// proceeding with no escrow contract configured.
#[test]
fn test_execute_settlement_swap_requires_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let caller = Address::generate(&env);
    let token = Address::generate(&env);
    let recipient = Address::generate(&env);

    let result =
        client.try_execute_settlement_swap(&caller, &1u64, &token, &token, &recipient, &100i128, &90i128, &300u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, TrustchainError::NotInitialized.into()),
        _ => panic!("Expected NotInitialized from un-initialized execute_settlement_swap"),
    }
}

/// Only the configured escrow contract may call execute_settlement_swap; any
/// other caller is rejected with Unauthorized before the stub guard fires.
///
/// Note: `TrustchainError::Unauthorized` and `SettlementError::SwapNotImplemented`
/// share wire discriminant #1 because this contract mixes the shared error
/// enum with its local one, so this assertion pins the discriminant rather
/// than being able to tell the two variants apart on the wire.
#[test]
fn test_execute_settlement_swap_rejects_non_escrow_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let escrow = Address::generate(&env);
    client.init(&admin, &escrow);

    let intruder = Address::generate(&env);
    let token = Address::generate(&env);
    let recipient = Address::generate(&env);

    let result = client
        .try_execute_settlement_swap(&intruder, &1u64, &token, &token, &recipient, &100i128, &90i128, &300u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, TrustchainError::Unauthorized.into()),
        _ => panic!("Expected Unauthorized for a non-escrow caller"),
    }
}

/// Even the authorized escrow caller is rejected when the swap parameters are
/// degenerate (identical input and output token).
#[test]
fn test_execute_settlement_swap_rejects_same_token_pair() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let escrow = Address::generate(&env);
    client.init(&admin, &escrow);

    let token = Address::generate(&env);
    let recipient = Address::generate(&env);

    let result = client
        .try_execute_settlement_swap(&escrow, &1u64, &token, &token, &recipient, &100i128, &90i128, &300u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, SettlementError::InvalidSwapParameters.into()),
        _ => panic!("Expected InvalidSwapParameters for an identical token pair"),
    }
}

/// Issue #589: clear_driver_preference removes the preference.
#[test]
fn test_clear_driver_preference_removes() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let driver = Address::generate(&env);
    let to_token = Address::generate(&env);

    client.set_driver_preference(&driver, &to_token);
    assert_eq!(client.get_driver_preference(&driver), Some(to_token.clone()));

    client.clear_driver_preference(&driver);
    assert_eq!(client.get_driver_preference(&driver), None);
}

#[test]
fn test_execute_settlement_swap_uses_configured_router() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let escrow = Address::generate(&env);
    let router = env.register_contract(None, MockSwapRouter);
    let input_admin = Address::generate(&env);
    let output_admin = Address::generate(&env);
    let from_token = env
        .register_stellar_asset_contract_v2(input_admin.clone())
        .address();
    let to_token = env
        .register_stellar_asset_contract_v2(output_admin.clone())
        .address();
    let recipient = Address::generate(&env);

    client.init(&admin, &escrow);
    client.set_swap_router(&admin, &router);
    token::StellarAssetClient::new(&env, &from_token).mint(&contract_id, &100);
    token::StellarAssetClient::new(&env, &to_token).mint(&router, &200);

    client.execute_settlement_swap(
        &escrow,
        &42u64,
        &from_token,
        &to_token,
        &recipient,
        &100,
        &190,
        &300,
    );

    assert_eq!(token::Client::new(&env, &to_token).balance(&recipient), 200);
    assert_eq!(token::Client::new(&env, &from_token).balance(&router), 100);
}

#[test]
fn test_execute_settlement_swap_rejects_deadline_beyond_window() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let escrow = Address::generate(&env);
    let from_token = Address::generate(&env);
    let to_token = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.init(&admin, &escrow);
    env.ledger().set_timestamp(100);

    let too_far = client.try_execute_settlement_swap(
        &escrow,
        &1u64,
        &from_token,
        &to_token,
        &recipient,
        &100,
        &90,
        &(100 + shared_types::SETTLEMENT_SWAP_DEADLINE_WINDOW_SECONDS + 1),
    );
    assert!(too_far.is_err());

    let expired = client.try_execute_settlement_swap(
        &escrow,
        &1u64,
        &from_token,
        &to_token,
        &recipient,
        &100,
        &90,
        &99,
    );
    assert!(expired.is_err());
}

// ── delivery_id correlation ────────────────────────────────────────────────

/// A caller other than the configured escrow cannot attribute a swap to an
/// escrow delivery_id: the identifier is only accepted from the escrow.
#[test]
fn test_execute_settlement_swap_rejects_delivery_id_from_non_escrow_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let escrow = Address::generate(&env);
    client.init(&admin, &escrow);

    let intruder = Address::generate(&env);
    let from_token = Address::generate(&env);
    let to_token = Address::generate(&env);
    let recipient = Address::generate(&env);

    let result = client.try_execute_settlement_swap(
        &intruder, &7u64, &from_token, &to_token, &recipient, &100i128, &90i128, &300u64,
    );
    match result {
        Err(Ok(err)) => assert_eq!(err, TrustchainError::Unauthorized.into()),
        _ => panic!("Expected Unauthorized for a non-escrow caller"),
    }
}

/// The escrow caller must authenticate; a forged `caller` argument without
/// the escrow's authorization is rejected.
#[test]
fn test_execute_settlement_swap_requires_escrow_auth() {
    let env = Env::default();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let escrow = Address::generate(&env);
    env.mock_all_auths();
    client.init(&admin, &escrow);
    env.set_auths(&[]);

    let from_token = Address::generate(&env);
    let to_token = Address::generate(&env);
    let recipient = Address::generate(&env);

    let result = client.try_execute_settlement_swap(
        &escrow, &7u64, &from_token, &to_token, &recipient, &100i128, &90i128, &300u64,
    );
    assert!(result.is_err());
}

// ── settlement_swap_executed event ─────────────────────────────────────────

/// Router whose delivered and reported output are set independently, so tests
/// can model honest, under-delivering and lying routers.
#[contract]
struct ConfigurableSwapRouter;

#[contractimpl]
impl ConfigurableSwapRouter {
    pub fn configure(env: Env, delivered: i128, reported: i128) {
        env.storage().instance().set(&soroban_sdk::symbol_short!("deliv"), &delivered);
        env.storage().instance().set(&soroban_sdk::symbol_short!("report"), &reported);
    }

    pub fn swap_exact_tokens_for_tokens(
        env: Env,
        caller: Address,
        from_token: Address,
        to_token: Address,
        recipient: Address,
        amount: i128,
        _min_amount_out: i128,
        _deadline: u64,
    ) -> i128 {
        let router = env.current_contract_address();
        token::Client::new(&env, &from_token).transfer_from(&router, &caller, &router, &amount);
        let delivered: i128 = env
            .storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("deliv"))
            .unwrap();
        if delivered > 0 {
            token::Client::new(&env, &to_token).transfer(&router, &recipient, &delivered);
        }
        env.storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("report"))
            .unwrap()
    }
}

struct SwapFixture {
    env: Env,
    client: SettlementContractClient<'static>,
    escrow: Address,
    from_token: Address,
    to_token: Address,
    recipient: Address,
}

fn swap_fixture(delivered: i128, reported: i128) -> SwapFixture {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, SettlementContract);
    let client = SettlementContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let escrow = Address::generate(&env);
    let router = env.register_contract(None, ConfigurableSwapRouter);
    ConfigurableSwapRouterClient::new(&env, &router).configure(&delivered, &reported);
    let from_token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let to_token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let recipient = Address::generate(&env);

    client.init(&admin, &escrow);
    client.set_swap_router(&admin, &router);
    token::StellarAssetClient::new(&env, &from_token).mint(&contract_id, &100);
    token::StellarAssetClient::new(&env, &to_token).mint(&router, &1_000);

    SwapFixture { env, client, escrow, from_token, to_token, recipient }
}

fn swap_events(env: &Env) -> std::vec::Vec<SettlementSwapExecutedEvent> {
    use soroban_sdk::{testutils::Events as _, xdr, TryFromVal, Val};
    let topic = shared_types::events::settlement_swap_executed(env);
    let mut out = std::vec::Vec::new();
    for raw in env.events().all().events() {
        let xdr::ContractEventBody::V0(body) = raw.body.clone();
        let Some(first) = body.topics.first() else { continue };
        let Ok(sym) = soroban_sdk::Symbol::try_from_val(env, first) else { continue };
        if sym != topic {
            continue;
        }
        let data = Val::try_from_val(env, &body.data).unwrap();
        out.push(SettlementSwapExecutedEvent::try_from_val(env, &data).unwrap());
    }
    out
}

#[test]
fn test_execute_settlement_swap_emits_event_with_measured_output() {
    let f = swap_fixture(180, 180);

    let returned =
        f.client
            .execute_settlement_swap(&f.escrow, &55u64, &f.from_token, &f.to_token, &f.recipient, &100, &150, &300);

    assert_eq!(returned, 180);
    let events = swap_events(&f.env);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0],
        SettlementSwapExecutedEvent {
            escrow_contract: f.escrow.clone(),
            delivery_id: 55,
            from_token: f.from_token.clone(),
            to_token: f.to_token.clone(),
            recipient: f.recipient.clone(),
            amount_in: 100,
            amount_out: 180,
        }
    );
    assert_eq!(token::Client::new(&f.env, &f.to_token).balance(&f.recipient), 180);
}

/// A router that reports a healthy output but under-delivers to the recipient
/// is rejected on the measured amount, and no settlement event is emitted.
#[test]
fn test_execute_settlement_swap_rejects_router_that_overreports_output() {
    let f = swap_fixture(100, 500);

    let result = f.client.try_execute_settlement_swap(
        &f.escrow, &56u64, &f.from_token, &f.to_token, &f.recipient, &100, &150, &300,
    );

    match result {
        Err(Ok(err)) => assert_eq!(err, SettlementError::SwapOutputBelowMinimum.into()),
        _ => panic!("Expected SwapOutputBelowMinimum for an under-delivering router"),
    }
    assert!(swap_events(&f.env).is_empty());
    assert_eq!(token::Client::new(&f.env, &f.to_token).balance(&f.recipient), 0);
}

/// Boundary: output exactly equal to `min_amount_out` is accepted and the
/// event reports that exact amount.
#[test]
fn test_execute_settlement_swap_accepts_output_equal_to_minimum() {
    let f = swap_fixture(150, 150);

    let returned = f.client.execute_settlement_swap(
        &f.escrow, &57u64, &f.from_token, &f.to_token, &f.recipient, &100, &150, &300,
    );

    assert_eq!(returned, 150);
    let events = swap_events(&f.env);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].amount_out, 150);
    assert_eq!(events[0].delivery_id, 57);
}
