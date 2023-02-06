#![cfg_attr(not(feature = "std"), no_std)]
use concordium_cis2::*;
use concordium_std::*;

/// The state tracked for each address.
#[derive(Serialize, SchemaType)]
struct PlayerData {
    /// The player's state
    state: PlayerState,
    /// The player's wins
    wins: u64,
    /// The player's losses
    losses: u64,
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
    Active,
    Suspended,
}

#[derive(Debug, Serialize, SchemaType, Clone, Copy)]
enum BattleResult {
    Win,
    Loss,
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

#[derive(Serialize, SchemaType)]
struct NewBattleResultEvent {
    /// Player address.
    player: Address,
    /// Player's new battle result.
    is_win: BattleResult,
}

/// A BattleResultEvent introduced by this smart contract.
/// This event is emitted when a player's battle result is updated.
#[derive(Serial, SchemaType)]
struct BattleResultEvent {
    /// Player address.
    player: Address,
    /// Player's new battle result.
    is_win: bool,
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
    /// The caller is not the admin.
    Unauthorized,
    /// Contract is paused.
    ContractPaused,
    /// Failed to invoke a contract.
    InvokeContractError,
    /// Player does not exist.
    PlayerDoesNotExist,
    /// Upgrade failed because the new module does not exist.
    FailedUpgradeMissingModule,
    /// Upgrade failed because the new module does not contain a contract with a
    /// matching name.
    FailedUpgradeMissingContract,
    /// Upgrade failed because the smart contract version of the module is not
    /// supported.
    FailedUpgradeUnsupportedModuleVersion,
}

type ContractError = CustomContractError;

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

}

// Contract functions

/// Init function that creates a new smart contract.
#[init(contract = "Versus-League-Manager", enable_logger)]
fn contract_init<S: HasStateApi>(
    ctx: &impl HasInitContext,
    state_builder: &mut StateBuilder<S>,
    logger: &mut impl HasLogger,
) -> InitResult<State<S>> {
    // Get the instantiator of this contract instance to be used as the initial
    // admin.
    let invoker = Address::Account(ctx.init_origin());
    // Construct the initial contract state.
    let state = State::new(state_builder, invoker);

    logger.log(&NewAdminEvent {
        new_admin: invoker,
    })?;

    Ok(state)
}

/// Add new player.
#[receive(
    contract = "Versus-League-Manager",
    name = "setPlayerData",
    parameter = "(Address, PlayerState)",
    error = "CustomContractError",
    mutable,
)]
fn contract_state_set_player_data<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    // Check that contract is not paused.
    ensure!(!host.state().paused, ContractError::ContractPaused);
    // Check that only the admin is authorized to set player data.
    ensure_eq!(
        ctx.sender(),
        host.state().admin,
        ContractError::Unauthorized
    );

    let params: (Address, PlayerState) = ctx.parameter_cursor().get()?;

    host
        .state_mut()
        .player_data
        .entry(params.0)
        .and_modify(|pd| pd.state = params.1)
        .or_insert(PlayerData {
            state: params.1,
            wins: 0,
            losses: 0,
        });

    Ok(())
}

#[receive(
    contract = "Versus-League-Manager",
    name = "updateBattleResult",
    parameter = "UpdateBattleResultParams",
    error = "CustomContractError",
    mutable,
    enable_logger
)]
fn update_battle_result<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
    logger: &mut impl HasLogger,
) -> ContractResult<()> {

    // Check that contract is not paused.
    ensure!(!host.state().paused, ContractError::ContractPaused);
    // Check that only the admin is authorized to set player data.
    ensure_eq!(
        ctx.sender(),
        host.state().admin,
        ContractError::Unauthorized
    );

    let params: UpdateBattleResultParams = ctx.parameter_cursor().get()?;

    let player_data = host.state_mut().player_data.get_mut(&params.player);
    if player_data.is_none() {
        return Ok(());
    }

    let mut player_data = player_data.unwrap();

    match params.result {
        BattleResult::Win => {
            player_data.wins += 1;
        }
        BattleResult::Loss => {
            player_data.losses += 1;
        }
    }

    logger.log(&NewBattleResultEvent {
        player: params.player,
        is_win: params.result,
    })?;

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
    return_value = "(PlayerState)",
    error = "CustomContractError"
)]
fn contract_state_get_player_data<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<PlayerState> {
    let params: Address = ctx.parameter_cursor().get()?;

    let player = host.state().player_data.get(&params);
    match player {
        Some(player) => Ok(player.state),
        None => Err(CustomContractError::PlayerDoesNotExist.into()),
    }
}

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
    let player_data = host.state().player_data.get(&params);

    Ok(player_data.is_some())
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

