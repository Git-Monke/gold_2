use secp256k1::{schnorr::Signature, Keypair, Secp256k1, XOnlyPublicKey};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, env::consts::OS};
use thiserror::Error;

#[derive(Debug)]
pub struct Block {
    pub header: Header,
    pub txns: Vec<Txn>,
    pub name_changes: Vec<RenameOp>,
}

#[derive(Debug, Clone)]
pub struct Txn {
    pub sender: Address,
    pub recievers: Vec<(Address, u64)>,
    pub signature: [u8; 64],
    pub fee: u64,
}

#[derive(Debug, Clone)]
pub enum Address {
    Key([u8; 32]),
    Name(String),
}

// In a rename operation, the fee is always paid by the new pk.
// If a person already owns the name, their pk must be the one that signs this txn. Otherwise, the new pk signs it.
#[derive(Debug)]
pub struct RenameOp {
    pub pk: [u8; 32],
    pub sig: [u8; 64],
    pub new_name: String,
    pub fee: u64,
}

#[derive(Debug)]
pub struct Header {
    pub prev_block_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub time: u64,
    pub nonce: u64,
}

pub struct BlockchainState {
    pub account_set: Accounts,
    pub name_set: Names,
    pub difficulty: [u8; 32],
    pub height: usize,
    pub last_720_times: [u64; 720],
    pub last_100_block_sizes: [usize; 100],
    pub previous_block: Block,
}

pub struct UndoBlock {
    removed_time: u64,
    removed_block_size: usize,
    txns: Vec<Txn>,
    name_changes: Vec<RenameOpUndo>,
}

pub struct RenameOpUndo {
    old_pk: Option<[u8; 32]>,
    name: String,
    fee: u64,
}

pub type Accounts = HashMap<[u8; 32], u64>;
pub type Names = HashMap<String, [u8; 32]>;

pub const HEADER_SIZE: usize = 80;

pub const TXN_FEES_PER_BYTE: u64 = 400_000;
pub const NAME_CHANGE_FEES_PER_BYTE: u64 = 100_000_000;

pub const DEFAULT_COINBASE: u64 = 200_000_000_000;

