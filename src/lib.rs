#![cfg_attr(not(feature = "std"), no_std)]
use concordium_cis2::{Cis2Event, *};
use concordium_std::{collections::BTreeMap, *};

/// The contract state.
#[derive(Serial, DeserialWithState, StateClone)]
#[concordium(state_parameter = "S")]
struct State<S: HasStateApi> {
    /// The state of the one player.
    player_data:        StateMap<Address, PlayerData, S>,
    /// Contract is paused/unpaused.
    paused:             bool,
}

#[derive(Debug, Serialize, SchemaType)]
struct UpgradeParams {
    /// The new module reference.
    module:  ModuleReference,
    /// Optional entrypoint to call in the new module after upgrade.
    migrate: Option<(OwnedEntrypointName, OwnedParameter)>,
}

/// The return type for the contract function `view`.
#[derive(Serialize, SchemaType)]
struct ReturnBasicState {
    /// The admin address can upgrade the contract, pause and unpause the
    /// contract, transfer the admin address to a new address, set
    /// implementors, and update the metadata URL in the contract.
    admin:        Address,
    /// Contract is paused if `paused = true` and unpaused if `paused = false`.
    paused:       bool,
    /// The metadata URL of the token.
    metadata_url: concordium_cis2::MetadataUrl,
}

/// The parameter type for the contract function `setMetadataUrl`.
#[derive(Serialize, SchemaType, Clone)]
struct SetMetadataUrlParams {
    /// The URL following the specification RFC1738.
    url:  String,
    /// The hash of the document stored at the above URL.
    hash: Option<Sha256>,
}

/// The parameter type for the contract function `setPaused`.
#[derive(Serialize, SchemaType)]
#[repr(transparent)]
struct SetPausedParams {
    /// Contract is paused if `paused = true` and unpaused if `paused = false`.
    paused: bool,
}

/// A NewAdminEvent introduced by this smart contract.
#[derive(Serial, SchemaType)]
#[repr(transparent)]
struct NewAdminEvent {
    /// New admin address.
    new_admin: Address,
}

/// Contract errors
#[derive(Debug, PartialEq, Eq, Reject, Serial, SchemaType)]
enum CustomContractError {
    /// Failed parsing the parameter.
    #[from(ParseError)]
    ParseParamsError,
    /// Your error
    /// Failed logging: Log is full.
    LogFull,
    /// Failed logging: Log is malformed.
    LogMalformed,
    /// Failed to invoke a contract.
    InvokeContractError,
    /// Contract already initialized.
    AlreadyInitialized,
    /// Contract not initialized.
    UnInitialized,
    /// Upgrade failed because the new module does not exist.
    FailedUpgradeMissingModule,
    /// Upgrade failed because the new module does not contain a contract with a
    /// matching name.
    FailedUpgradeMissingContract,
    /// Upgrade failed because the smart contract version of the module is not
    /// supported.
    FailedUpgradeUnsupportedModuleVersion,
}

type ContractResult<A> = Result<A, CustomContractError>;

/// Mapping the logging errors to ContractError.
impl From<LogError> for CustomContractError {
    fn from(le: LogError) -> Self {
        match le {
            LogError::Full => Self::LogFull,
            LogError::Malformed => Self::LogMalformed,
        }
    }
}

/// Mapping errors related to contract invocations to CustomContractError.
impl<T> From<CallContractError<T>> for CustomContractError {
    fn from(_cce: CallContractError<T>) -> Self { Self::InvokeContractError }
}

/// Mapping errors related to contract upgrades to CustomContractError.
impl From<UpgradeError> for CustomContractError {
    #[inline(always)]
    fn from(ue: UpgradeError) -> Self {
        match ue {
            UpgradeError::MissingModule => Self::FailedUpgradeMissingModule,
            UpgradeError::MissingContract => Self::FailedUpgradeMissingContract,
            UpgradeError::UnsupportedModuleVersion => Self::FailedUpgradeUnsupportedModuleVersion,
        }
    }
}

/// Mapping CustomContractError to ContractError
impl From<CustomContractError> for ContractError {
    fn from(c: CustomContractError) -> Self { Cis2Error::Custom(c) }
}

