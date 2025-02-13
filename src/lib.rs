struct Block {
    header: Header,
    txns: Vec<Txn>,
}

struct Txn {
    sender: [u8; 32],
    reciever: [u8; 32],
    signature: [u8; 64],
    amount: u64,
    fee: u64,
}

struct Header {
    prev_block_hash: [u8; 32],
    merkle_root: [u8; 32],
    time: u64,
    nonce: u64,
}

struct BlockchainState {
    difficulty: [u8; 32],
    median_time: u64,
    network_time: u64,
    median_block_size: usize,
    previous_block: Block,
}

type Accounts = std::collections::HashMap<[u8; 32], u64>;

// Takes a validated block and updates the account set
fn push_block(block: &Block, account_set: &mut Accounts) {
    // For each txn, create necessary account numbers, then transfer those numbers
}

// Takes the most recently applied block and undoes its transactions
fn pop_block(block: &Block, account_set: &mut Accounts) {
    // For each txn, perform each txn in reverse and then delete all 0 accounts
    todo!()
}

// Takes a block and ensures that it meets all required rules
fn validate_block(block: &Block, blockchain_state: &BlockchainState) {
    // 1 check header
    // - Check header hash meets required difficulty
    // - Check header time is greater than the last block and no more than 30 mins away from network time
    // - Check the merkle root
    // - Check the previous block hash matches the previous block
    // 2 check txns
    todo!()
}