/// Set the admin of the contract instance.
#[receive(
    contract = "Versus-League-Manager",
    name = "updateAdmin",
    parameter = "Address",
    error = "ContractError",
    enable_logger,
    mutable
)]
fn contract_update_admin<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
    logger: &mut impl HasLogger,
) -> ContractResult<()> {
    // Check that only the current admin is authorized to update the admin address.
    ensure_eq!(ctx.sender(), host.state().admin, ContractError::Unauthorized);
    
    // Parse the parameter.
    let new_admin = ctx.parameter_cursor().get()?;

    // Update the admin variable.
    host.state_mut().admin = new_admin;

    logger.log(&NewAdminEvent {
        new_admin: new_admin,
    })?;

    Ok(())
}

/// Pause or unpause the contract.
#[receive(
    contract = "Versus-League-Manager",
    name = "setPaused",
    parameter = "SetPausedParams",
    error = "ContractError",
    mutable
)]
fn contract_update_pause<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    // Check that only the admin is authorized to pause/unpause the contract.
    ensure_eq!(ctx.sender(), host.state().admin, ContractError::Unauthorized);

    // Parse the parameter.
    let params: SetPausedParams = ctx.parameter_cursor().get()?;

    // Update the paused variable.
    host.state_mut().paused = params.paused;

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

#[concordium_cfg_test]
mod tests {
    use super::*;
    use test_infrastructure::*;

    const ADMIN_ACCOUNT: AccountAddress = AccountAddress([2u8; 32]);
    const ADMIN_ADDRESS: Address = Address::Account(ADMIN_ACCOUNT);
    const NEW_ADMIN_ACCOUNT: AccountAddress = AccountAddress([3u8; 32]);
    const NEW_ADMIN_ADDRESS: Address = Address::Account(NEW_ADMIN_ACCOUNT);


    /// Test admin can update to a new admin address.
    #[concordium_test]
    fn test_update_admin() {
        // Set up the context
        let mut ctx = TestReceiveContext::empty();
        ctx.set_sender(ADMIN_ADDRESS);
        let mut logger = TestLogger::init();

        // Set up the parameter.
        let parameter_bytes = to_bytes(&[NEW_ADMIN_ADDRESS]);
        ctx.set_parameter(&parameter_bytes);

        // Set up the state and host.
        let mut state_builder = TestStateBuilder::new();
        let state = initial_state(&mut state_builder);
        let mut host = TestHost::new(state, state_builder);

        // Check the admin state.
        claim_eq!(host.state().admin, ADMIN_ADDRESS, "Admin should be the old admin address");

        // Call the contract function.
        let result: ContractResult<()> = contract_update_admin(&ctx, &mut host, &mut logger);

        // Check the result.
        claim!(result.is_ok(), "Results in rejection");

        // Check the admin state.
        claim_eq!(host.state().admin, NEW_ADMIN_ADDRESS, "Admin should be the new admin address");

        // Check the logs
        claim_eq!(logger.logs.len(), 1, "Exactly one event should be logged");

        // Check the event
        claim!(
            logger.logs.contains(&to_bytes(&WccdEvent::NewAdmin(NewAdminEvent {
                new_admin: NEW_ADMIN_ADDRESS,
            }))),
            "Missing event for the new admin"
        );
    }