#[derive(Debug, Error)]
pub enum Error {
    #[error("The block failed to validate because {0}")]
    BlockValidationError(String),
    #[error("A txn failed to validate because {0}")]
    TxnValidationError(String),
    #[error("A transaction referenced a name that is not in the name set")]
    MissingDataError,
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

// ! TODO Add difficulty adjustment
// Takes a validated block and updates the account set
fn push_block(block: Block, blockchain_state: &mut BlockchainState) -> UndoBlock {
    let account_set = &mut blockchain_state.account_set;
    let name_set = &mut blockchain_state.name_set;

    // Any time a name change occurs, the data must be stored in case of an undo. This is later stored in the undo block.
    let mut name_undos = vec![];

    // Execute transactions
    for txn in block.txns.iter() {
        let total_spend = txn_total_spend(txn);

        account_set
            .entry(address_to_key_unchecked(&txn.sender, name_set))
            .and_modify(|a| *a -= total_spend);

        for reciever in txn.recievers.iter() {
            account_set
                .entry(address_to_key_unchecked(&reciever.0, name_set))
                .and_modify(|a| *a += reciever.1)
                .or_insert(reciever.1);
        }
    }

    // Execute name changes
    for op in block.name_changes.iter() {
        name_undos.push(RenameOpUndo {
            old_pk: name_set.get(&op.new_name.clone()).map(|v| *v),
            name: op.new_name.clone(),
            fee: op.fee,
        });

        name_set.insert(op.new_name.clone(), op.pk);

        if account_set[&op.pk] == op.fee {
            account_set.remove(&op.pk);
        } else {
            account_set.entry(op.pk).and_modify(|a| *a -= op.fee);
        }
    }

    // Shift the running totals
    let removed_time = push_to_front(&mut blockchain_state.last_720_times, block.header.time);
    let removed_block_size = push_to_front(
        &mut blockchain_state.last_100_block_sizes,
        block_size(&block),
    );

    UndoBlock {
        removed_time,
        removed_block_size,
        txns: block.txns,
        name_changes: name_undos,
    }
}

// ! TODO Add difficulty adjustment
// Takes the most recently applied block and undoes its transactions
// In a normal block, name changes are done after txns. So for the undo block, you must reverse the name-changes first.
fn pop_block(undo_block: &UndoBlock, blockchain_state: &mut BlockchainState) {
    let account_set = &mut blockchain_state.account_set;
    let name_set = &mut blockchain_state.name_set;

    for name_change in undo_block.name_changes.iter() {
        if name_change.old_pk.is_some() {
            name_set.insert(name_change.name.clone(), name_change.old_pk.unwrap());
        }
    }

    for txn in undo_block.txns.iter() {
        let total_spend = txn_total_spend(txn);

        account_set
            .entry(address_to_key_unchecked(&txn.sender, name_set))
            .and_modify(|a| *a += total_spend);

        for reciever in txn.recievers.iter() {
            // if the reciever has a balance equal to as much as they were sent in this txn, their balance will be 0 after. Remove from the account set.
            let key = address_to_key_unchecked(&reciever.0, name_set);

            if account_set[&key] == reciever.1 {
                account_set.remove(&key);
            } else {
                account_set.entry(key).and_modify(|a| *a -= reciever.1);
            }
        }
    }

    push_to_back(
        &mut blockchain_state.last_100_block_sizes,
        undo_block.removed_block_size,
    );

    push_to_back(
        &mut blockchain_state.last_720_times,
        undo_block.removed_time,
    );
}

// Takes a block and ensures that it meets all required rules
pub fn validate_block(block: &Block, blockchain_state: &BlockchainState) -> Result<(), Error> {
    // validate header

    if block.txns.len() < 1 {
        block_validation_error!("The block contains no transactions (coinbase txn is mandatory)")
    }

    if !(meets_difficulty(&hash_header(&block.header), &blockchain_state.difficulty)) {
        block_validation_error!("Header hash does not meet required difficulty");
    }

    if block.header.time < blockchain_state.previous_block.header.time {
        block_validation_error!("Block time is less than previous block time");
    }

    if merkle_root(&block.txns, &block.name_changes) != block.header.merkle_root {
        block_validation_error!("Header merkle root does not match calculated merkle root");
    }

    if hash(&encode_header(&blockchain_state.previous_block.header)) != block.header.prev_block_hash
    {
        block_validation_error!(
            "Header previous block hash does not match calculated hash of previous block"
        )
    }

    // validate other qualities

    let median_block_size = median_block_size(&blockchain_state.last_100_block_sizes);
    let block_size = block_size(&block);

    if block_size > 20_000 && block_size > 2 * median_block_size {
        block_validation_error!("Block is bigger than twice the median block size")
    }

    check_txns(
        &block.txns,
        blockchain_state,
        calc_coinbase(block_size, median_block_size),
    )?;

    check_name_changes(&block.name_changes, &blockchain_state.name_set)?;

    Ok(())
}

//
// --- NAME CHANGE VALIDATION FUNCTIONS
//

pub fn check_name_changes(op_list: &Vec<RenameOp>, name_set: &Names) -> Result<(), Error> {
    for op in op_list.iter() {
        check_name_change(&op, &name_set)?;
    }
    Ok(())
}

pub fn check_name_change(op: &RenameOp, name_set: &Names) -> Result<(), Error> {
    let pk = XOnlyPublicKey::from_byte_array(&op.pk).map_err(|_| {
        Error::TxnValidationError(
            "Rename operation used a pk that isn't a point on the curve".into(),
        )
    })?;

    let signer;

    if name_set.contains_key(&op.new_name) {
        signer = XOnlyPublicKey::from_byte_array(name_set.get(&op.new_name).unwrap()).unwrap();
    } else {
        signer = pk;
    }

    let encoded_op = encode_name_change(op);
    let sig = Signature::from_byte_array(op.sig);
    let secp = Secp256k1::new();

    secp.verify_schnorr(&sig, &hash(encoded_op.as_slice()), &pk)
        .map_err(|_| Error::TxnValidationError("Name-change signature was invalid".into()))?;

    let fee = (encoded_op.len() as u64) * NAME_CHANGE_FEES_PER_BYTE;

    if op.fee < fee {
        txn_validation_error!("Rename does not pay enough in fees");
    }

    if op.new_name.bytes().len() > 255 {
        txn_validation_error!("New name was greater than 255 bytes");
    }

    Ok(())
}

//
// --- TXN VALIDATION FUNCTIONS ---
//

pub fn check_txns(
    txn_list: &Vec<Txn>,
    blockchain_state: &BlockchainState,
    coinbase: u64,
) -> Result<(), Error> {
    let mut fees = 0;
    // The cumulative amount each user has spent in the block. Used for making sure multiple transactions don't add up to more than the users total balance
    let mut total_spend: HashMap<[u8; 32], u64> = HashMap::new();

    for (i, txn) in txn_list.iter().enumerate() {
        if i == 0 {
            continue;
        }

        check_txn(&txn, blockchain_state)?;
        let sender_key = address_to_key_unchecked(&txn.sender, &blockchain_state.name_set);

        // check_txn verifies the account is in the set, so this will always unwrap properly
        let balance = blockchain_state
            .account_set
            .get(&sender_key)
            .copied()
            .unwrap();
        let current_spend = total_spend.get(&sender_key).copied().unwrap_or(0);
        let spend = txn_total_spend(&txn);

        if (spend + current_spend) > balance {
            txn_validation_error!("Sender tried to spend more than their balance");
        }

        total_spend.entry(sender_key).and_modify(|a| *a += spend);

        fees += txn.fee;
    }

    if txn_list[0].recievers.len() > 1 {
        txn_validation_error!("Coinbase txn had more than 1 reciever")
    }

    if txn_list[0].recievers[0].1 > coinbase + fees {
        txn_validation_error!("Coinbase transaction produces more currency than allowed")
    }

    Ok(())
}

// checks the data is valid, the fee matches the txn size, but doesn't check if the amount they're trying to spend is valid
pub fn check_txn(txn: &Txn, blockchain_state: &BlockchainState) -> Result<(), Error> {
    let sender_key = address_to_key(&txn.sender, &blockchain_state.name_set)?;

    let key = XOnlyPublicKey::from_byte_array(&sender_key).map_err(|_| {
        Error::TxnValidationError("The sender's public key isn't a point on the curve".into())
    })?;

    let curve = secp256k1::Secp256k1::new();
    let sig = Signature::from_byte_array(txn.signature);

    let mut txn = txn.clone();
    txn.signature = [0; 64];

    curve
        .verify_schnorr(&sig, &encode_txn(&txn), &key)
        .map_err(|e| Error::TxnValidationError(e.to_string()))?;

    blockchain_state
        .account_set
        .get(&sender_key)
        .ok_or(Error::TxnValidationError(
            "The sender's pk isn't in the account set".into(),
        ))?;

    let size = encode_txn(&txn).len() as u64;
    let min_fee = TXN_FEES_PER_BYTE * size;

    if txn.fee < min_fee {
        txn_validation_error!("Txn doesn't pay enough in fees");
    }

    Ok(())
}

//
// --- HEADER VALIDATION FUNCTIONS ---
//

pub fn merkle_root(txn_list: &Vec<Txn>, name_changes: &Vec<RenameOp>) -> [u8; 32] {
    if txn_list.len() == 0 && name_changes.len() == 0 {
        return [0; 32];
    }

    let mut hashes: Vec<[u8; 32]> = txn_list.iter().map(|txn| txn_hash(&txn)).collect();

    hashes.extend(
        name_changes
            .iter()
            .map(|op| name_change_hash(&op))
            .collect::<Vec<[u8; 32]>>()
            .iter(),
    );

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

pub fn meets_difficulty(value: &[u8; 32], difficulty: &[u8; 32]) -> bool {
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

// --- RANDOM UTILITY FUNCTIONS

// hash is in a seperate function in case I decide to change the hashing alg later on
pub fn hash(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn encode_header(header: &Header) -> [u8; HEADER_SIZE] {
    let mut data = [0_u8; 80];

    data[0..32].copy_from_slice(&header.prev_block_hash[0..32]);
    data[32..64].copy_from_slice(&header.merkle_root[0..32]);
    data[64..72].copy_from_slice(&header.time.to_le_bytes());
    data[72..80].copy_from_slice(&header.nonce.to_le_bytes());

    data
}

pub fn txn_hash(txn: &Txn) -> [u8; 32] {
    hash(&encode_txn(txn))
}

pub fn encode_txn(txn: &Txn) -> Vec<u8> {
    let mut data = vec![];

    encode_address(&txn.sender, &mut data);
    data.push(txn.recievers.len() as u8);

    for reciever in txn.recievers.iter() {
        encode_address(&reciever.0, &mut data);
        data.extend(reciever.1.to_le_bytes().iter());
    }

    data.extend(txn.signature.iter());
    data.extend(txn.fee.to_le_bytes().iter());

    data
}

pub fn encode_address(address: &Address, data: &mut Vec<u8>) {
    match address {
        Address::Name(n) => {
            data.push(1);
            data.push(n.len() as u8);
            data.extend(n.as_bytes().iter());
        }
        Address::Key(k) => {
            data.push(0);
            data.extend(k.iter());
        }
    }
}

pub fn block_size(block: &Block) -> usize {
    let mut size = HEADER_SIZE;

    // 32 bit unsigned int representing the num of txns in the block
    size += 4;

    for txn in block.txns.iter() {
        size += encode_txn(&txn).len();
    }

    // 32 bit unsigned int representing the num of name-changes in the block
    size += 4;

    for rename in block.name_changes.iter() {
        size += encode_name_change(rename).len();
    }

    return size;
}

pub fn calc_coinbase(block_size: usize, median_block_size: usize) -> u64 {
    let block_size = block_size as f64;
    let median_block_size = median_block_size as f64;

    if block_size - median_block_size > 10_000f64 && block_size - 10_000f64 > median_block_size {
        // The first 10kb of any block is given for free. This is equal to about 70 transactions, or 1 transaction every 2 seconds.
        // This was chosen purposefully. If blocks remain exactly 10kb, that fixes blockchain growth at 2GB/yr. This is negligible.
        let percent = 1f64 - ((block_size - 10_000f64 - median_block_size) / median_block_size);
        println!("{percent}");
        ((DEFAULT_COINBASE as f64) / 1000_f64 * percent.powi(2)) as u64 * 1_000
    } else {
        DEFAULT_COINBASE
    }
}

pub fn txn_total_spend(txn: &Txn) -> u64 {
    let mut sum = 0;
    for output in txn.recievers.iter() {
        sum += output.1;
    }
    sum + txn.fee
}

pub fn name_change_hash(change: &RenameOp) -> [u8; 32] {
    hash(encode_name_change(change).as_slice())
}

pub fn encode_name_change(change: &RenameOp) -> Vec<u8> {
    let mut data: Vec<u8> = vec![];

    data.extend(change.pk.iter());
    data.extend(change.sig.iter());

    let bytes = change.new_name.bytes();
    data.push(bytes.len() as u8);
    data.extend(bytes);
    data.extend(change.fee.to_le_bytes());

    data
}

pub fn hash_header(header: &Header) -> [u8; 32] {
    hash(&encode_header(header))
}

pub fn median_block_size(values: &[usize; 100]) -> usize {
    let mut block_sizes = values.clone();
    block_sizes.sort_unstable();
    block_sizes[50]
}

// Will panic if the name is not in the Names set. Only use in functions where the txns have already been validated.
pub fn address_to_key_unchecked(address: &Address, names: &Names) -> [u8; 32] {
    match address {
        Address::Name(n) => *names.get(n).unwrap(),
        Address::Key(k) => *k,
    }
}

pub fn address_to_key(address: &Address, names: &Names) -> Result<[u8; 32], Error> {
    match address {
        Address::Name(n) => names.get(n).map(|k| *k).ok_or(Error::MissingDataError),
        Address::Key(k) => Ok(*k),
    }
}

pub fn push_to_back<T: Copy + Default>(arr: &mut [T], item: T) {
    for i in 1..arr.len() {
        arr[i + 1] = arr[i];
    }

    arr[0] = item;
}

// Could be optimized using Vec::pop_front, but this isn't a bottleneck so I will stick with simplicity.
pub fn push_to_front<T: Copy + Default>(arr: &mut [T], item: T) -> T {
    let output = arr[0];

    for i in 0..(arr.len() - 1) {
        arr[i] = arr[i + 1];
    }

    arr[arr.len() - 1] = item;

    output
}

// Signs a transaction and sets appropriate fees
pub fn finalize_txn(txn: &mut Txn, signer_keypair: &Keypair) {
    let secp = Secp256k1::new();
    let txn_size = encode_txn(&txn).len();
    txn.fee = txn_size as u64 * TXN_FEES_PER_BYTE;
    txn.signature = *secp
        .sign_schnorr(&encode_txn(txn), signer_keypair)
        .as_byte_array();
}
