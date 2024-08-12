# PoRD-SQ: Semi-decentralized Sequencer based on my ZK Consensus Protocol
This project is based on my semi-decentralized consensus protocol [Proof of Random Delta](https://github.com/jonas089/PoRD)

Read the PoRD whitepaper [here](https://github.com/jonas089/PoRD/tree/master/whitepaper)

# What is the motivation behind this product?
Decentralized sequencing is a huge challenge in the L2 Blockchain space and many companies are developing solutions
that are overly complex with respect to consensus (and tokenomics). Having reviewed some existing approaches and 
"work-in-progress" repositories, I decided that we want something more straightforward and are willing to compromize the
degree of decentralization.

In my personal opinion [PoRD](https://github.com/jonas089/PoRD) establishes a good balance of decentralization and 
simplicity. Because of this I have decided to implement a general-purpose node on top of the PoRD abstract / "whitepaper" - I know that at the time of writing PoRD is not 
mathematically sophisticated enough to be called a real "whitepaper" - anyways, this is a functional approach with reasonable security guarantees, not a theoretically bulletproof one.

# How does PoRD-SQ work?
PoRD Nodes collect arbitrary Transactions and store them in a temporary database (a transaction pool). Every era the PoRD consensus ceremony is held to select a validator from the fixed validator set to create the next Block. This selection process is based on perfectly deterministic, yet difficult to predict, Zero Knowledge random numbers.

## Merkle Commitments
The exact nature of the merkle commitments is yet to be defined. Either Blocks will be stored as leafs in a Merkle Tree, or all Transactions in those Blocks will be stored as leafs in a Merkle Tree. Once the consensus and block generation is in place I will decide on this and implement it. Finally a reporting oracle will pass the root hash on to a Blockchain where merkle paths for sequenced transactions can be verified.

