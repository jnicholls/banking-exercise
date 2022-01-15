use std::cmp;
use std::collections::{BinaryHeap, HashMap};
use std::thread::{self, JoinHandle};

use derive_more::Constructor;
use snafu::{ResultExt, Whatever};

use crate::models::{account::Account, transaction::Transaction};

#[derive(Clone, Constructor, Copy, Debug)]
pub struct OrderedTransaction {
    order: usize,
    txn: Transaction,
}

impl cmp::PartialEq for OrderedTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.order == other.order
    }
}

impl cmp::Eq for OrderedTransaction {}

impl cmp::Ord for OrderedTransaction {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.order.cmp(&other.order)
    }
}

impl cmp::PartialOrd for OrderedTransaction {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub struct TransactionProcessor {
    txn_dispatcher: JoinHandle<Vec<Worker>>,
    txn_tx: crossbeam_channel::Sender<Option<OrderedTransaction>>,
}

impl TransactionProcessor {
    pub fn new(num_workers: usize) -> Self {
        let workers: Vec<_> = (0..num_workers).map(|_| Worker::start()).collect();

        // Start up the transaction dispatcher. It will expect OrderedTransactions to come into its
        // work queue with an order range that logically starts at 0, and goes until the dispatcher
        // is shut down.
        let (txn_tx, txn_rx) = crossbeam_channel::unbounded::<Option<OrderedTransaction>>();
        let txn_dispatcher = thread::spawn(move || {
            // Maintain a priority queue of OrderedTransactions from lowest order to highest order.
            let mut heap: BinaryHeap<cmp::Reverse<OrderedTransaction>> = BinaryHeap::new();
            let mut next_expected = 0usize;

            // This method will deliver a transaction to a processing worker thread.
            let process_txn = |txn: Transaction| {
                // Use the target account ID as the partitioning key for distributing transactions across
                // our workers.
                let account_id: u16 = txn.account_id().into();
                let worker_idx = account_id as usize % workers.len();
                if let Err(e) = workers[worker_idx].process_txn(txn) {
                    tracing::error!(
                        "An error occurred when delivering a transaction to a worker thread: {e}"
                    );
                }
            };

            // As we receive ordered transactions off of our work queue:
            //   1. If it is not the next expected transaction, we will add it to our priority queue
            //      to process later when it is its turn.
            //   2. If it is the next expected transaction, we will send it to a processor right
            //      away. We will then continually check the top of our priority queue and process
            //      transactions whose turn is next, until the priority queue is empty or we come
            //      across a gap in the order. Then, we wait for the next transaction to come in off
            //      the work queue.
            while let Ok(Some(ordered_txn)) = txn_rx.recv() {
                if ordered_txn.order == next_expected {
                    process_txn(ordered_txn.txn);
                    next_expected += 1;

                    while let Some(&cmp::Reverse(ordered_txn)) = heap.peek() {
                        if ordered_txn.order == next_expected {
                            process_txn(ordered_txn.txn);
                            next_expected += 1;
                            heap.pop();
                        } else {
                            break;
                        }
                    }
                } else {
                    heap.push(cmp::Reverse(ordered_txn));
                }
            }

            workers
        });

        Self {
            txn_dispatcher,
            txn_tx,
        }
    }

    pub fn process_ordered_txn(&self, ordered_txn: OrderedTransaction) -> Result<(), Whatever> {
        self.txn_tx
            .send(Some(ordered_txn))
            .whatever_context("unable to deliver ordered transaction to dispatcher")
    }

    pub fn shutdown(self) -> Result<Vec<Account>, Whatever> {
        // Shut down the transaction dispatcher first.
        self.txn_tx
            .send(None)
            .whatever_context("unable to cleanly shutdown transaction dispatcher")?;

        // Then gather the workers' account outputs and amalgamate them together.
        self.txn_dispatcher
            .join()
            .expect("transaction dispatcher thread panicked")
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
