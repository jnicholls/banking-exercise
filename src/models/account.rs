use std::collections::HashMap;

use derive_more::{Display, From, Into};
use rust_decimal::Decimal;
use serde::{
    ser::{self, SerializeStruct},
    Deserialize, Serialize,
};
use snafu::{OptionExt, Snafu};

use crate::models::transaction::{Transaction, TransactionId, TransactionType};

#[derive(Clone, Debug)]
pub struct Account {
    id: AccountId,
    available: Decimal,
    held: Decimal,
    locked: bool,
    txn_history: HashMap<TransactionId, Transaction>,
    disputed_txns: HashMap<TransactionId, Decimal>,
}

impl Account {
    pub fn new(id: AccountId) -> Self {
        let available = Default::default();
        let held = Default::default();
        let locked = false;
        let txn_history = Default::default();
        let disputed_txns = Default::default();

        Self {
            id,
            available,
            held,
            locked,
            txn_history,
            disputed_txns,
        }
    }
    pub fn id(&self) -> AccountId {
        self.id
    }

    pub fn available(&self) -> Decimal {
        self.available
    }

    pub fn held(&self) -> Decimal {
        self.held
    }

    pub fn total(&self) -> Decimal {
        self.available() - self.held()
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    pub fn process_txn(&mut self, txn: Transaction) -> Result<(), TransactionError> {
        use TransactionType::*;

        let span = tracing::debug_span!(
            "process_txn",
            account_id = %self.id,
            txn_id = %txn.id(),
            txn_type = %txn.txn_type(),
        );
        let _enter = span.enter();

        // If the provided transaction is not intended for our account, then we should not process
        // it.
        snafu::ensure!(
            self.id == txn.account_id(),
            WrongAccountSnafu {
                id: self.id,
                intended_account: txn.account_id(),
                txn_id: txn.id()
            }
        );

        // If the account is currently locked, then we cannot process any transactions for it.
        snafu::ensure!(!self.locked, AccountLockedSnafu { id: self.id });

        tracing::debug!(
            available = %self.available,
            held = %self.held,
            total = %self.total(),
            locked = self.locked,
            "preparing to process transaction..."
        );

        // Let's try to process the transaction...
        match txn.txn_type() {
            Deposit { amount } => {
                // For a Deposit, it is not expected to have already seen this transaction ID.
                snafu::ensure!(
                    !self.txn_history.contains_key(&txn.id()),
                    TransactionAlreadyProcessedSnafu {
                        id: self.id,
                        txn_id: txn.id(),
                    },
                );

                // Deposits will increase the available funds for the account.
                self.available += amount;

                // Store the transaction in case of future disputes.
                self.txn_history.insert(txn.id(), txn);
            }

            Withdrawal { amount } => {
                // For a Withdrawal, it is not expected to have already seen this transaction ID.
                snafu::ensure!(
                    !self.txn_history.contains_key(&txn.id()),
                    TransactionAlreadyProcessedSnafu {
                        id: self.id,
                        txn_id: txn.id(),
                    },
                );

                // Withdrawals will decrease the available funds for the account. However, if there
                // are not enough available funds, the transaction will fail.
                snafu::ensure!(
                    self.available >= amount,
                    InsufficientFundsSnafu {
                        id: self.id,
                        available: self.available,
                        needed: amount
                    }
                );

                self.available -= amount;

                // Store the transaction in case of future disputes.
                self.txn_history.insert(txn.id(), txn);
            }

            Dispute => {
                // Upon a dispute, we will look up a past Deposit or Withdrawal transaction and if
                // found, escrow account funds into its held assets.
                //
                // The description in the exercise did not make sense to me in all cases. It states:
                //
                //   This means that the clients available funds should decrease by the amount
                //   disputed, their held funds should increase by the amount disputed, while their
                //   total funds should remain the same.
                //
                // That description makes sense to me for temporarily undoing Deposit transactions.
                // However, it does not make sense to me for temporarily undoing Withdrawal
                // transactions. There were several areas of ambiguity in the exercise description,
                // particularly around handling Chargebacks, which I would expect to be handled
                // differently depending on a Deposit or a Withdrawal.
                //
                // Because there are automated test inputs, I'm going to interpret the exercise
                // requirements verbatim, and make no distinction between Deposits and Withdrawals.
                // Nevertheless, I believe the behavior of Dispute, Resolve, and Chargebacks would
                // be different depending on whether it is a Deposit or a Withdrawal transaction. It
                // could be the case that for a Withdrawal dispute, an accompanying Deposit
                // transaction is made along with the Dispute transaction, which would then make
                // this all proper logic. Since it wasn't mentioned, I will make this assumption
                // and test accordingly.

                // First, if a particular transaction is already in dispute, then we should ignore
                // this transaction.
                snafu::ensure!(
                    !self.disputed_txns.contains_key(&txn.id()),
                    TransactionAlreadyInDisputeSnafu {
                        id: self.id,
                        txn_id: txn.id()
                    }
                );

                // Attempt to lookup this transaction in our history of Deposits and Withdrawals.
                let past_txn =
                    self.txn_history
                        .get(&txn.id())
                        .context(TransactionNotFoundSnafu {
                            id: self.id,
                            txn_id: txn.id(),
                        })?;

                match past_txn.txn_type() {
                    Deposit { amount } | Withdrawal { amount } => {
                        // For disputing a transaction, we'll take the funds from the account's
                        // available funds and put them on hold.
                        self.available -= amount;
                        self.held += amount;
                        self.disputed_txns.insert(past_txn.id(), amount);
                    }

                    _ => (),
                }
            }

            Resolve => {
                // Attempt to lookup this transaction in our set of disputed transactions.
                let disputed_amount =
                    self.disputed_txns
                        .remove(&txn.id())
                        .context(TransactionNotInDisputeSnafu {
                            id: self.id,
                            txn_id: txn.id(),
                        })?;

                // For resolving a dispute, we'll restore funds to an account's
                // available balance.
                self.available += disputed_amount;
                self.held -= disputed_amount;
            }

            Chargeback => {
                // Attempt to lookup this transaction in our set of disputed transactions.
                let disputed_amount =
                    self.disputed_txns
                        .remove(&txn.id())
                        .context(TransactionNotInDisputeSnafu {
                            id: self.id,
                            txn_id: txn.id(),
                        })?;

                // For finalizing a dispute via a chargeback, we'll remove the disputed funds on
                // hold in the account.
                self.held -= disputed_amount;
                self.locked = true;
            }
        }

        // Note: For this exercise, only transactions that are Deposits or Withdrawals are recorded
        // for future reference. However, for audit purposes it would be good practice to record all
        // transaction types and whether or not they were successfully committed.

        tracing::debug!(
            available = %self.available,
            held = %self.held,
            total = %self.total(),
            locked = self.locked,
            "transaction successfully applied"
        );
        Ok(())
    }
}

impl ser::Serialize for Account {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        let mut s = serializer.serialize_struct("Account", 5)?;
        s.serialize_field("client", &self.id())?;
        s.serialize_field("available", &self.available())?;
        s.serialize_field("held", &self.held())?;
        s.serialize_field("total", &self.total())?;
        s.serialize_field("locked", &self.locked())?;
        s.end()
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Display,
    Eq,
    From,
    Hash,
    Into,
    PartialEq,
    PartialOrd,
    Ord,
    Serialize,
)]
#[display(fmt = "{_0}")]
#[serde(transparent)]
pub struct AccountId(u16);

