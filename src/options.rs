use std::path::{Path, PathBuf};

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Options {
    #[structopt(
        name = "TRANSACTIONS_FILE",
        parse(from_os_str),
        help = "Path to a file containing transactions in CSV format.",
        validator(is_file)
    )]
    pub input_file: PathBuf,

    #[structopt(
        short = "w",
        long,
        help = "Number of transaction processing worker threads. Defaults to an optimum number based on the number of physical cores on the system.",
        validator(is_greater_than_zero)
    )]
    pub num_workers: Option<usize>,
}

fn is_file(path: String) -> Result<(), String> {
    if Path::new(&path).is_file() {
        Ok(())
    } else {
        Err(format!(
            "The specified path '{path}' is not an accessible file."
        ))
    }
}

fn is_greater_than_zero(num_workers: String) -> Result<(), String> {
    let num_workers = num_workers.parse::<usize>().map_err(|e| e.to_string())?;

    if num_workers > 0 {
        Ok(())
    } else {
        Err("The specified number of workers cannot be 0.".to_string())
    }
}
