# PoRD-SQ: Semi-decentralized Sequencer based on my ZK Consensus Protocol
⚠️ This Project is not production ready and in a Research stage of development ⚠️

This project is based on my half-baked consensus protocol [Proof of Random Delta](https://github.com/jonas089/PoRD)

Read the PoRD whitepaper [here](https://github.com/jonas089/PoRD/tree/master/whitepaper)

# What is the motivation behind this product?
Decentralized sequencing is a huge challenge in the L2 Blockchain space and many companies are developing solutions
that are overly complex with respect to consensus (and tokenomics). Having reviewed some existing approaches and 
"work-in-progress" repositories, I decided that we want something more straightforward and are willing to compromize the
degree of decentralization.

In my personal opinion [PoRD](https://github.com/jonas089/PoRD) establishes a good balance of decentralization and 
simplicity. Because of this I have decided to implement a general-purpose node on top of the PoRD abstract / "whitepaper" - I know that at the time of writing PoRD is not 
mathematically sophisticated enough to be called a real "whitepaper" - anyways, this is a functional approach with reasonable security guarantees, not a theoretically bulletproof one.

It was pointed out that the ZK Random number generator can be replaced with a general VRF, I am researching this and might choose to replace the ZK Randomness with a VRF if it makes sense (as it will likely be faster).

# How does PoRD-SQ work?
PoRD Nodes collect arbitrary Transactions and store them in a temporary database (a transaction pool). Every era the PoRD consensus ceremony is held to select a validator from the fixed validator set to create the next Block. This selection process is based on perfectly deterministic, yet difficult to predict, Zero Knowledge random numbers | VRF numbers.

# Run basic E2E test with 2 Nodes (manually, in-memory)
Split your terminal into 2 sessions and run:
```bash
API_HOST_WITH_PORT=127.0.0.1:8081 cargo run
```
in Terminal A,

and

```bash
cargo run
```
in Terminal B

This will start the Network and initiate the Block generation process:
![example](https://github.com/jonas089/PoRD-sequencer/blob/master/resources/demo.png)

To submit an example Transaction to both nodes, run:

```bash
cargo test test_schedule_transactions
```

# API Routes

## Internal
```rust
        .route("/schedule", post(schedule))
        .route("/commit", post(commit))
        .route("/propose", post(propose))
```
## External
```rust
        .route("/get/pool", get(get_pool))
        .route("/get/commitments", get(get_commitments))
        .route("/get/block/:height", get(get_block))
```

To view a Block when running the example setup, request `127.0.0.1:8080/get/block/<id>`, or `127.0.0.1:8081/get/block/<id>`.

## Merkle Commitments
TBD
