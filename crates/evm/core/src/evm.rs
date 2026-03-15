use std::{
    fmt::Debug,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use crate::{
    Env, FoundryContextExt, FoundryInspectorExt,
    backend::{DatabaseExt, FoundryJournalExt, JournaledState},
    constants::DEFAULT_CREATE2_DEPLOYER_CODEHASH,
};
use alloy_consensus::constants::KECCAK_EMPTY;
use alloy_evm::{Evm, EvmEnv, eth::EthEvmContext, precompiles::PrecompilesMap};
use alloy_primitives::{Address, Bytes, U256};
use foundry_fork_db::DatabaseError;
use revm::{
    Context, Database, Journal,
    context::{
        BlockEnv, CfgEnv, ContextTr, CreateScheme, Evm as RevmEvm, JournalTr, LocalContext,
        LocalContextTr, TxEnv,
        result::{EVMError, ExecResultAndState, ExecutionResult, HaltReason, ResultAndState},
    },
    handler::{
        EthFrame, EthPrecompiles, EvmTr, FrameResult, FrameTr, Handler, ItemOrResult,
        instructions::EthInstructions,
    },
    inspector::{Inspector, InspectorEvmTr, InspectorHandler},
    interpreter::{
        CallInput, CallInputs, CallOutcome, CallScheme, CallValue, CreateInputs, CreateOutcome,
        FrameInput, Gas, InstructionResult, InterpreterResult, SharedMemory,
        interpreter::EthInterpreter, interpreter_action::FrameInit, return_ok,
    },
    precompile::{PrecompileSpecId, Precompiles},
    primitives::hardfork::SpecId,
};

/// Creates a new [`FoundryEvm`] with the given database, environment, and inspector.
///
/// Generic over `DB: Database` — callers typically pass `&mut dyn DatabaseExt` or
/// `&mut CowBackend`, but any `Database` implementation works.
pub fn new_evm_with_inspector<DB, I>(
    db: DB,
    evm_env: EvmEnv,
    tx_env: TxEnv,
    inspector: I,
) -> FoundryEvm<DB, I>
where
    DB: Database + Debug,
    I: Inspector<EthEvmContext<DB>> + FoundryInspectorExt,
{
    let mut ctx = EthEvmContext {
        journaled_state: {
            let mut journal = Journal::new(db);
            journal.set_spec_id(evm_env.cfg_env.spec);
            journal
        },
        block: evm_env.block_env,
        cfg: evm_env.cfg_env,
        tx: tx_env,
        chain: (),
        local: LocalContext::default(),
        error: Ok(()),
    };
    ctx.cfg.tx_chain_id_check = true;
    let spec = ctx.cfg.spec;

    let mut evm = FoundryEvm {
        inner: RevmEvm::new_with_inspector(
            ctx,
            inspector,
            EthInstructions::default(),
            get_precompiles(spec),
        ),
    };

    evm.inspector().get_networks().inject_precompiles(evm.precompiles_mut());
    evm
}

/// Get the precompiles for the given spec.
fn get_precompiles(spec: SpecId) -> PrecompilesMap {
    PrecompilesMap::from_static(
        EthPrecompiles {
            precompiles: Precompiles::new(PrecompileSpecId::from_spec_id(spec)),
            spec,
        }
        .precompiles,
    )
}

/// Get the call inputs for the CREATE2 factory.
fn get_create2_factory_call_inputs(
    salt: U256,
    inputs: &CreateInputs,
    deployer: Address,
) -> CallInputs {
    let calldata = [&salt.to_be_bytes::<32>()[..], &inputs.init_code()[..]].concat();
    CallInputs {
        caller: inputs.caller(),
        bytecode_address: deployer,
        known_bytecode: None,
        target_address: deployer,
        scheme: CallScheme::Call,
        value: CallValue::Transfer(inputs.value()),
        input: CallInput::Bytes(calldata.into()),
        gas_limit: inputs.gas_limit(),
        is_static: false,
        return_memory_offset: 0..0,
    }
}

/// Foundry's custom EVM wrapper with CREATE2 factory interception.
///
/// Generic over `DB` and `I`. Bounds are on the impl blocks, not the struct,
/// so downstream code can hold a `FoundryEvm` without requiring `DB: Debug`.
pub struct FoundryEvm<DB: Database, I> {
    #[allow(clippy::type_complexity)]
    inner: RevmEvm<
        EthEvmContext<DB>,
        I,
        EthInstructions<EthInterpreter, EthEvmContext<DB>>,
        PrecompilesMap,
        EthFrame<EthInterpreter>,
    >,
}

impl<DB, I> FoundryEvm<DB, I>
where
    DB: Database + Debug,
    I: Inspector<EthEvmContext<DB>> + FoundryInspectorExt,
{
    /// Consumes the EVM and returns the inner context.
    pub fn into_context(self) -> EthEvmContext<DB> {
        self.inner.ctx
    }

    pub fn run_execution(
        &mut self,
        frame: FrameInput,
    ) -> Result<FrameResult, EVMError<DB::Error>> {
        let mut handler = FoundryHandler::<DB, I>::default();

        // Create first frame
        let memory =
            SharedMemory::new_with_buffer(self.inner.ctx().local().shared_memory_buffer().clone());
        let first_frame_input = FrameInit { depth: 0, memory, frame_input: frame };

        // Run execution loop
        let mut frame_result = handler.inspect_run_exec_loop(&mut self.inner, first_frame_input)?;

        // Handle last frame result
        handler.last_frame_result(&mut self.inner, &mut frame_result)?;

        Ok(frame_result)
    }
}

impl<DB, I> Evm for FoundryEvm<DB, I>
where
    DB: Database + Debug,
    I: Inspector<EthEvmContext<DB>> + FoundryInspectorExt,
{
    type Precompiles = PrecompilesMap;
    type Inspector = I;
    type DB = DB;
    type Error = EVMError<DB::Error>;
    type HaltReason = HaltReason;
    type Spec = SpecId;
    type Tx = TxEnv;
    type BlockEnv = BlockEnv;

    fn block(&self) -> &BlockEnv {
        &self.inner.block
    }

    fn chain_id(&self) -> u64 {
        self.inner.ctx.cfg.chain_id
    }

    fn components(&self) -> (&Self::DB, &Self::Inspector, &Self::Precompiles) {
        (&self.inner.ctx.journaled_state.database, &self.inner.inspector, &self.inner.precompiles)
    }

    fn components_mut(&mut self) -> (&mut Self::DB, &mut Self::Inspector, &mut Self::Precompiles) {
        (
            &mut self.inner.ctx.journaled_state.database,
            &mut self.inner.inspector,
            &mut self.inner.precompiles,
        )
    }

    fn db_mut(&mut self) -> &mut Self::DB {
        &mut self.inner.ctx.journaled_state.database
    }

    fn precompiles(&self) -> &Self::Precompiles {
        &self.inner.precompiles
    }

    fn precompiles_mut(&mut self) -> &mut Self::Precompiles {
        &mut self.inner.precompiles
    }

    fn inspector(&self) -> &Self::Inspector {
        &self.inner.inspector
    }

    fn inspector_mut(&mut self) -> &mut Self::Inspector {
        &mut self.inner.inspector
    }

    fn set_inspector_enabled(&mut self, _enabled: bool) {
        unimplemented!("FoundryEvm is always inspecting")
    }

    fn transact_raw(
        &mut self,
        tx: Self::Tx,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        self.inner.ctx.tx = tx;

        let mut handler = FoundryHandler::<DB, I>::default();
        let result = handler.inspect_run(&mut self.inner)?;

        Ok(ResultAndState::new(result, self.inner.ctx.journaled_state.inner.state.clone()))
    }

    fn transact_system_call(
        &mut self,
        _caller: Address,
        _contract: Address,
        _data: Bytes,
    ) -> Result<ExecResultAndState<ExecutionResult>, Self::Error> {
        unimplemented!()
    }

    fn finish(self) -> (Self::DB, EvmEnv<Self::Spec>)
    where
        Self: Sized,
    {
        let Context { block: block_env, cfg: cfg_env, journaled_state, .. } = self.inner.ctx;

        (journaled_state.database, EvmEnv { block_env, cfg_env })
    }
}

impl<DB, I> Deref for FoundryEvm<DB, I>
where
    DB: Database + Debug,
    I: Inspector<EthEvmContext<DB>> + FoundryInspectorExt,
{
    type Target = Context<BlockEnv, TxEnv, CfgEnv, DB>;

    fn deref(&self) -> &Self::Target {
        &self.inner.ctx
    }
}

impl<DB, I> DerefMut for FoundryEvm<DB, I>
where
    DB: Database + Debug,
    I: Inspector<EthEvmContext<DB>> + FoundryInspectorExt,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner.ctx
    }
}

