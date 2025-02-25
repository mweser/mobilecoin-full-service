// Copyright (c) 2020-2021 MobileCoin Inc.

//! Service for managing ledger materials and MobileCoin protocol objects.

use crate::{
    db::{
        models::{TransactionLog, Txo},
        transaction_log::{TransactionID, TransactionLogModel},
        txo::TxoModel,
    },
    WalletService,
};
use mc_blockchain_types::{Block, BlockContents, BlockVersion};
use mc_connection::{BlockchainConnection, RetryableBlockchainConnection, UserTxConnection};
use mc_fog_report_validation::FogPubkeyResolver;
use mc_ledger_db::Ledger;
use mc_ledger_sync::NetworkState;
use mc_transaction_core::{
    ring_signature::KeyImage,
    tokens::Mob,
    tx::{Tx, TxOut},
    Token, TokenId,
};

use crate::db::WalletDbError;
use displaydoc::Display;
use rayon::prelude::*; // For par_iter
use std::{cmp, collections::BTreeMap, convert::TryFrom, iter::empty};

/// Errors for the Address Service.
#[derive(Display, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum LedgerServiceError {
    /// Error interacting with the database: {0}
    Database(WalletDbError),

    /// Error with LedgerDB: {0}
    LedgerDB(mc_ledger_db::Error),

    /// Error decoding prost: {0}
    ProstDecode(mc_util_serial::DecodeError),

    /** No transaction object associated with this transaction. Note,
     * received transactions do not have transaction objects.
     */
    NoTxInTransaction,
}

impl From<mc_ledger_db::Error> for LedgerServiceError {
    fn from(src: mc_ledger_db::Error) -> Self {
        Self::LedgerDB(src)
    }
}

impl From<mc_util_serial::DecodeError> for LedgerServiceError {
    fn from(src: mc_util_serial::DecodeError) -> Self {
        Self::ProstDecode(src)
    }
}

impl From<WalletDbError> for LedgerServiceError {
    fn from(src: WalletDbError) -> Self {
        Self::Database(src)
    }
}

/// Trait defining the ways in which the wallet can interact with and manage
/// ledger objects and interfaces.
pub trait LedgerService {
    /// Get the total number of blocks on the ledger.
    fn get_network_block_height(&self) -> Result<u64, LedgerServiceError>;

    fn get_transaction_object(&self, transaction_id_hex: &str) -> Result<Tx, LedgerServiceError>;

    fn get_txo_object(&self, txo_id_hex: &str) -> Result<TxOut, LedgerServiceError>;

    fn get_block_object(
        &self,
        block_index: u64,
    ) -> Result<(Block, BlockContents), LedgerServiceError>;

    fn contains_key_image(&self, key_image: &KeyImage) -> Result<bool, LedgerServiceError>;

    fn get_network_fees(&self) -> BTreeMap<TokenId, u64>;

    fn get_network_block_version(&self) -> BlockVersion;
}

impl<T, FPR> LedgerService for WalletService<T, FPR>
where
    T: BlockchainConnection + UserTxConnection + 'static,
    FPR: FogPubkeyResolver + Send + Sync + 'static,
{
    fn get_network_block_height(&self) -> Result<u64, LedgerServiceError> {
        let network_state = self.network_state.read().expect("lock poisoned");
        match network_state.highest_block_index_on_network() {
            Some(index) => Ok(index + 1),
            None => Ok(0),
        }
    }

    fn get_transaction_object(&self, transaction_id_hex: &str) -> Result<Tx, LedgerServiceError> {
        let conn = self.wallet_db.get_conn()?;
        let transaction_log =
            TransactionLog::get(&TransactionID(transaction_id_hex.to_string()), &conn)?;
        let tx: Tx = mc_util_serial::decode(&transaction_log.tx)?;
        Ok(tx)
    }

    fn get_txo_object(&self, txo_id_hex: &str) -> Result<TxOut, LedgerServiceError> {
        let conn = self.wallet_db.get_conn()?;
        let txo_details = Txo::get(txo_id_hex, &conn)?;

        let txo: TxOut = mc_util_serial::decode(&txo_details.txo)?;
        Ok(txo)
    }

    fn get_block_object(
        &self,
        block_index: u64,
    ) -> Result<(Block, BlockContents), LedgerServiceError> {
        let block = self.ledger_db.get_block(block_index)?;
        let block_contents = self.ledger_db.get_block_contents(block_index)?;
        Ok((block, block_contents))
    }

    fn contains_key_image(&self, key_image: &KeyImage) -> Result<bool, LedgerServiceError> {
        Ok(self.ledger_db.contains_key_image(key_image)?)
    }

    fn get_network_fees(&self) -> BTreeMap<TokenId, u64> {
        let mut fees = self
            .peer_manager
            .conns()
            .par_iter()
            .filter_map(|conn| conn.fetch_block_info(empty()).ok())
            .map(|block_info| block_info.minimum_fees)
            .reduce(BTreeMap::new, |mut acc, fees| {
                for (token_id, fee) in fees {
                    acc.entry(token_id)
                        .and_modify(|e| *e = cmp::max(*e, fee))
                        .or_insert(fee);
                }
                acc
            });
        fees.entry(Mob::ID)
            .and_modify(|e| *e = cmp::max(*e, Mob::MINIMUM_FEE))
            .or_insert(Mob::MINIMUM_FEE);
        fees
    }

    fn get_network_block_version(&self) -> BlockVersion {
        if self.peer_manager.is_empty() {
            BlockVersion::MAX
        } else {
            let block_version = self
                .peer_manager
                .conns()
                .par_iter()
                .filter_map(|conn| conn.fetch_block_info(empty()).ok())
                .map(|block_info| block_info.network_block_version)
                .max()
                .unwrap_or(*BlockVersion::MAX);

            BlockVersion::try_from(block_version).unwrap_or(BlockVersion::MAX)
        }
    }
}
