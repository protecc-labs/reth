//! Traits for execution.

use reth_interfaces::provider::ProviderError;
use reth_primitives::U256;
use revm::db::BundleState;
use revm_primitives::db::Database;

/// A general purpose executor trait that executes on an input (e.g. blocks) and produces an output
/// (e.g. state changes and receipts).
pub trait Executor<DB> {
    /// The input type for the executor.
    type Input<'a>;
    /// The output type for the executor.
    type Output;
    /// The error type returned by the executor.
    type Error;

    /// Consumes the type and executes the block.
    ///
    /// Returns the output of the block execution.
    fn execute(self, input: Self::Input<'_>) -> Result<Self::Output, Self::Error>;
}

/// An executor that can execute multiple blocks in a row and keep track of the state over the
/// entire batch.
pub trait BatchExecutor<DB> {
    /// The input type for the executor.
    type Input<'a>;
    /// The output type for the executor.
    type Output;
    /// The error type returned by the executor.
    type Error;

    /// Executes the next block in the batch and update the state internally.
    fn execute_one(&mut self, input: Self::Input<'_>) -> Result<BatchBlockOutput, Self::Error>;

    /// Finishes the batch and return the final state.
    fn finalize(self) -> Self::Output;
}

/// The output of an executed block in a batch.
#[derive(Debug, Clone, Copy)]
pub struct BatchBlockOutput {
    /// The size hint of the batch's tracked state.
    pub size_hint: Option<usize>,
}

/// The output of an ethereum block.
///
/// Contains the state changes, transaction receipts, and total gas used in the block.
///
/// TODO(mattsse): combine with BundleStateWithReceipts
#[derive(Debug)]
pub struct EthBlockOutput<T> {
    /// The changed state of the block after execution.
    pub state: BundleState,
    /// All the receipts of the transactions in the block.
    pub receipts: Vec<T>,
    /// The total gas used by the block.
    pub gas_used: u64,
}

/// A helper type for ethereum block inputs that consists of a block and the total difficulty.
#[derive(Debug)]
pub struct EthBlockExecutionInput<'a, Block> {
    /// The block to execute.
    pub block: &'a Block,
    /// The total difficulty of the block.
    pub total_difficulty: U256,
}

impl<'a, Block> EthBlockExecutionInput<'a, Block> {
    /// Creates a new input.
    pub fn new(block: &'a Block, total_difficulty: U256) -> Self {
        Self { block, total_difficulty }
    }
}

impl<'a, Block> From<(&'a Block, U256)> for EthBlockExecutionInput<'a, Block> {
    fn from((block, total_difficulty): (&'a Block, U256)) -> Self {
        Self::new(block, total_difficulty)
    }
}

/// A type that can create a new executor.
pub trait ExecutorProvider: Send + Sync + Clone {
    /// An executor that can execute a single block given a database.
    type Executor<DB: Database<Error = ProviderError>>: Executor<DB>;
    /// An executor that can execute a batch of blocks given a database.

    type BatchExecutor<DB: Database<Error = ProviderError>>: BatchExecutor<DB>;
    /// Creates a new executor for single block execution.
    fn executor<DB>(&self, db: DB) -> Self::Executor<DB>
    where
        DB: Database<Error = ProviderError>;

    /// Creates a new batch executor
    fn batch_executor<DB>(&self, db: DB) -> Self::BatchExecutor<DB>
    where
        DB: Database<Error = ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use revm::db::{CacheDB, EmptyDBTyped};
    use std::marker::PhantomData;

    #[derive(Clone, Default)]
    struct TestExecutorProvider;

    impl ExecutorProvider for TestExecutorProvider {
        type Executor<DB: Database<Error = ProviderError>> = TestExecutor<DB>;
        type BatchExecutor<DB: Database<Error = ProviderError>> = TestExecutor<DB>;

        fn executor<DB>(&self, _db: DB) -> Self::Executor<DB>
        where
            DB: Database<Error = ProviderError>,
        {
            TestExecutor(PhantomData)
        }

        fn batch_executor<DB>(&self, _db: DB) -> Self::BatchExecutor<DB>
        where
            DB: Database<Error = ProviderError>,
        {
            TestExecutor(PhantomData)
        }
    }

    struct TestExecutor<DB>(PhantomData<DB>);

    impl<DB> Executor<DB> for TestExecutor<DB> {
        type Input<'a> = &'static str;
        type Output = ();
        type Error = String;

        fn execute(self, _input: Self::Input<'_>) -> Result<Self::Output, Self::Error> {
            Ok(())
        }
    }

    impl<DB> BatchExecutor<DB> for TestExecutor<DB> {
        type Input<'a> = &'static str;
        type Output = ();
        type Error = String;

        fn execute_one(
            &mut self,
            _input: Self::Input<'_>,
        ) -> Result<BatchBlockOutput, Self::Error> {
            Ok(BatchBlockOutput { size_hint: None })
        }

        fn finalize(self) -> Self::Output {}
    }

    #[test]
    fn test_provider() {
        let provider = TestExecutorProvider;
        let db = CacheDB::<EmptyDBTyped<ProviderError>>::default();
        let executor = provider.executor(db);
        executor.execute("test").unwrap();
    }
}
