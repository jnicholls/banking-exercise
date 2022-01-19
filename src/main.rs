use std::error::Error;
use std::fs::File;
use std::io::{self, BufReader, BufWriter};

use rayon::iter::{ParallelBridge, ParallelIterator};
use structopt::StructOpt;

use banking_exercise::{
    models::transaction::Transaction,
    options::Options,
    processor::{OrderedTransaction, TransactionProcessor},
};

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(io::stderr)
        .init();

    let opts = Options::from_args();

    // Start up our multi-threaded transaction processor, with the specified number of workers. If
    // no worker count was specified, we default to the number of physical cores on the system,
    // accounting for the main thread that is focused on I/O and deserialization. This is an optimum
    // thread arrangement.
    let num_workers = opts
        .num_workers
        .unwrap_or_else(|| usize::max(num_cpus::get_physical(), 2) - 1);
    let txn_processor = TransactionProcessor::new(num_workers);

    // Open up the CSV file of transactions.
    let file = File::open(opts.input_file)?;

    // Stream in the transactions from the CSV file, deserialize each in parallel, and pass them to
    // our transaction processor.
    tracing::info!("Starting up transaction processing...");
    let mut csv_reader = csv::Reader::from_reader(BufReader::new(file));
    let headers = csv_reader.byte_headers()?.clone();
    let mut record_count = 0usize;

    // Each CSV record read in is tagged with the order in which it was present in the CSV file.
    // This tuple of (order, ByteRecord) is then dispatched to a thread pool where records are
    // deserialized into Transactions in parallel, which is a reasonably CPU-intensive task.
    // This leaves the main thread in charge of the blocking I/O and thread pool dispatch.
    //
    // Once ByteRecords are deserialized into Transactions, the (order, Transaction) tuple is
    // used to construct an OrderedTransaction that is in turn sent to the TransactionProcessor.
    // Since the dispatch to the TransactionProcessor is occurring from multiple deserialization
    // threads, they will likely end up out of chronological order. The TransactionProcessor will
    // re-order them before sending them on to be processed by its internal worker threads.
    csv_reader
        .byte_records()
        .map(|br_result| {
            br_result.map(|br| {
                let br = (record_count, br);
                record_count += 1;
                br
            })
        })
        .par_bridge()
        .map(|br_result| {
            br_result.and_then(|(order, br)| {
                let txn = br.deserialize::<Transaction>(Some(&headers))?;
                Ok((order, txn))
            })
        })
        .try_for_each(|br_result| {
            let (order, txn) = br_result.map_err(|e| e.to_string())?;
            let ordered_txn = OrderedTransaction::new(order, txn);
            txn_processor
                .process_ordered_txn(ordered_txn)
                .map_err(|e| e.to_string())
        })?;

    // When we've finished passing all transactions to the processor, we'll initiate its shutdown.
    // The processor will complete all inflight transactions, if any, and then return to us the
    // latest state of all the accounts that were created during transaction processing.
    tracing::info!("Finished reading transactions, waiting for processing to complete...");
    let accounts = txn_processor.shutdown()?;
    tracing::info!("All transactions processed!");

    // We now will dump all the account data to stdout.
    let mut writer = csv::Writer::from_writer(BufWriter::new(io::stdout()));
    for account in accounts {
        writer.serialize(&account)?;
    }
    writer.flush()?;

    Ok(())
}
