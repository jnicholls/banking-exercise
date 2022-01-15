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

For the single-threaded deserialization solution, please see the [master branch](https://github.com/jnicholls/banking-exercise).

For performance against a large amount of transactions, I created a small multi-threaded processing engine to spread the burden of transaction history management and lookup. The processing workflow is as follows:

* The main thread opens the CSV file and begins reading it record by record.
* Each record is tagged with a count, and delivered to a processing thread pool, where records are deserialized into transactions in parallel.
* After deserialization, transactions along with their tagged count are delivered to a serialization thread that is responsible for ordered transaction dispatch.
* The ordered dispatcher receives tagged transactions.
  * If the transaction is not the next expected one, it will be added to a priority queue.
  * If the transaction is the next expected one, it will be scheduled to the transaction processing worker, partitioned by client ID (what I call Account in the code). Then, the priority queue will be repeatedly consulted for the next expected transaction until it is either emptied, or an expected transaction is missing.
* When the main thread has finished reading the CSV, it will wait for all transaction processing to complete.
* When transactions are completely processed, the final account state is delivered to the main thread for print out.

This paralleled deserialization approach increases performance by about 43% on my MacBook Pro Quad-Core i7.