impl<S: HasStateApi> State<S> {
    /// Creates the new state of the `state` contract with no one having any
    /// data by default. The ProtocolAddressesState is uninitialized.
    /// The ProtocolAddressesState has to be set with the `initialize`
    /// function after the `proxy` contract is deployed.
    fn new(state_builder: &mut StateBuilder<S>) -> Self {
        // Setup state.
        State {
            protocol_addresses: ProtocolAddressesState::UnInitialized,
            player_data:        state_builder.new_map(),
            paused:             false,
        }
    }
}

// Contract functions

/// Init function that creates a new smart contract.
#[init(contract = "Versus-League-Manager")]
fn contract_init<S: HasStateApi>(
    _ctx: &impl HasInitContext,
    state_builder: &mut StateBuilder<S>,
) -> InitResult<State<S>> {
    // Construct the initial contract state.
    let state = State::new(state_builder);

    Ok(state)
}

/// Update player state.
#[receive(
    contract = "Versus-League-Manager",
    name = "updatePlayerState",
    parameter = "UpdatePlayerStateParams",
    error = "CustomContractError",
    mutable
)]
fn contract_update_player_state<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    let (_proxy_address, implementation_address) = get_protocol_addresses_from_state(host)?;

    // Only implementation can set state.
    only_implementation(implementation_address, ctx.sender())?;

    // update player state.
    let params: UpdatePlayerStateParams = ctx.parameter_cursor().get()?;
    let state = host.state();

    let mut player_data = state.player_data.entry(params.player).or_insert_with(|| PlayerData {
        state:   PlayerState::NotAdded,
        result:  BattleResult::NoResult,
    });
    player_data.state = params.state;

    // host.state_mut().player_data.entry(params.player).and_modify(|player_data| {
    //     player_data.state = params.state
    // })

    Ok(())
}

/// Update player battle result.
#[receive(
    contract = "Versus-League-Manager",
    name = "updateBattleResult",
    parameter = "UpdateBattleResultParams",
    error = "CustomContractError",
    mutable
)]
fn contract_update_battle_result<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    let (_proxy_address, implementation_address) = get_protocol_addresses_from_state(host)?;

    // Only implementation can set result.
    only_implementation(implementation_address, ctx.sender())?;

    // update player state.
    let params: UpdateBattleResultParams = ctx.parameter_cursor().get()?;
    let state = host.state();

    let mut player_data = state.player_data.entry(params.player).or_insert_with(|| PlayerData {
        state:   PlayerState::NotAdded,
        result:  BattleResult::NoResult,
    });
    player_data.result = params.result;

    // host.state_mut().player_data.entry(params.player).and_modify(|player_data| {
    //     player_data.result = params.result
    // })

    Ok(())
}

/// Add new player with concordium id.
#[receive(
    contract = "Versus-League-Manager",
    name = "addPlayer",
    parameter = "Address",
    error = "CustomContractError",
    mutable
)]
fn contract_state_set_player_data<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    let (_proxy_address, implementation_address) = get_protocol_addresses_from_state(host)?;

    // Only implementation can set result.
    only_implementation(implementation_address, ctx.sender())?;

    // add new player.
    let params: Address = ctx.parameter_cursor().get()?;
    let state = host.state();

    state.player_data.entry(params).or_insert_with(|| PlayerData {
        state:   PlayerState::NotAdded,
        result:  BattleResult::NoResult,
    });

    Ok(())
}

/// Get paused.
#[receive(
    contract = "Versus-League-Manager",
    name = "getPaused",
    return_value = "bool",
    error = "CustomContractError"
)]
fn contract_state_get_paused<S: HasStateApi>(
    _ctx: &impl HasReceiveContext,
    host: &impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<bool> {
    Ok(host.state().paused)
}

/// Get player data.
#[receive(
    contract = "Versus-League-Manager",
    name = "getPlayerData",
    parameter = "Address",
    return_value = "(PlayerState, BattleResult)",
    error = "CustomContractError"
)]
fn contract_state_get_player_data<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<(PlayerState, BattleResult)> {
    let params: Address = ctx.parameter_cursor().get()?;
    
    let player = host.state().player_data.get(&params).unwrap();

    Ok((player.state, player.result))
}

