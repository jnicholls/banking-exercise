use std::error::Error;
use std::fs::File;
use std::io::{self, BufReader, BufWriter};

use structopt::StructOpt;

use banking_exercise::{
    models::transaction::Transaction, options::Options, processor::TransactionProcessor,
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

    // Stream in the transactions from the CSV file, and pass them to our transaction processor.
    tracing::info!("Starting up transaction processing...");
    let mut csv_reader = csv::Reader::from_reader(BufReader::new(file));
    for result in csv_reader.deserialize() {
        let txn: Transaction = result?;
        tracing::info!(%txn);
        txn_processor.process_txn(txn)?;
    }

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
