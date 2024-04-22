//! Compatibility functions for rpc `Transaction` type.
use alloy_rpc_types::request::{TransactionInput, TransactionRequest};

use reth_primitives::{
    BlockNumber, Transaction as PrimitiveTransaction, TransactionKind as PrimitiveTransactionKind,
    TransactionSignedEcRecovered, TxType, B256,
};
#[cfg(feature = "optimism")]
use reth_rpc_types::optimism::OptimismTransactionFields;
use reth_rpc_types::{AccessList, AccessListItem, Transaction};
use signature::from_primitive_signature;
pub use typed::*;

mod signature;
mod typed;

/// Create a new rpc transaction result for a mined transaction, using the given block hash,
/// number, and tx index fields to populate the corresponding fields in the rpc result.
///
/// The block hash, number, and tx index fields should be from the original block where the
/// transaction was mined.
pub fn from_recovered_with_block_context(
    tx: TransactionSignedEcRecovered,
    block_hash: B256,
    block_number: BlockNumber,
    base_fee: Option<u64>,
    tx_index: usize,
) -> Transaction {
    fill(tx, Some(block_hash), Some(block_number), base_fee, Some(tx_index))
}

/// Create a new rpc transaction result for a _pending_ signed transaction, setting block
/// environment related fields to `None`.
pub fn from_recovered(tx: TransactionSignedEcRecovered) -> Transaction {
    fill(tx, None, None, None, None)
}

/// Create a new rpc transaction result for a _pending_ signed transaction, setting block
/// environment related fields to `None`.
fn fill(
    tx: TransactionSignedEcRecovered,
    block_hash: Option<B256>,
    block_number: Option<BlockNumber>,
    base_fee: Option<u64>,
    transaction_index: Option<usize>,
) -> Transaction {
    let signer = tx.signer();
    let mut signed_tx = tx.into_signed();

    let to = match signed_tx.kind() {
        PrimitiveTransactionKind::Create => None,
        PrimitiveTransactionKind::Call(to) => Some(*to),
    };

    #[allow(unreachable_patterns)]
    let (gas_price, max_fee_per_gas) = match signed_tx.tx_type() {
        TxType::Legacy => (Some(signed_tx.max_fee_per_gas()), None),
        TxType::Eip2930 => (Some(signed_tx.max_fee_per_gas()), None),
        TxType::Eip1559 | TxType::Eip4844 => {
            // the gas price field for EIP1559 is set to `min(tip, gasFeeCap - baseFee) +
            // baseFee`
            let gas_price = base_fee
                .and_then(|base_fee| {
                    signed_tx
                        .effective_tip_per_gas(Some(base_fee))
                        .map(|tip| tip + base_fee as u128)
                })
                .unwrap_or_else(|| signed_tx.max_fee_per_gas());

            (Some(gas_price), Some(signed_tx.max_fee_per_gas()))
        }
        _ => {
            // OP-deposit
            (None, None)
        }
    };

    // let chain_id = signed_tx.chain_id().map(U64::from);
    let chain_id = signed_tx.chain_id();
    let mut blob_versioned_hashes = None;

    #[allow(unreachable_patterns)]
    let access_list = match &mut signed_tx.transaction {
        PrimitiveTransaction::Legacy(_) => None,
        PrimitiveTransaction::Eip2930(tx) => Some(AccessList(
            tx.access_list
                .0
                .iter()
                .map(|item| AccessListItem {
                    address: item.address.0.into(),
                    storage_keys: item.storage_keys.iter().map(|key| key.0.into()).collect(),
                })
                .collect(),
        )),
        PrimitiveTransaction::Eip1559(tx) => Some(AccessList(
            tx.access_list
                .0
                .iter()
                .map(|item| AccessListItem {
                    address: item.address.0.into(),
                    storage_keys: item.storage_keys.iter().map(|key| key.0.into()).collect(),
                })
                .collect(),
        )),
        PrimitiveTransaction::Eip4844(tx) => {
            // extract the blob hashes from the transaction
            blob_versioned_hashes = Some(std::mem::take(&mut tx.blob_versioned_hashes));

            Some(AccessList(
                tx.access_list
                    .0
                    .iter()
                    .map(|item| AccessListItem {
                        address: item.address.0.into(),
                        storage_keys: item.storage_keys.iter().map(|key| key.0.into()).collect(),
                    })
                    .collect(),
            ))
        }
        _ => {
            // OP deposit tx
            None
        }
    };

    let signature =
        from_primitive_signature(*signed_tx.signature(), signed_tx.tx_type(), signed_tx.chain_id());

    Transaction {
        hash: signed_tx.hash(),
        nonce: signed_tx.nonce(),
        from: signer,
        to,
        value: signed_tx.value(),
        gas_price,
        max_fee_per_gas,
        max_priority_fee_per_gas: signed_tx.max_priority_fee_per_gas(),
        signature: Some(signature),
        gas: signed_tx.gas_limit() as u128,
        input: signed_tx.input().clone(),
        chain_id,
        access_list,
        transaction_type: Some(signed_tx.tx_type() as u8),

        // These fields are set to None because they are not stored as part of the transaction
        block_hash,
        block_number,
        transaction_index: transaction_index.map(|idx| idx as u64),
        // EIP-4844 fields
        max_fee_per_blob_gas: signed_tx.max_fee_per_blob_gas(),
        blob_versioned_hashes,
        // Optimism fields
        #[cfg(feature = "optimism")]
        other: OptimismTransactionFields {
            source_hash: signed_tx.source_hash(),
            mint: signed_tx.mint().map(reth_primitives::U128::from),
            is_system_tx: signed_tx.is_deposit().then_some(signed_tx.is_system_transaction()),
        }
        .into(),
        #[cfg(not(feature = "optimism"))]
        other: Default::default(),
    }
}

