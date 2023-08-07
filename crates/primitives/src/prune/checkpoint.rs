use crate::{prune::PruneMode, BlockNumber};
use reth_codecs::{main_codec, Compact};

/// Saves the pruning progress of a stage.
#[main_codec]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(test, derive(Default))]
pub struct PruneCheckpoint {
    /// Highest pruned block number.
    pub block_number: BlockNumber,
    /// Prune mode.
    pub prune_mode: PruneMode,
}
