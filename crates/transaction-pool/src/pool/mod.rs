//! Transaction Pool internals.
//!
//! Incoming transactions validated are before they enter the pool first. The validation outcome can
//! have 3 states:
//!
//!  1. Transaction can _never_ be valid
//!  2. Transaction is _currently_ valid
//!  3. Transaction is _currently_ invalid, but could potentially become valid in the future
//!
//! However, (2.) and (3.) of a transaction can only be determined on the basis of the current
//! state, whereas (1.) holds indefinitely. This means once the state changes (2.) and (3.) the
//! state of a transaction needs to be reevaluated again.
//!
//! The transaction pool is responsible for storing new, valid transactions and providing the next
//! best transactions sorted by their priority. Where priority is determined by the transaction's
//! score ([`TransactionOrdering`]).
//!
//! Furthermore, the following characteristics fall under (3.):
//!
//!  a) Nonce of a transaction is higher than the expected nonce for the next transaction of its
//! sender. A distinction is made here whether multiple transactions from the same sender have
//! gapless nonce increments.
//!
//!  a)(1) If _no_ transaction is missing in a chain of multiple
//! transactions from the same sender (all nonce in row), all of them can in principle be executed
//! on the current state one after the other.
//!
//!  a)(2) If there's a nonce gap, then all
//! transactions after the missing transaction are blocked until the missing transaction arrives.
//!
//!  b) Transaction does not meet the dynamic fee cap requirement introduced by EIP-1559: The
//! fee cap of the transaction needs to be no less than the base fee of block.
//!
//!
//! In essence the transaction pool is made of three separate sub-pools:
//!
//!  - Pending Pool: Contains all transactions that are valid on the current state and satisfy
//! (3. a)(1): _No_ nonce gaps. A _pending_ transaction is considered _ready_ when it has the lowest
//! nonce of all transactions from the same sender. Once a _ready_ transaction with nonce `n` has
//! been executed, the next highest transaction from the same sender `n + 1` becomes ready.
//!
//!  - Queued Pool: Contains all transactions that are currently blocked by missing
//! transactions: (3. a)(2): _With_ nonce gaps or due to lack of funds.
//!
//!  - Basefee Pool: To account for the dynamic base fee requirement (3. b) which could render
//! an EIP-1559 and all subsequent transactions of the sender currently invalid.
//!
//! The classification of transactions is always dependent on the current state that is changed as
//! soon as a new block is mined. Once a new block is mined, the account changeset must be applied
//! to the transaction pool.
//!
//!
//! Depending on the use case, consumers of the [`TransactionPool`](crate::traits::TransactionPool)
//! are interested in (2.) and/or (3.).

//! A generic [`TransactionPool`](crate::traits::TransactionPool) that only handles transactions.
//!
//! This Pool maintains two separate sub-pools for (2.) and (3.)
//!
//! ## Terminology
//!
//!  - _Pending_: pending transactions are transactions that fall under (2.). These transactions can
//!    currently be executed and are stored in the pending sub-pool
//!  - _Queued_: queued transactions are transactions that fall under category (3.). Those
//!    transactions are _currently_ waiting for state changes that eventually move them into
//!    category (2.) and become pending.

use crate::{
    error::{PoolError, PoolResult},
    identifier::{SenderId, SenderIdentifiers, TransactionId},
    pool::{
        listener::PoolEventBroadcast,
        state::SubPool,
        txpool::{SenderInfo, TxPool},
    },
    traits::{
        AllPoolTransactions, BlockInfo, NewTransactionEvent, PoolSize, PoolTransaction,
        PropagatedTransactions, TransactionOrigin,
    },
    validate::{TransactionValidationOutcome, ValidPoolTransaction},
    CanonicalStateUpdate, ChangedAccount, PoolConfig, TransactionOrdering, TransactionValidator,
};
use best::BestTransactions;
use parking_lot::{Mutex, RwLock};
use reth_primitives::{Address, TxHash, H256};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
    time::Instant,
};
use tokio::sync::mpsc;
use tracing::{debug, trace};