#[derive(Debug, Snafu)]
pub enum TransactionError {
    #[snafu(display("The account with ID {id} is currently locked"))]
    AccountLocked { id: AccountId },

    #[snafu(display("The account with ID {id} has insufficient funds; funds available: {available}, funds needed: {needed}"))]
    InsufficientFunds {
        id: AccountId,
        available: Decimal,
        needed: Decimal,
    },

    #[snafu(display("The account with ID {id} already has transaction ID {txn_id} in dispute"))]
    TransactionAlreadyInDispute {
        id: AccountId,
        txn_id: TransactionId,
    },

    #[snafu(display("The account with ID {id} has already processed transaction ID {txn_id}"))]
    TransactionAlreadyProcessed {
        id: AccountId,
        txn_id: TransactionId,
    },

    #[snafu(display("The account with ID {id} had no past transaction with the ID {txn_id}"))]
    TransactionNotFound {
        id: AccountId,
        txn_id: TransactionId,
    },

    #[snafu(display(
        "The account with ID {id} had no transaction with the ID {txn_id} in dispute"
    ))]
    TransactionNotInDispute {
        id: AccountId,
        txn_id: TransactionId,
    },

    #[snafu(display("The account with ID {id} could not process the transaction ID {txn_id} as it is intended for account {intended_account}"))]
    WrongAccount {
        id: AccountId,
        intended_account: AccountId,
        txn_id: TransactionId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT_TXN_ID: AtomicU32 = AtomicU32::new(1);

    fn get_account() -> Account {
        Account::new(1.into())
    }

    fn next_txn_id() -> TransactionId {
        NEXT_TXN_ID.fetch_add(1, Ordering::SeqCst).into()
    }

    #[test]
    fn wrong_account() -> Result<(), Box<dyn Error>> {
        let amount = "100".parse()?;
        let mut account = get_account();
        let txn = Transaction::new(
            next_txn_id(),
            123.into(),
            TransactionType::Deposit { amount },
        );

        assert!(
            matches!(
                account.process_txn(txn),
                Err(TransactionError::WrongAccount { .. })
            ),
            "a transaction ought to target the correct account"
        );

        Ok(())
    }

    #[test]
    fn deposit() -> Result<(), Box<dyn Error>> {
        let amount = "100".parse()?;
        let mut account = get_account();
        let txn = Transaction::new(
            next_txn_id(),
            account.id(),
            TransactionType::Deposit { amount },
        );
        account.process_txn(txn)?;

        assert!(
            account.available() == amount && account.held() == Decimal::ZERO,
            "account should have 100 units available after deposit"
        );

        assert!(
            matches!(
                account.process_txn(txn),
                Err(TransactionError::TransactionAlreadyProcessed { .. })
            ),
            "cannot process the same transaction more than once"
        );

        Ok(())
    }

    #[test]
    fn withdrawal() -> Result<(), Box<dyn Error>> {
        let amount = "100".parse()?;
        let mut account = get_account();
        let txn = Transaction::new(
            next_txn_id(),
            account.id(),
            TransactionType::Deposit { amount },
        );
        account.process_txn(txn)?;

        assert!(
            account.available() == amount && account.held() == Decimal::ZERO,
            "account should have 100 units available after deposit"
        );

        let txn = Transaction::new(
            next_txn_id(),
            account.id(),
            TransactionType::Withdrawal { amount },
        );
        account.process_txn(txn)?;

        assert_eq!(
            account.total(),
            Decimal::ZERO,
            "account should have 0 units available after the withdrawal"
        );

        let txn = Transaction::new(
            next_txn_id(),
            account.id(),
            TransactionType::Withdrawal { amount },
        );
        assert!(
            matches!(
                account.process_txn(txn),
                Err(TransactionError::InsufficientFunds { .. })
            ),
            "account cannot withdrawal with insufficient funds"
        );

        Ok(())
    }

    #[test]
    fn bad_dispute() -> Result<(), Box<dyn Error>> {
        let mut account = get_account();
        let txn = Transaction::new(next_txn_id(), account.id(), TransactionType::Dispute);

        assert!(
            matches!(
                account.process_txn(txn),
                Err(TransactionError::TransactionNotFound { .. })
            ),
            "transaction cannot be put in dispute that does not exist"
        );

        Ok(())
    }

    #[test]
    fn resolve() -> Result<(), Box<dyn Error>> {
        let amount = "100".parse()?;
        let mut account = get_account();
        let txn = Transaction::new(
            next_txn_id(),
            account.id(),
            TransactionType::Deposit { amount },
        );
        account.process_txn(txn)?;

        assert!(
            account.available() == amount && account.held() == Decimal::ZERO,
            "account should have 100 units available after deposit"
        );

        let txn = Transaction::new(txn.id(), account.id(), TransactionType::Resolve);
        assert!(
            matches!(
                account.process_txn(txn),
                Err(TransactionError::TransactionNotInDispute { .. })
            ),
            "transaction that is not in dispute cannot be resolved"
        );

        let txn = Transaction::new(txn.id(), account.id(), TransactionType::Dispute);
        account.process_txn(txn)?;

        assert!(
            account.available() == Decimal::ZERO && account.held() == amount,
            "account should have 0 units available and 100 on hold after dispute"
        );

        let txn = Transaction::new(txn.id(), account.id(), TransactionType::Dispute);
        assert!(
            matches!(
                account.process_txn(txn),
                Err(TransactionError::TransactionAlreadyInDispute { .. })
            ),
            "transaction cannot be put into dispute more than once"
        );

        let txn = Transaction::new(txn.id(), account.id(), TransactionType::Resolve);
        account.process_txn(txn)?;

        assert!(
            account.available() == amount && account.held() == Decimal::ZERO,
            "account should have 100 units available after resolving the dispute"
        );

        Ok(())
    }

    #[test]
    fn chargeback() -> Result<(), Box<dyn Error>> {
        let amount = "100".parse()?;
        let mut account = get_account();
        let txn = Transaction::new(
            next_txn_id(),
            account.id(),
            TransactionType::Deposit { amount },
        );
        account.process_txn(txn)?;

        assert!(
            account.available() == amount && account.held() == Decimal::ZERO,
            "account should have 100 units available after deposit"
        );

        let txn = Transaction::new(txn.id(), account.id(), TransactionType::Dispute);
        account.process_txn(txn)?;

        assert!(
            account.available() == Decimal::ZERO && account.held() == amount,
            "account should have 0 units available and 100 on hold after dispute"
        );

        let txn = Transaction::new(txn.id(), account.id(), TransactionType::Chargeback);
        account.process_txn(txn)?;

        assert!(
            account.total() == Decimal::ZERO && account.locked(),
            "account should have 0 units available and be locked after a chargeback"
        );

        let txn = Transaction::new(
            next_txn_id(),
            account.id(),
            TransactionType::Deposit { amount },
        );
        assert!(
            matches!(
                account.process_txn(txn),
                Err(TransactionError::AccountLocked { .. })
            ),
            "account cannot process transactions while locked"
        );

        Ok(())
    }
}