/// Convert [reth_primitives::AccessList] to [reth_rpc_types::AccessList]
pub fn from_primitive_access_list(
    access_list: reth_primitives::AccessList,
) -> reth_rpc_types::AccessList {
    reth_rpc_types::AccessList(
        access_list
            .0
            .into_iter()
            .map(|item| reth_rpc_types::AccessListItem {
                address: item.address.0.into(),
                storage_keys: item.storage_keys.into_iter().map(|key| key.0.into()).collect(),
            })
            .collect(),
    )
}

/// Convert [TransactionSignedEcRecovered] to [TransactionRequest]
pub fn transaction_to_call_request(tx: TransactionSignedEcRecovered) -> TransactionRequest {
    let from = tx.signer();
    let to = tx.transaction.to();
    let gas = tx.transaction.gas_limit();
    let value = tx.transaction.value();
    let input = tx.transaction.input().clone();
    let nonce = tx.transaction.nonce();
    let chain_id = tx.transaction.chain_id();
    let access_list = tx.transaction.access_list().cloned().map(from_primitive_access_list);
    let max_fee_per_blob_gas = tx.transaction.max_fee_per_blob_gas();
    let blob_versioned_hashes = tx.transaction.blob_versioned_hashes();
    let tx_type = tx.transaction.tx_type();

    // fees depending on the transaction type
    let (gas_price, max_fee_per_gas) = if tx.is_dynamic_fee() {
        (None, Some(tx.max_fee_per_gas()))
    } else {
        (Some(tx.max_fee_per_gas()), None)
    };
    let max_priority_fee_per_gas = tx.transaction.max_priority_fee_per_gas();

    TransactionRequest {
        from: Some(from),
        to,
        gas_price,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        gas: Some(gas as u128),
        value: Some(value),
        input: TransactionInput::new(input),
        nonce: Some(nonce),
        chain_id,
        access_list,
        max_fee_per_blob_gas,
        blob_versioned_hashes,
        transaction_type: Some(tx_type.into()),
        sidecar: None,
    }
}