mod events;
pub use events::{FullTransactionEvent, TransactionEvent};

mod listener;
use crate::{pool::txpool::UpdateOutcome, traits::PendingTransactionListenerKind};
pub use listener::{AllTransactionsEvents, TransactionEvents};

mod best;
mod parked;
pub(crate) mod pending;
pub(crate) mod size;
pub(crate) mod state;
pub mod txpool;
mod update;

/// Transaction pool internals.
pub struct PoolInner<V: TransactionValidator, T: TransactionOrdering> {
    /// Internal mapping of addresses to plain ints.
    identifiers: RwLock<SenderIdentifiers>,
    /// Transaction validation.
    validator: V,
    /// The internal pool that manages all transactions.
    pool: RwLock<TxPool<T>>,
    /// Pool settings.
    config: PoolConfig,
    /// Manages listeners for transaction state change events.
    event_listener: RwLock<PoolEventBroadcast<T::Transaction>>,
    /// Listeners for new pending transactions.
    pending_transaction_listener: Mutex<Vec<PendingTransactionListener>>,
    /// Listeners for new transactions added to the pool.
    transaction_listener: Mutex<Vec<mpsc::Sender<NewTransactionEvent<T::Transaction>>>>,
}

// === impl PoolInner ===

impl<V, T> PoolInner<V, T>
where
    V: TransactionValidator,
    T: TransactionOrdering<Transaction = <V as TransactionValidator>::Transaction>,
{
    /// Create a new transaction pool instance.
    pub(crate) fn new(validator: V, ordering: T, config: PoolConfig) -> Self {
        Self {
            identifiers: Default::default(),
            validator,
            event_listener: Default::default(),
            pool: RwLock::new(TxPool::new(ordering, config.clone())),
            pending_transaction_listener: Default::default(),
            transaction_listener: Default::default(),
            config,
        }
    }

    /// Returns stats about the size of the pool.
    pub(crate) fn size(&self) -> PoolSize {
        self.pool.read().size()
    }

    /// Returns the currently tracked block
    pub(crate) fn block_info(&self) -> BlockInfo {
        self.pool.read().block_info()
    }
    /// Returns the currently tracked block
    pub(crate) fn set_block_info(&self, info: BlockInfo) {
        self.pool.write().set_block_info(info)
    }

    /// Returns the internal `SenderId` for this address
    pub(crate) fn get_sender_id(&self, addr: Address) -> SenderId {
        self.identifiers.write().sender_id_or_create(addr)
    }

    /// Returns all senders in the pool
    pub(crate) fn unique_senders(&self) -> HashSet<Address> {
        self.pool.read().unique_senders()
    }

    /// Converts the changed accounts to a map of sender ids to sender info (internal identifier
    /// used for accounts)
    fn changed_senders(
        &self,
        accs: impl Iterator<Item = ChangedAccount>,
    ) -> HashMap<SenderId, SenderInfo> {
        let mut identifiers = self.identifiers.write();
        accs.into_iter()
            .map(|acc| {
                let ChangedAccount { address, nonce, balance } = acc;
                let sender_id = identifiers.sender_id_or_create(address);
                (sender_id, SenderInfo { state_nonce: nonce, balance })
            })
            .collect()
    }

    /// Get the config the pool was configured with.
    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    /// Get the validator reference.
    pub fn validator(&self) -> &V {
        &self.validator
    }

    /// Adds a new transaction listener to the pool that gets notified about every new _pending_
    /// transaction inserted into the pool
    pub fn add_pending_listener(
        &self,
        kind: PendingTransactionListenerKind,
    ) -> mpsc::Receiver<TxHash> {
        const TX_LISTENER_BUFFER_SIZE: usize = 2048;
        let (sender, rx) = mpsc::channel(TX_LISTENER_BUFFER_SIZE);
        let listener = PendingTransactionListener { sender, kind };
        self.pending_transaction_listener.lock().push(listener);
        rx
    }

    /// Adds a new transaction listener to the pool that gets notified about every new transaction.
    pub fn add_new_transaction_listener(
        &self,
    ) -> mpsc::Receiver<NewTransactionEvent<T::Transaction>> {
        const TX_LISTENER_BUFFER_SIZE: usize = 1024;
        let (tx, rx) = mpsc::channel(TX_LISTENER_BUFFER_SIZE);
        self.transaction_listener.lock().push(tx);
        rx
    }

    /// If the pool contains the transaction, this adds a new listener that gets notified about
    /// transaction events.
    pub(crate) fn add_transaction_event_listener(
        &self,
        tx_hash: TxHash,
    ) -> Option<TransactionEvents> {
        let pool = self.pool.read();
        if pool.contains(&tx_hash) {
            Some(self.event_listener.write().subscribe(tx_hash))
        } else {
            None
        }
    }

    /// Adds a listener for all transaction events.
    pub(crate) fn add_all_transactions_event_listener(
        &self,
    ) -> AllTransactionsEvents<T::Transaction> {
        self.event_listener.write().subscribe_all()
    }

    /// Returns hashes of _all_ transactions in the pool.
    pub(crate) fn pooled_transactions_hashes(&self) -> Vec<TxHash> {
        let pool = self.pool.read();
        pool.all().transactions_iter().filter(|tx| tx.propagate).map(|tx| *tx.hash()).collect()
    }

    /// Returns _all_ transactions in the pool.
    pub(crate) fn pooled_transactions(&self) -> Vec<Arc<ValidPoolTransaction<T::Transaction>>> {
        let pool = self.pool.read();
        pool.all().transactions_iter().filter(|tx| tx.propagate).collect()
    }

    /// Updates the entire pool after a new block was executed.
    pub(crate) fn on_canonical_state_change(&self, update: CanonicalStateUpdate) {
        trace!(target: "txpool", %update, "updating pool on canonical state change");

        let CanonicalStateUpdate {
            hash,
            number,
            pending_block_base_fee,
            changed_accounts,
            mined_transactions,
            timestamp: _,
        } = update;
        let changed_senders = self.changed_senders(changed_accounts.into_iter());
        let block_info = BlockInfo {
            last_seen_block_hash: hash,
            last_seen_block_number: number,
            pending_basefee: pending_block_base_fee,
        };
        let outcome = self.pool.write().on_canonical_state_change(
            block_info,
            mined_transactions,
            changed_senders,
        );
        self.notify_on_new_state(outcome);
    }

    /// Performs account updates on the pool.
    ///
    /// This will either promote or discard transactions based on the new account state.
    pub(crate) fn update_accounts(&self, accounts: Vec<ChangedAccount>) {
        let changed_senders = self.changed_senders(accounts.into_iter());
        let UpdateOutcome { promoted, discarded } =
            self.pool.write().update_accounts(changed_senders);
        let mut listener = self.event_listener.write();
        promoted.iter().for_each(|tx| listener.pending(tx, None));
        discarded.iter().for_each(|tx| listener.discarded(tx));
    }

    /// Add a single validated transaction into the pool.
    ///
    /// Note: this is only used internally by [`Self::add_transactions()`], all new transaction(s)
    /// come in through that function, either as a batch or `std::iter::once`.
    fn add_transaction(
        &self,
        origin: TransactionOrigin,
        tx: TransactionValidationOutcome<T::Transaction>,
    ) -> PoolResult<TxHash> {
        match tx {
            TransactionValidationOutcome::Valid {
                balance,
                state_nonce,
                transaction,
                propagate,
            } => {
                let sender_id = self.get_sender_id(transaction.sender());
                let transaction_id = TransactionId::new(sender_id, transaction.nonce());
                let encoded_length = transaction.encoded_length();

                let tx = ValidPoolTransaction {
                    transaction,
                    transaction_id,
                    propagate,
                    timestamp: Instant::now(),
                    origin,
                    encoded_length,
                };

                let added = self.pool.write().add_transaction(tx, balance, state_nonce)?;
                let hash = *added.hash();

                // Notify about new pending transactions
                if added.is_pending() {
                    self.on_new_pending_transaction(&added);
                }

                // Notify tx event listeners
                self.notify_event_listeners(&added);

                // Notify listeners for _all_ transactions
                self.on_new_transaction(added.into_new_transaction_event());

                Ok(hash)
            }
            TransactionValidationOutcome::Invalid(tx, err) => {
                let mut listener = self.event_listener.write();
                listener.discarded(tx.hash());
                Err(PoolError::InvalidTransaction(*tx.hash(), err))
            }
            TransactionValidationOutcome::Error(tx_hash, err) => {
                let mut listener = self.event_listener.write();
                listener.discarded(&tx_hash);
                Err(PoolError::Other(tx_hash, err))
            }
        }
    }

    pub(crate) fn add_transaction_and_subscribe(
        &self,
        origin: TransactionOrigin,
        tx: TransactionValidationOutcome<T::Transaction>,
    ) -> PoolResult<TransactionEvents> {
        let listener = {
            let mut listener = self.event_listener.write();
            listener.subscribe(tx.tx_hash())
        };
        self.add_transactions(origin, std::iter::once(tx)).pop().expect("exists; qed")?;
        Ok(listener)
    }

    /// Adds all transactions in the iterator to the pool, returning a list of results.
    pub fn add_transactions(
        &self,
        origin: TransactionOrigin,
        transactions: impl IntoIterator<Item = TransactionValidationOutcome<T::Transaction>>,
    ) -> Vec<PoolResult<TxHash>> {
        let added =
            transactions.into_iter().map(|tx| self.add_transaction(origin, tx)).collect::<Vec<_>>();

        // If at least one transaction was added successfully, then we enforce the pool size limits.
        let discarded =
            if added.iter().any(Result::is_ok) { self.discard_worst() } else { Default::default() };

        if discarded.is_empty() {
            return added
        }

        // It may happen that a newly added transaction is immediately discarded, so we need to
        // adjust the result here
        added
            .into_iter()
            .map(|res| match res {
                Ok(ref hash) if discarded.contains(hash) => {
                    Err(PoolError::DiscardedOnInsert(*hash))
                }
                other => other,
            })
            .collect()
    }

    /// Notify all listeners about a new pending transaction.
    fn on_new_pending_transaction(&self, pending: &AddedTransaction<T::Transaction>) {
        let tx_hash = *pending.hash();
        let propagate_allowed = pending.is_propagate_allowed();

        let mut transaction_listeners = self.pending_transaction_listener.lock();
        transaction_listeners.retain_mut(|listener| {
            if listener.kind.is_propagate_only() && !propagate_allowed {
                // only emit this hash to listeners that are only allowed to receive propagate only
                // transactions, such as network
                return !listener.sender.is_closed()
            }

            match listener.sender.try_send(tx_hash) {
                Ok(()) => true,
                Err(err) => {
                    if matches!(err, mpsc::error::TrySendError::Full(_)) {
                        debug!(
                            target: "txpool",
                            "[{:?}] failed to send pending tx; channel full",
                            tx_hash,
                        );
                        true
                    } else {
                        false
                    }
                }
            }
        });
    }

    /// Notify all listeners about a new pending transaction.
    fn on_new_transaction(&self, event: NewTransactionEvent<T::Transaction>) {
        let mut transaction_listeners = self.transaction_listener.lock();

        transaction_listeners.retain_mut(|listener| match listener.try_send(event.clone()) {
            Ok(()) => true,
            Err(err) => {
                if matches!(err, mpsc::error::TrySendError::Full(_)) {
                    debug!(
                        target: "txpool",
                        "skipping transaction on full transaction listener",
                    );
                    true
                } else {
                    false
                }
            }
        });
    }

    /// Notifies transaction listeners about changes after a block was processed.
    fn notify_on_new_state(&self, outcome: OnNewCanonicalStateOutcome) {
        let OnNewCanonicalStateOutcome { mined, promoted, discarded, block_hash } = outcome;

        let mut listener = self.event_listener.write();

        mined.iter().for_each(|tx| listener.mined(tx, block_hash));
        promoted.iter().for_each(|tx| listener.pending(tx, None));
        discarded.iter().for_each(|tx| listener.discarded(tx));
    }

    /// Fire events for the newly added transaction.
    fn notify_event_listeners(&self, tx: &AddedTransaction<T::Transaction>) {
        let mut listener = self.event_listener.write();

        match tx {
            AddedTransaction::Pending(tx) => {
                let AddedPendingTransaction { transaction, promoted, discarded, replaced } = tx;

                listener.pending(transaction.hash(), replaced.clone());
                promoted.iter().for_each(|tx| listener.pending(tx, None));
                discarded.iter().for_each(|tx| listener.discarded(tx));
            }
            AddedTransaction::Parked { transaction, replaced, .. } => {
                listener.queued(transaction.hash());
                if let Some(replaced) = replaced {
                    listener.replaced(replaced.clone(), *transaction.hash());
                }
            }
        }
    }

    /// Returns an iterator that yields transactions that are ready to be included in the block.
    pub(crate) fn best_transactions(&self) -> BestTransactions<T> {
        self.pool.read().best_transactions()
    }

    /// Returns an iterator that yields transactions that are ready to be included in the block with
    /// the given base fee.
    pub(crate) fn best_transactions_with_base_fee(
        &self,
        base_fee: u64,
    ) -> Box<dyn crate::traits::BestTransactions<Item = Arc<ValidPoolTransaction<T::Transaction>>>>
    {
        self.pool.read().best_transactions_with_base_fee(base_fee)
    }

    /// Returns all transactions from the pending sub-pool
    pub(crate) fn pending_transactions(&self) -> Vec<Arc<ValidPoolTransaction<T::Transaction>>> {
        self.pool.read().pending_transactions()
    }

    /// Returns all transactions from parked pools
    pub(crate) fn queued_transactions(&self) -> Vec<Arc<ValidPoolTransaction<T::Transaction>>> {
        self.pool.read().queued_transactions()
    }

    /// Returns all transactions in the pool
    pub(crate) fn all_transactions(&self) -> AllPoolTransactions<T::Transaction> {
        let pool = self.pool.read();
        AllPoolTransactions {
            pending: pool.pending_transactions(),
            queued: pool.queued_transactions(),
        }
    }

    /// Removes and returns all matching transactions from the pool.
    pub(crate) fn remove_transactions(
        &self,
        hashes: impl IntoIterator<Item = TxHash>,
    ) -> Vec<Arc<ValidPoolTransaction<T::Transaction>>> {
        let removed = self.pool.write().remove_transactions(hashes);

        let mut listener = self.event_listener.write();

        removed.iter().for_each(|tx| listener.discarded(tx.hash()));

        removed
    }

    /// Removes all transactions that are present in the pool.
    pub(crate) fn retain_unknown(&self, hashes: &mut Vec<TxHash>) {
        let pool = self.pool.read();
        hashes.retain(|tx| !pool.contains(tx))
    }

    /// Returns the transaction by hash.
    pub(crate) fn get(
        &self,
        tx_hash: &TxHash,
    ) -> Option<Arc<ValidPoolTransaction<T::Transaction>>> {
        self.pool.read().get(tx_hash)
    }

    /// Returns all transactions of the address
    pub(crate) fn get_transactions_by_sender(
        &self,
        sender: Address,
    ) -> Vec<Arc<ValidPoolTransaction<T::Transaction>>> {
        let sender_id = self.get_sender_id(sender);
        self.pool.read().get_transactions_by_sender(sender_id)
    }

    /// Returns all the transactions belonging to the hashes.
    ///
    /// If no transaction exists, it is skipped.
    pub(crate) fn get_all(
        &self,
        txs: impl IntoIterator<Item = TxHash>,
    ) -> Vec<Arc<ValidPoolTransaction<T::Transaction>>> {
        self.pool.read().get_all(txs).collect()
    }

    /// Notify about propagated transactions.
    pub(crate) fn on_propagated(&self, txs: PropagatedTransactions) {
        let mut listener = self.event_listener.write();

        txs.0.into_iter().for_each(|(hash, peers)| listener.propagated(&hash, peers))
    }

    /// Number of transactions in the entire pool
    pub(crate) fn len(&self) -> usize {
        self.pool.read().len()
    }

    /// Whether the pool is empty
    pub(crate) fn is_empty(&self) -> bool {
        self.pool.read().is_empty()
    }

    /// Enforces the size limits of pool and returns the discarded transactions if violated.
    pub(crate) fn discard_worst(&self) -> HashSet<TxHash> {
        self.pool.write().discard_worst().into_iter().map(|tx| *tx.hash()).collect()
    }
}