/// Object-safe trait exposing the operations that cheatcode nested EVM closures need.
///
/// This abstracts over the concrete EVM type (`FoundryEvm`, future `TempoEvm`, etc.)
/// so that cheatcode impls can build and run nested EVMs without knowing the concrete type.
pub trait NestedEvm {
    /// Returns a mutable reference to the journal inner state (`JournaledState`).
    fn journal_inner_mut(&mut self) -> &mut JournaledState;

    /// Runs a single execution frame (create or call) through the EVM handler loop.
    fn run_execution(&mut self, frame: FrameInput) -> Result<FrameResult, EVMError<DatabaseError>>;

    /// Executes a full transaction with the given `TxEnv`.
    fn transact(
        &mut self,
        tx: TxEnv,
    ) -> Result<ResultAndState<HaltReason>, EVMError<DatabaseError>>;

    /// Returns a snapshot of the current environment (cfg + block, tx).
    fn to_env(&self) -> (EvmEnv, TxEnv);
}

/// `NestedEvm` is implemented for `FoundryEvm` when `DB::Error = DatabaseError`,
/// which is the case for all foundry database types (`&mut dyn DatabaseExt`, `CowBackend`, etc.).
impl<DB, I> NestedEvm for FoundryEvm<DB, I>
where
    DB: Database<Error = DatabaseError> + Debug,
    I: Inspector<EthEvmContext<DB>> + FoundryInspectorExt,
{
    fn journal_inner_mut(&mut self) -> &mut JournaledState {
        &mut self.inner.ctx.journaled_state.inner
    }

    fn run_execution(&mut self, frame: FrameInput) -> Result<FrameResult, EVMError<DatabaseError>> {
        FoundryEvm::run_execution(self, frame)
    }

    fn transact(
        &mut self,
        tx: TxEnv,
    ) -> Result<ResultAndState<HaltReason>, EVMError<DatabaseError>> {
        Evm::transact_raw(self, tx)
    }

    fn to_env(&self) -> (EvmEnv, TxEnv) {
        (
            EvmEnv { cfg_env: self.inner.ctx.cfg.clone(), block_env: self.inner.ctx.block.clone() },
            self.inner.ctx.tx.clone(),
        )
    }
}