/// Check if player is added.
#[receive(
    contract = "Versus-League-Manager",
    name = "isAdded",
    parameter = "Address",
    return_value = "bool",
    error = "CustomContractError"
)]
fn contract_state_is_added<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<bool> {
    let params: Address = ctx.parameter_cursor().get()?;
    
    let player_state = host.state().player_data.get(&params).unwrap().state;

    Ok(player_state != PlayerState::NotAdded)
}

/// Function to view state of the state contract.
#[receive(
    contract = "Versus-League-Manager",
    name = "view",
    return_value = "ReturnBasicState",
    error = "CustomContractError"
)]
fn contract_state_view<S: HasStateApi>(
    _ctx: &impl HasReceiveContext,
    host: &impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<ReturnBasicState> {
    let (proxy_address, implementation_address) = get_protocol_addresses_from_state(host)?;

    let state = ReturnBasicState {
        proxy_address,
        implementation_address,
        paused: host.state().paused,
    };
    Ok(state)
}

/// Get the supported standards or addresses for a implementation given list of
/// standard identifiers.
///
/// It rejects if:
/// - It fails to parse the parameter.
#[receive(
    contract = "Versus-League-Manager",
    name = "supports",
    parameter = "SupportsQueryParams",
    return_value = "SupportsQueryResponse",
    error = "ContractError"
)]
fn contract_supports<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<SupportsQueryResponse> {
    // Parse the parameter.
    let params: SupportsQueryParams = ctx.parameter_cursor().get()?;

    // Build the response.
    let mut response = Vec::with_capacity(params.queries.len());
    for std_id in params.queries {
        if SUPPORTS_STANDARDS.contains(&std_id.as_standard_identifier()) {
            response.push(SupportResult::Support);
        } else {
            response.push(host.state().have_implementors(&std_id));
        }
    }
    let result = SupportsQueryResponse::from(response);
    Ok(result)
}

/// Set the addresses for an implementation given a standard identifier and a
/// list of contract addresses.
///
/// It rejects if:
/// - Sender is not the admin of the contract instance.
/// - It fails to parse the parameter.
#[receive(
    contract = "Versus-League-Manager",
    name = "setImplementors",
    parameter = "SetImplementorsParams",
    error = "ContractError",
    mutable
)]
fn contract_set_implementor<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    // Check that only the admin is authorized to set implementors.
    ensure_eq!(ctx.sender(), host.state().admin, ContractError::Unauthorized);
    // Parse the parameter.
    let params: SetImplementorsParams = ctx.parameter_cursor().get()?;
    // Update the implementors in the state
    host.state_mut().set_implementors(params.id, params.implementors);
    Ok(())
}

/// Upgrade this smart contract instance to a new module and call optionally a
/// migration function after the upgrade.
///
/// It rejects if:
/// - Sender is not the admin of the contract instance.
/// - It fails to parse the parameter.
/// - If the ugrade fails.
/// - If the migration invoke fails.
///
/// This function is marked as `low_level`. This is **necessary** since the
/// high-level mutable functions store the state of the contract at the end of
/// execution. This conflicts with migration since the shape of the state
/// **might** be changed by the migration function. If the state is then written
/// by this function it would overwrite the state stored by the migration
/// function.
#[receive(
    contract = "Versus-League-Manager",
    name = "upgrade",
    parameter = "UpgradeParams",
    error = "ContractError",
    low_level
)]
fn contract_upgrade<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<S>,
) -> ContractResult<()> {
    // Read the top-level contract state.
    let state: State<S> = host.state().read_root()?;

    // Check that only the admin is authorized to upgrade the smart contract.
    ensure_eq!(ctx.sender(), state.admin, ContractError::Unauthorized);
    // Parse the parameter.
    let params: UpgradeParams = ctx.parameter_cursor().get()?;
    // Trigger the upgrade.
    host.upgrade(params.module)?;
    // Call the migration function if provided.
    if let Some((func, parameters)) = params.migrate {
        host.invoke_contract_raw(
            &ctx.self_address(),
            parameters.as_parameter(),
            func.as_entrypoint_name(),
            Amount::zero(),
        )?;
    }
    Ok(())
}