    /// Test that only the current admin can update the admin address.
    #[concordium_test]
    fn test_update_admin_not_authorized() {
        // Set up the context.
        let mut ctx = TestReceiveContext::empty();
        // NEW_ADMIN is not the current admin but tries to update the admin variable to
        // its own address.
        ctx.set_sender(NEW_ADMIN_ADDRESS);
        let mut logger = TestLogger::init();

        // Set up the parameter.
        let parameter_bytes = to_bytes(&[NEW_ADMIN_ADDRESS]);
        ctx.set_parameter(&parameter_bytes);

        // Set up the state and host.
        let mut state_builder = TestStateBuilder::new();
        let state = initial_state(&mut state_builder);
        let mut host = TestHost::new(state, state_builder);

        // Check the admin state.
        claim_eq!(host.state().admin, ADMIN_ADDRESS, "Admin should be the old admin address");

        // Call the contract function.
        let result: ContractResult<()> = contract_update_admin(&ctx, &mut host, &mut logger);

        // Check that invoke failed.
        claim_eq!(
            result,
            Err(ContractError::Unauthorized),
            "Update admin should fail because not the current admin tries to update"
        );

        // Check the admin state.
        claim_eq!(host.state().admin, ADMIN_ADDRESS, "Admin should be still the old admin address");
    }

    /// Test pausing the contract.
    #[concordium_test]
    fn test_pause() {
        // Set up the context.
        let mut ctx = TestReceiveContext::empty();
        ctx.set_sender(ADMIN_ADDRESS);

        // Set up the parameter to pause the contract.
        let parameter_bytes = to_bytes(&true);
        ctx.set_parameter(&parameter_bytes);

        // Set up the state and host.
        let mut state_builder = TestStateBuilder::new();
        let state = initial_state(&mut state_builder);
        let mut host = TestHost::new(state, state_builder);

        // Call the contract function.
        let result: ContractResult<()> = contract_set_paused(&ctx, &mut host);

        // Check the result.
        claim!(result.is_ok(), "Results in rejection");

        // Check contract is paused.
        claim_eq!(host.state().paused, true, "Smart contract should be paused");
    }

    /// Test unpausing the contract.
    #[concordium_test]
    fn test_unpause() {
        // Set up the context.
        let mut ctx = TestReceiveContext::empty();
        ctx.set_sender(ADMIN_ADDRESS);

        // Set up the parameter to pause the contract.
        let parameter_bytes = to_bytes(&true);
        ctx.set_parameter(&parameter_bytes);

        // Set up the state and host.
        let mut state_builder = TestStateBuilder::new();
        let state = initial_state(&mut state_builder);
        let mut host = TestHost::new(state, state_builder);

        // Call the contract function.
        let result: ContractResult<()> = contract_set_paused(&ctx, &mut host);

        // Check the result.
        claim!(result.is_ok(), "Results in rejection");

        // Check contract is paused.
        claim_eq!(host.state().paused, true, "Smart contract should be paused");

        // Set up the parameter to unpause the contract.
        let parameter_bytes = to_bytes(&false);
        ctx.set_parameter(&parameter_bytes);

        // Call the contract function.
        let result: ContractResult<()> = contract_set_paused(&ctx, &mut host);

        // Check the result.
        claim!(result.is_ok(), "Results in rejection");

        // Check contract is unpaused.
        claim_eq!(host.state().paused, false, "Smart contract should be unpaused");
    }

    /// Test that only the current admin can pause/unpause the contract.
    #[concordium_test]
    fn test_pause_not_authorized() {
        // Set up the context.
        let mut ctx = TestReceiveContext::empty();
        // NEW_ADMIN is not the current admin but tries to pause/unpause the contract.
        ctx.set_sender(NEW_ADMIN_ADDRESS);

        // Set up the parameter to pause the contract.
        let parameter_bytes = to_bytes(&true);
        ctx.set_parameter(&parameter_bytes);

        // Set up the state and host.
        let mut state_builder = TestStateBuilder::new();
        let state = initial_state(&mut state_builder);
        let mut host = TestHost::new(state, state_builder);

        // Call the contract function.
        let result: ContractResult<()> = contract_set_paused(&ctx, &mut host);

        // Check that invoke failed.
        claim_eq!(
            result,
            Err(ContractError::Unauthorized),
            "Pause should fail because not the current admin tries to invoke it"
        );
    }

   

}