/// Clones the current context (env + journal), passes the database, cloned env,
/// and cloned journal inner to the callback. The callback builds whatever EVM it
/// needs, runs its operations, and returns `(result, modified_env, modified_journal)`.
/// Modified state is written back after the callback returns.
pub fn with_cloned_context<CTX: FoundryContextExt, R>(
    ecx: &mut CTX,
    f: impl FnOnce(
        &mut dyn DatabaseExt,
        EvmEnv,
        TxEnv,
        JournaledState,
    ) -> Result<(R, EvmEnv, TxEnv, JournaledState), EVMError<DatabaseError>>,
) -> Result<R, EVMError<DatabaseError>>
where
    CTX::Journal: FoundryJournalExt,
{
    let (evm_env, tx_env) = Env::clone_evm_and_tx(ecx);

    let journal = ecx.journal_mut();
    let (db, journal_inner) = journal.as_db_and_inner();
    let journal_inner_clone = journal_inner.clone();

    let (result, sub_evm_env, sub_tx, sub_inner) = f(db, evm_env, tx_env, journal_inner_clone)?;

    // Write back modified state. The db borrow was released when f returned.
    ecx.journal_mut().set_inner(sub_inner);
    Env::apply_evm_and_tx(ecx, sub_evm_env, sub_tx);

    Ok(result)
}

/// Foundry's custom EVM handler with CREATE2 factory interception.
///
/// Generic over `DB` — the handler logic only uses generic journal/inspector
/// trait methods, not `DatabaseExt`-specific operations.
pub struct FoundryHandler<DB: Database, I> {
    create2_overrides: Vec<(usize, CallInputs)>,
    _phantom: PhantomData<fn() -> (DB, I)>,
}

impl<DB: Database, I> Default for FoundryHandler<DB, I> {
    fn default() -> Self {
        Self { create2_overrides: Vec::new(), _phantom: PhantomData }
    }
}

// Blanket Handler implementation for FoundryHandler, needed for implementing the InspectorHandler
// trait.
impl<DB, I> Handler for FoundryHandler<DB, I>
where
    DB: Database + Debug,
    I: Inspector<EthEvmContext<DB>> + FoundryInspectorExt,
{
    type Evm = RevmEvm<
        EthEvmContext<DB>,
        I,
        EthInstructions<EthInterpreter, EthEvmContext<DB>>,
        PrecompilesMap,
        EthFrame<EthInterpreter>,
    >;
    type Error = EVMError<DB::Error>;
    type HaltReason = HaltReason;
}