impl<V: TransactionValidator, T: TransactionOrdering> fmt::Debug for PoolInner<V, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoolInner").field("config", &self.config).finish_non_exhaustive()
    }
}

/// An active listener for new pending transactions.
#[derive(Debug)]
struct PendingTransactionListener {
    sender: mpsc::Sender<TxHash>,
    /// Whether to include transactions that should not be propagated over the network.
    kind: PendingTransactionListenerKind,
}

/// Tracks an added transaction and all graph changes caused by adding it.
#[derive(Debug, Clone)]
pub struct AddedPendingTransaction<T: PoolTransaction> {
    /// Inserted transaction.
    transaction: Arc<ValidPoolTransaction<T>>,
    /// Replaced transaction.
    replaced: Option<Arc<ValidPoolTransaction<T>>>,
    /// transactions promoted to the ready queue
    promoted: Vec<TxHash>,
    /// transaction that failed and became discarded
    discarded: Vec<TxHash>,
}

/// Represents a transaction that was added into the pool and its state
#[derive(Debug, Clone)]
pub enum AddedTransaction<T: PoolTransaction> {
    /// Transaction was successfully added and moved to the pending pool.
    Pending(AddedPendingTransaction<T>),
    /// Transaction was successfully added but not yet ready for processing and moved to a
    /// parked pool instead.
    Parked {
        /// Inserted transaction.
        transaction: Arc<ValidPoolTransaction<T>>,
        /// Replaced transaction.
        replaced: Option<Arc<ValidPoolTransaction<T>>>,
        /// The subpool it was moved to.
        subpool: SubPool,
    },
}

