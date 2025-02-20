#[cfg(test)]
mod blockchain_validation {
    use secp256k1::{rand::rngs::OsRng, Keypair};
    use std::collections::HashMap;

    use gold_2::*;
    use secp256k1::Secp256k1;

    #[test]
    fn calc_coinbase_test() {
        assert_eq!(calc_coinbase(10_000, 80), DEFAULT_COINBASE);
        assert_eq!(calc_coinbase(10_001, 80), DEFAULT_COINBASE);
        assert_eq!(calc_coinbase(10_081, 80), 195_031_250_000);
        assert_eq!(calc_coinbase(28_912, 10_000), 2_367_488_000);
        assert_eq!(calc_coinbase(183_928, 100_000), 13_594_983_000);
        assert_eq!(calc_coinbase(10_160, 80), 0);
    }

    fn create_dummy_account_set(default_account: [u8; 32], balance: u64) -> Accounts {
        let mut account_set: Accounts = HashMap::new();

        account_set.insert(default_account, balance);

        account_set
    }

    fn create_dummy_name_set(default_name: String, key: [u8; 32]) -> Names {
        let mut name_set: Names = HashMap::new();

        name_set.insert(default_name, key);

        name_set
    }

    fn create_dummy_blockchainstate() -> (BlockchainState, Keypair) {
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut OsRng);
        let serialized_pk = keypair.x_only_public_key().0.serialize();

        let mut account_set: Accounts = create_dummy_account_set(serialized_pk, 200_000_000_000);
        let mut name_set: Names = create_dummy_name_set("GitMonke".into(), serialized_pk);

        let header = Header {
            prev_block_hash: [0; 32],
            merkle_root: [0; 32],
            time: 820,
            nonce: 0,
        };

        (
            BlockchainState {
                account_set,
                name_set,
                difficulty: [
                    0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                ],
                height: 0,
                last_720_times: [100; 720],
                last_100_block_sizes: [10_000; 100],
                previous_block_header: header,
            },
            keypair,
        )
    }

    fn create_dummy_valid_block() -> (BlockchainState, Block, Keypair) {
        let (state, keypair) = create_dummy_blockchainstate();
        let prev_block_hash = hash_header(&state.previous_block_header);

        // GitMonke sends 100_000 to all 0's
        let mut example_txn = Txn {
            sender: Address::Name("GitMonke".into()),
            recievers: vec![(Address::Key([0; 32]), 100_000)],
            signature: [0; 64],
            fee: 0,
        };

        finalize_txn(&mut example_txn, &keypair);

        let txns = vec![example_txn];
        let renames = vec![];

        let mut block = Block {
            header: Header {
                prev_block_hash,
                merkle_root: [0; 32],
                time: 821,
                nonce: 2224777,
            },
            txns,
            name_changes: renames,
        };

        // inserting the coinbase txn needs refactoring

        // All 0's sends a coinbase txn to GitMonke
        let mut txn = Txn {
            sender: Address::Key([0; 32]),
            recievers: vec![(Address::Name("GitMonke".into()), 0)],
            signature: [0; 64],
            fee: 0,
        };

        block.txns.insert(0, txn);

        let coinbase = calc_coinbase(
            block_size(&block),
            median_block_size(&state.last_100_block_sizes),
        );

        block.txns[0].recievers[0].1 = coinbase;

        (state, block, keypair)
    }

    fn finalize_block(block: &mut Block, state: &BlockchainState) {
        block.header.merkle_root = merkle_root(&block.txns, &block.name_changes);

        while !meets_difficulty(&hash_header(&block.header), &state.difficulty) {
            block.header.nonce += 1;
        }
    }

    #[test]
    fn validate_block_test() {
        let (state, mut block, _) = create_dummy_valid_block();
        finalize_block(&mut block, &state);
        let result = validate_block(&block, &state);

        assert!(
            result.is_ok(),
            "Expected ok, got: {:?}",
            result.unwrap_err()
        )
    }

    #[test]
    fn invalid_address() {
        let (state, mut block, _) = create_dummy_valid_block();
        block.txns[1].sender = Address::Name("GitMone".into());
        finalize_block(&mut block, &state);

        let result = validate_block(&block, &state);

        assert!(matches!(result, Err(Error::MissingDataError)));
    }

    #[test]
    fn invalid_coinbase() {
        let (state, mut block, _) = create_dummy_valid_block();
        block.txns[0].recievers[0].1 = 300_000_000_000;
        finalize_block(&mut block, &state);

        let result = validate_block(&block, &state);

        if let Err(Error::TxnValidationError(msg)) = result {
            assert_eq!(msg, "Coinbase amount is invalid")
        } else {
            panic!(
                "Expected coinbase amount is invalid, got {}",
                result.unwrap_err()
            )
        }
    }

    #[test]
    fn invalid_time() {
        let (state, mut block, _) = create_dummy_valid_block();
        block.header.time = 99;
        finalize_block(&mut block, &state);

        let result = validate_block(&block, &state);

        if let Err(Error::BlockValidationError(msg)) = result {
            assert_eq!(msg, "Block time is less than previous block time")
        } else {
            panic!("Expected blocktime error, got {}", result.unwrap_err())
        }
    }

    #[test]
    fn test_pushblock() {
        let (mut state, mut block, _) = create_dummy_valid_block();

        push_block(block.clone(), &mut state);

        assert_eq!(state.last_100_block_sizes[99], block_size(&block));
        assert_eq!(state.last_720_times[719], 821);
        assert_eq!(state.previous_block_header, block.header);
    }

    #[test]
    fn test_popblock() {
        let (mut state, mut block, _) = create_dummy_valid_block();
        let state_before_push = state.clone();
        let prev_block_size = state.last_100_block_sizes[99];
        let prev_header = state.previous_block_header.clone();

        let undo_block = push_block(block.clone(), &mut state);
        pop_block(&undo_block, &mut state);

        assert_eq!(state, state_before_push);
    }
}