impl<DB, I> FoundryHandler<DB, I>
where
    DB: Database + Debug,
    I: Inspector<EthEvmContext<DB>> + FoundryInspectorExt,
{
    /// Handles CREATE2 frame initialization, potentially transforming it to use the CREATE2
    /// factory.
    fn handle_create_frame(
        &mut self,
        evm: &mut <Self as Handler>::Evm,
        init: &mut FrameInit,
    ) -> Result<Option<FrameResult>, <Self as Handler>::Error> {
        if let FrameInput::Create(inputs) = &init.frame_input
            && let CreateScheme::Create2 { salt } = inputs.scheme()
        {
            let (ctx, inspector) = evm.ctx_inspector();

            if inspector.should_use_create2_factory(ctx.journal().depth(), inputs) {
                let gas_limit = inputs.gas_limit();

                // Get CREATE2 deployer.
                let create2_deployer = evm.inspector().create2_deployer();

                // Generate call inputs for CREATE2 factory.
                let call_inputs = get_create2_factory_call_inputs(salt, inputs, create2_deployer);

                // Push data about current override to the stack.
                self.create2_overrides.push((evm.journal().depth(), call_inputs.clone()));

                // Sanity check that CREATE2 deployer exists.
                let code_hash = evm.journal_mut().load_account(create2_deployer)?.info.code_hash;
                if code_hash == KECCAK_EMPTY {
                    return Ok(Some(FrameResult::Call(CallOutcome {
                        result: InterpreterResult {
                            result: InstructionResult::Revert,
                            output: Bytes::from(
                                format!("missing CREATE2 deployer: {create2_deployer}")
                                    .into_bytes(),
                            ),
                            gas: Gas::new(gas_limit),
                        },
                        memory_offset: 0..0,
                        was_precompile_called: false,
                        precompile_call_logs: vec![],
                    })));
                } else if code_hash != DEFAULT_CREATE2_DEPLOYER_CODEHASH {
                    return Ok(Some(FrameResult::Call(CallOutcome {
                        result: InterpreterResult {
                            result: InstructionResult::Revert,
                            output: "invalid CREATE2 deployer bytecode".into(),
                            gas: Gas::new(gas_limit),
                        },
                        memory_offset: 0..0,
                        was_precompile_called: false,
                        precompile_call_logs: vec![],
                    })));
                }

                // Rewrite the frame init
                init.frame_input = FrameInput::Call(Box::new(call_inputs));
            }
        }
        Ok(None)
    }

    /// Transforms CREATE2 factory call results back into CREATE outcomes.
    fn handle_create2_override(
        &mut self,
        evm: &mut <Self as Handler>::Evm,
        result: FrameResult,
    ) -> FrameResult {
        if self.create2_overrides.last().is_some_and(|(depth, _)| *depth == evm.journal().depth()) {
            let (_, call_inputs) = self.create2_overrides.pop().unwrap();
            let FrameResult::Call(mut call) = result else {
                unreachable!("create2 override should be a call frame");
            };

            // Decode address from output.
            let address = match call.instruction_result() {
                return_ok!() => Address::try_from(call.output().as_ref())
                    .map_err(|_| {
                        call.result = InterpreterResult {
                            result: InstructionResult::Revert,
                            output: "invalid CREATE2 factory output".into(),
                            gas: Gas::new(call_inputs.gas_limit),
                        };
                    })
                    .ok(),
                _ => None,
            };

            FrameResult::Create(CreateOutcome { result: call.result, address })
        } else {
            result
        }
    }
}

impl<DB, I> InspectorHandler for FoundryHandler<DB, I>
where
    DB: Database + Debug,
    I: Inspector<EthEvmContext<DB>> + FoundryInspectorExt,
{
    type IT = EthInterpreter;

    fn inspect_run_exec_loop(
        &mut self,
        evm: &mut Self::Evm,
        first_frame_input: <<Self::Evm as EvmTr>::Frame as FrameTr>::FrameInit,
    ) -> Result<FrameResult, Self::Error> {
        let res = evm.inspect_frame_init(first_frame_input)?;

        if let ItemOrResult::Result(frame_result) = res {
            return Ok(frame_result);
        }

        loop {
            let call_or_result = evm.inspect_frame_run()?;

            let result = match call_or_result {
                ItemOrResult::Item(mut init) => {
                    // Handle CREATE/CREATE2 frame initialization
                    if let Some(frame_result) = self.handle_create_frame(evm, &mut init)? {
                        return Ok(frame_result);
                    }

                    match evm.inspect_frame_init(init)? {
                        ItemOrResult::Item(_) => continue,
                        ItemOrResult::Result(result) => result,
                    }
                }
                ItemOrResult::Result(result) => result,
            };

            // Handle CREATE2 override transformation if needed
            let result = self.handle_create2_override(evm, result);

            if let Some(result) = evm.frame_return_result(result)? {
                return Ok(result);
            }
        }
    }
}
