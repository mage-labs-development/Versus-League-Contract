#![cfg_attr(not(feature = "std"), no_std)]
use concordium_cis2::{Cis2Event, *};
use concordium_std::{collections::BTreeMap, *};

/// The state tracked for each address.
#[derive(Serialize, SchemaType)]
struct PlayerData {
    /// The player's state
    state: PlayerState,
    /// The player's battle result
    results: Vec<BattleResult>,
}

/// The parameter type for the state contract function `updatePlayerState`.
#[derive(Serialize, SchemaType)]
struct UpdatePlayerStateParams {
    /// Player to update state.
    player: Address,
    /// Active or Suspended
    state: PlayerState,
}

/// The parameter type for the state contract function `updateBattleResult`.
#[derive(Serialize, SchemaType)]
struct UpdateBattleResultParams {
    /// Player to update state.
    player: Address,
    /// Win or Loss
    result: BattleResult,
}

/// List of supported standards by this contract address.
const SUPPORTS_STANDARDS: [StandardIdentifier<'static>; 2] =
    [CIS0_STANDARD_IDENTIFIER, CIS2_STANDARD_IDENTIFIER];

/// The contract state.
#[derive(Serial, DeserialWithState, StateClone)]
#[concordium(state_parameter = "S")]
struct State<S: HasStateApi> {
    /// The admin address can upgrade the contract, pause and unpause the
    /// contract, transfer the admin address to a new address, set
    /// implementors, and update the metadata URL in the contract.
    admin: Address,
    /// The state of the one player.
    player_data: StateMap<Address, PlayerData, S>,
    /// Contract is paused/unpaused.
    paused: bool,
    /// Map with contract addresses providing implementations of additional
    /// standards.
    implementors: StateMap<StandardIdentifierOwned, Vec<ContractAddress>, S>,
}

#[derive(Debug, Serialize, SchemaType, Clone, Copy, PartialEq)]
enum PlayerState {
    NotAdded,
    Active,
    Suspended,
}

#[derive(Debug, Serialize, SchemaType, Clone, Copy)]
enum BattleResult {
    NoResult,
    Win,
    Loss,
}

/// The parameter type for the contract function `setImplementors`.
/// Takes a standard identifier and list of contract addresses providing
/// implementations of this standard.
#[derive(Debug, Serialize, SchemaType)]
struct SetImplementorsParams {
    /// The identifier for the standard.
    id: StandardIdentifierOwned,
    /// The addresses of the implementors of the standard.
    implementors: Vec<ContractAddress>,
}

#[derive(Debug, Serialize, SchemaType)]
struct UpgradeParams {
    /// The new module reference.
    module: ModuleReference,
    /// Optional entrypoint to call in the new module after upgrade.
    migrate: Option<(OwnedEntrypointName, OwnedParameter)>,
}

/// The return type for the contract function `view`.
#[derive(Serialize, SchemaType)]
struct ReturnBasicState {
    /// The admin address can upgrade the contract, pause and unpause the
    /// contract, transfer the admin address to a new address, set
    /// implementors, and update the metadata URL in the contract.
    admin: Address,
    /// Contract is paused if `paused = true` and unpaused if `paused = false`.
    paused: bool,
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
    ParseParams,
    /// Failed logging: Log is full.
    LogFull,
    /// Failed logging: Log is malformed.
    LogMalformed,
    /// Contract is paused.
    ContractPaused,
    /// Failed to invoke a contract.
    InvokeContractError,
    /// Failed to invoke a transfer.
    InvokeTransferError,
    /// Upgrade failed because the new module does not exist.
    FailedUpgradeMissingModule,
    /// Upgrade failed because the new module does not contain a contract with a
    /// matching name.
    FailedUpgradeMissingContract,
    /// Upgrade failed because the smart contract version of the module is not
    /// supported.
    FailedUpgradeUnsupportedModuleVersion,
}

type ContractError = Cis2Error<CustomContractError>;

type ContractResult<A> = Result<A, ContractError>;

/// Mapping the logging errors to ContractError.
impl From<LogError> for CustomContractError {
    fn from(le: LogError) -> Self {
        match le {
            LogError::Full => Self::LogFull,
            LogError::Malformed => Self::LogMalformed,
        }
    }
}

/// Mapping errors related to contract invocations to ContractError.
impl<T> From<CallContractError<T>> for CustomContractError {
    fn from(_cce: CallContractError<T>) -> Self {
        Self::InvokeContractError
    }
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

impl<S: HasStateApi> State<S> {
    /// Creates the new state of the `state` contract with no one having any
    /// data by default. The ProtocolAddressesState is uninitialized.
    /// The ProtocolAddressesState has to be set with the `initialize`
    /// function after the `proxy` contract is deployed.
    fn new(state_builder: &mut StateBuilder<S>, admin: Address) -> Self {
        // Setup state.
        State {
            admin,
            player_data: state_builder.new_map(),
            paused: false,
            implementors: state_builder.new_map(),
        }
    }

    /// Check if state contains any implementors for a given standard.
    fn have_implementors(&self, std_id: &StandardIdentifierOwned) -> SupportResult {
        if let Some(addresses) = self.implementors.get(std_id) {
            SupportResult::SupportBy(addresses.to_vec())
        } else {
            SupportResult::NoSupport
        }
    }

    /// Set implementors for a given standard.
    fn set_implementors(
        &mut self,
        std_id: StandardIdentifierOwned,
        implementors: Vec<ContractAddress>,
    ) {
        self.implementors.insert(std_id, implementors);
    }
}

// Contract functions

/// Init function that creates a new smart contract.
#[init(contract = "Versus-League-Manager")]
fn contract_init<S: HasStateApi>(
    _ctx: &impl HasInitContext,
    state_builder: &mut StateBuilder<S>,
) -> InitResult<State<S>> {
    // Get the instantiator of this contract instance to be used as the initial
    // admin.
    let invoker = Address::Account(_ctx.init_origin());
    // Construct the initial contract state.
    let state = State::new(state_builder, invoker);

    Ok(state)
}

/// Add new player.
#[receive(
    contract = "Versus-League-Manager",
    name = "setPlayerData",
    parameter = "(Address, PlayerState, BattleResult)",
    error = "CustomContractError",
    mutable
)]
fn contract_state_set_player_data<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    // Check that contract is not paused.
    ensure!(!host.state().paused, ContractError::Custom(CustomContractError::ContractPaused));

    let params: (Address, PlayerState, BattleResult) = ctx.parameter_cursor().get()?;

    //result should be empty vector
    let mut player_data = host
        .state_mut()
        .player_data
        .entry(params.0)
        .or_insert_with(|| PlayerData {
            state: PlayerState::Active,
            results: Vec::new(),
        });
    player_data.state = params.1;

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
    // Check that contract is not paused.
    ensure!(!host.state().paused, ContractError::Custom(CustomContractError::ContractPaused));
    
    let params: UpdateBattleResultParams = ctx.parameter_cursor().get()?;
    let mut player_data = host
        .state_mut()
        .player_data
        .entry(params.player)
        .or_insert_with(|| PlayerData {
            state: PlayerState::Active,
            results: Vec::new(),
        });
    player_data.results.push(params.result);
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
) -> ContractResult<(PlayerState, Vec<BattleResult>)> {
    let params: Address = ctx.parameter_cursor().get()?;

    let player = host.state().player_data.get(&params).unwrap();

    Ok(( player.state, player.results.clone() ))
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
fn contract_view<S: HasStateApi>(
    _ctx: &impl HasReceiveContext,
    host: &impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<ReturnBasicState> {
    let state = ReturnBasicState {
        admin: host.state().admin,
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
    error = "CustomContractError"
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
    error = "CustomContractError",
    mutable
)]
fn contract_set_implementor<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    // Check that only the admin is authorized to set implementors.
    ensure_eq!(
        ctx.sender(),
        host.state().admin,
        ContractError::Unauthorized
    );
    // Parse the parameter.
    let params: SetImplementorsParams = ctx.parameter_cursor().get()?;
    // Update the implementors in the state
    host.state_mut()
        .set_implementors(params.id, params.implementors);
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
    error = "CustomContractError",
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
