use std::collections::HashMap;
use std::thread::{self, JoinHandle};

use snafu::{ResultExt, Whatever};

use crate::models::{account::Account, transaction::Transaction};

pub struct TransactionProcessor {
    workers: Vec<Worker>,
}

impl TransactionProcessor {
    pub fn new(num_workers: usize) -> Self {
        let workers = (0..num_workers).map(|_| Worker::start()).collect();
        Self { workers }
    }

    pub fn process_txn(&self, txn: Transaction) -> Result<(), Whatever> {
        // Use the target account ID as the partitioning key for distributing transactions across
        // our workers.
        let account_id: u16 = txn.account_id().into();
        let worker_idx = account_id as usize % self.workers.len();
        self.workers[worker_idx].process_txn(txn)
    }

    pub fn shutdown(self) -> Result<Vec<Account>, Whatever> {
        self.workers
            .into_iter()
            .try_fold(vec![], |mut accounts, worker| {
                accounts.extend_from_slice(&worker.stop()?);
                Ok(accounts)
            })
    }
}

struct Worker {
    thread: JoinHandle<Vec<Account>>,
    txn_tx: crossbeam_channel::Sender<Option<Transaction>>,
}

impl Worker {
    fn start() -> Self {
        let (txn_tx, txn_rx) = crossbeam_channel::unbounded::<Option<Transaction>>();

        // Spin up our worker thread.
        let thread = thread::spawn(move || {
            // Each worker thread has local state of accounts for which it will be processing
            // transactions.
            let mut accounts = HashMap::new();

            while let Ok(Some(txn)) = txn_rx.recv() {
                if let Err(txn_err) = accounts
                    .entry(txn.account_id())
                    .or_insert_with(|| Account::new(txn.account_id()))
                    .process_txn(txn)
                {
                    tracing::warn!("A problem occurred while processing a transaction: {txn_err}");
                }
            }

            // When we have no more work to do, we will gather all of our account records
            // and return them.
            accounts.into_values().collect()
        });

        Self { thread, txn_tx }
    }

    fn process_txn(&self, txn: Transaction) -> Result<(), Whatever> {
        // Deliver the transaction to the worker's processing thread.
        self.txn_tx
            .send(Some(txn))
            .whatever_context("unable to deliver transaction to worker")
    }

    fn stop(self) -> Result<Vec<Account>, Whatever> {
        self.txn_tx
            .send(None)
            .whatever_context("unable to cleanly shutdown worker")?;
        Ok(self.thread.join().expect("worker thread panicked"))
    }
}