impl<T: PoolTransaction> AddedTransaction<T> {
    /// Returns whether the transaction is pending
    pub(crate) fn is_pending(&self) -> bool {
        matches!(self, AddedTransaction::Pending(_))
    }

    /// Returns the hash of the transaction
    pub(crate) fn hash(&self) -> &TxHash {
        match self {
            AddedTransaction::Pending(tx) => tx.transaction.hash(),
            AddedTransaction::Parked { transaction, .. } => transaction.hash(),
        }
    }
    /// Returns if the transaction should be propagated.
    pub(crate) fn is_propagate_allowed(&self) -> bool {
        match self {
            AddedTransaction::Pending(transaction) => transaction.transaction.propagate,
            AddedTransaction::Parked { transaction, .. } => transaction.propagate,
        }
    }

    /// Converts this type into the event type for listeners
    pub(crate) fn into_new_transaction_event(self) -> NewTransactionEvent<T> {
        match self {
            AddedTransaction::Pending(tx) => {
                NewTransactionEvent { subpool: SubPool::Pending, transaction: tx.transaction }
            }
            AddedTransaction::Parked { transaction, subpool, .. } => {
                NewTransactionEvent { transaction, subpool }
            }
        }
    }
}

/// Contains all state changes after a [`CanonicalStateUpdate`] was processed
#[derive(Debug)]
pub(crate) struct OnNewCanonicalStateOutcome {
    /// Hash of the block.
    pub(crate) block_hash: H256,
    /// All mined transactions.
    pub(crate) mined: Vec<TxHash>,
    /// Transactions promoted to the ready queue.
    pub(crate) promoted: Vec<TxHash>,
    /// transaction that were discarded during the update
    pub(crate) discarded: Vec<TxHash>,
}
