# Banking Exercise

This repository contains a simple transaction processing engine to demonstrate Rust proficiency.

## How to Run

*Requires rustc 1.58+.*

As requested, one can run the application using `cargo run`.

Optionally, one can provide `RUST_LOG` env_logger syntax to display logs written to stderr. However, if one's attached to a TTY and not redirecting stderr to a file, it can drastically reduce the performance of the application as it blocks on TTY I/O. Thus, I would not suggest it for large transaction inputs.

## Test Samples

There are a few samples included in the repository under the `samples` folder:
* test1.csv - Basic example as provided in the exercise description.
* test2.csv - A more complex example that exercises all transaction types.
* large-test.csv - A large file of 1 million transactions distributed across 8 clients.

E.g.

```
cargo run --release -- samples/large-test.csv
```

There are also unit tests for testing client transactions:

```
cargo test
```

## Solution

For the multi-threaded deserialization solution that improves performance by about 43%, please see the [parallel-deserialization branch](https://github.com/jnicholls/banking-exercise/tree/parallel-deserialization).

For performance against a large amount of transactions, I created a small multi-threaded processing engine to spread the burden of transaction history management and lookup. The main thread is focused on I/O and deserialization. Transactions are partitioned by client (which I call Account in the code). Given that the transactions are chronological, we can divide them up per client and schedule them to a particular worker thread for processing. Each worker processes one transaction at a time, in the order they are received. The history management is all in-memory, no durable storage is used.
