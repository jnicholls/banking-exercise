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
