use std::collections::HashMap;

use secp256k1::{schnorr::Signature, XOnlyPublicKey};
use sha2::{Digest, Sha256};

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

struct BlockchainState<'a> {
    account_set: &'a Accounts,
    difficulty: [u8; 32],
    median_time: u64,
    median_block_size: usize,
    previous_block: Block,
}

type Accounts = std::collections::HashMap<[u8; 32], u64>;

const HEADER_SIZE: usize = 80;
const TXN_SIZE: usize = 144;

pub enum Error {
    BlockValidationError(String),
    TxnValidationError(String),
}

macro_rules! block_validation_error {
    ($x:expr) => {
        return Err(Error::BlockValidationError($x.into()))
    };
}

macro_rules! txn_validation_error {
    ($x:expr) => {
        return Err(Error::TxnValidationError($x.into()))
    };
}

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
fn validate_block(block: &Block, blockchain_state: &BlockchainState) -> Result<(), Error> {
    // validate header

    if !(meets_difficulty(
        &hash(&encode_header(&block.header)),
        &blockchain_state.difficulty,
    )) {
        block_validation_error!("Header hash does not meet required difficulty");
    }

    if block.header.time < blockchain_state.previous_block.header.time {
        block_validation_error!("Block time is less than previous block time");
    }

    if merkle_root(&block.txns) != block.header.merkle_root {
        block_validation_error!("Header merkle root does not match calculated merkle root");
    }

    if hash(&encode_header(&blockchain_state.previous_block.header)) != block.header.prev_block_hash
    {
        block_validation_error!(
            "Header previous block hash does not match calculated hash of previous block"
        )
    }

    // validate other qualities

    if block_size(&block) > 2 * blockchain_state.median_block_size {
        block_validation_error!("Block is bigger than twice the median block size")
    }

    check_txns(
        &block.txns,
        blockchain_state.account_set,
        calc_coinbase(block_size(&block), blockchain_state.median_block_size),
    )?;

    Ok(())
}

pub fn check_txns(txn_list: &Vec<Txn>, account_set: &Accounts, coinbase: u64) -> Result<(), Error> {
    let mut fees = 0;
    // The cumulative amount each user has spent in the block. Used for making sure multiple transactions don't add up to more than the users total balance
    let mut total_spend: HashMap<[u8; 32], u64> = HashMap::new();

    for (i, txn) in txn_list.iter().enumerate() {
        if i == 1 {
            continue;
        }

        check_txn(&txn, account_set)?;

        // check_txn verifies the account is in the set, so this will always unwrap properly
        let balance = account_set.get(&txn.sender).copied().unwrap();
        let current_spend = total_spend.get(&txn.sender).copied().unwrap_or(0);

        if (txn.amount + txn.fee + current_spend) > balance {
            txn_validation_error!("Sender tried to spend more than their balance");
        }

        total_spend
            .entry(txn.sender)
            .and_modify(|a| *a += txn.amount + txn.fee);

        fees += txn.fee;
    }

    if txn_list[0].amount > coinbase + fees {
        txn_validation_error!("Coinbase transaction produces more currency than allowed")
    }

    Ok(())
}

// checks the data is valid, but doesn't check if the amount they're trying to spend is valid
pub fn check_txn(txn: &Txn, account_set: &Accounts) -> Result<(), Error> {
    let key = XOnlyPublicKey::from_byte_array(&txn.sender).map_err(|_| {
        Error::TxnValidationError("The sender's public key isn't a point on the curve".into())
    })?;

    let curve = secp256k1::Secp256k1::new();
    let sig = Signature::from_byte_array(txn.signature);

    curve
        .verify_schnorr(&sig, &encode_txn(&txn), &key)
        .map_err(|e| Error::TxnValidationError(e.to_string()))?;

    *account_set
        .get(&txn.sender)
        .ok_or(Error::TxnValidationError(
            "The sender's pk isn't in the account set".into(),
        ))?;

    Ok(())
}

pub fn block_size(block: &Block) -> usize {
    HEADER_SIZE + TXN_SIZE * block.txns.len()
}

pub fn calc_coinbase(block_size: usize, median_block_size: usize) -> u64 {
    let base = 1_000_000_000f64;

    let block_size = block_size as f64;
    let median_block_size = median_block_size as f64;

    if block_size > 50_000f64 && block_size > median_block_size {
        // calculate fraction, multiplfy by base, convert to u64 to cut off the non-deterministic decimal places, add back the precision.
        (base * (1f64 - ((block_size - median_block_size) / (median_block_size))).powi(2)) as u64
            * 1_000
    } else {
        1_000_000_000_000
    }
}

pub fn merkle_root(txn_list: &Vec<Txn>) -> [u8; 32] {
    let mut hashes: Vec<[u8; 32]> = txn_list.iter().map(|txn| txn_hash(&txn)).collect();
    let mut new_hashes = vec![];

    while hashes.len() > 1 {
        for i in (0..hashes.len()).step_by(2) {
            let mut data = [0; 64];
            data[0..32].copy_from_slice(&hashes[i]);

            if i + 1 < hashes.len() {
                data[32..64].copy_from_slice(&hashes[i + 1]);
            } else {
                data[32..64].copy_from_slice(&hashes[i]);
            }

            new_hashes.push(sha2::Sha256::digest(data).try_into().unwrap());
        }

        hashes = new_hashes;
        new_hashes = vec![];
    }

    hashes[0]
}

pub fn txn_hash(txn: &Txn) -> [u8; 32] {
    hash(&encode_txn(txn))
}

pub fn encode_txn(txn: &Txn) -> [u8; TXN_SIZE] {
    let mut data = [0_u8; 144];

    data[0..32].copy_from_slice(&txn.sender);
    data[32..64].copy_from_slice(&txn.reciever);
    data[64..128].copy_from_slice(&txn.signature);
    data[128..136].copy_from_slice(&txn.amount.to_le_bytes());
    data[136..=144].copy_from_slice(&txn.fee.to_le_bytes());

    data
}

pub fn encode_header(header: &Header) -> [u8; HEADER_SIZE] {
    let mut data = [0_u8; 80];

    data[0..32].copy_from_slice(&header.prev_block_hash[0..32]);
    data[32..64].copy_from_slice(&header.merkle_root[0..32]);
    data[64..72].copy_from_slice(&header.time.to_le_bytes());
    data[72..80].copy_from_slice(&header.nonce.to_le_bytes());

    data
}

fn meets_difficulty(value: &[u8; 32], difficulty: &[u8; 32]) -> bool {
    for i in 0..32 {
        if value[i] < difficulty[i] {
            return true;
        }

        if value[i] > difficulty[i] {
            return false;
        }
    }

    return true;
}

// hash is in a seperate function in case I decide to change the hashing alg later on
fn hash(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}
