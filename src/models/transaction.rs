use derive_more::{Constructor, Display, From, Into};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::models::account::AccountId;

#[derive(Clone, Constructor, Copy, Debug, Deserialize, Display)]
#[display(fmt = "ID: {id}, Account ID: {account_id}, Type: {txn_type}")]
pub struct Transaction {
    #[serde(rename = "tx")]
    id: TransactionId,

    #[serde(rename = "client")]
    account_id: AccountId,

    #[serde(flatten)]
    txn_type: TransactionType,
}

impl Transaction {
    pub fn id(&self) -> TransactionId {
        self.id
    }

    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    pub fn txn_type(&self) -> TransactionType {
        self.txn_type
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, Eq, From, Hash, Into, PartialEq, PartialOrd, Ord,
)]
#[display(fmt = "{_0}")]
#[serde(transparent)]
pub struct TransactionId(u32);

#[derive(Clone, Copy, Debug, Deserialize, Display)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum TransactionType {
    #[display(fmt = "Deposit {amount}")]
    Deposit { amount: Decimal },
    #[display(fmt = "Withdrawal ({amount})")]
    Withdrawal { amount: Decimal },
    #[display(fmt = "Dispute")]
    Dispute,
    #[display(fmt = "Resolve")]
    Resolve,
    #[display(fmt = "Chargeback")]
    Chargeback,
}
