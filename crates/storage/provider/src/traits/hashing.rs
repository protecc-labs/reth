use auto_impl::auto_impl;
use reth_db::models::BlockNumberAddress;
use reth_interfaces::Result;
use reth_primitives::{Account, Address, BlockNumber, StorageEntry, H256};
use std::{
    collections::{BTreeSet, HashMap},
    ops::{Range, RangeInclusive},
};

/// Hashing Writer
#[auto_impl(&, Arc, Box)]
pub trait HashingWriter: Send + Sync {
    /// Unwind and clear account hashing.
    ///
    /// # Returns
    ///
    /// Set of hashed keys of updated accounts.
    fn unwind_account_hashing(&self, range: RangeInclusive<BlockNumber>) -> Result<BTreeSet<H256>>;

    /// Inserts all accounts into [reth_db::tables::AccountHistory] table.
    ///
    /// # Returns
    ///
    /// Set of hashed keys of updated accounts.
    fn insert_account_for_hashing(
        &self,
        accounts: impl IntoIterator<Item = (Address, Option<Account>)>,
    ) -> Result<BTreeSet<H256>>;

    /// Unwind and clear storage hashing
    ///
    /// # Returns
    ///
    /// Mapping of hashed keys of updated accounts to their respective updated hashed slots.
    fn unwind_storage_hashing(
        &self,
        range: Range<BlockNumberAddress>,
    ) -> Result<HashMap<H256, BTreeSet<H256>>>;

    /// Iterates over storages and inserts them to hashing table.
    ///
    /// # Returns
    ///
    /// Mapping of hashed keys of updated accounts to their respective updated hashed slots.
    fn insert_storage_for_hashing(
        &self,
        storages: impl IntoIterator<Item = (Address, impl IntoIterator<Item = StorageEntry>)>,
    ) -> Result<HashMap<H256, BTreeSet<H256>>>;

    /// Calculate the hashes of all changed accounts and storages, and finally calculate the state
    /// root.
    ///
    /// The hashes are calculated from `fork_block_number + 1` to `current_block_number`.
    ///
    /// The resulting state root is compared with `expected_state_root`.
    fn insert_hashes(
        &self,
        range: RangeInclusive<BlockNumber>,
        end_block_hash: H256,
        expected_state_root: H256,
    ) -> Result<()>;
}
