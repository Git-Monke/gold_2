#[cfg(test)]
mod blockchain_validation {
    use std::collections::HashMap;

    use gold_2::*;

    #[test]
    fn calc_coinbase_test() {
        assert_eq!(calc_coinbase(10_000, 80), DEFAULT_COINBASE);
        assert_eq!(calc_coinbase(10_001, 80), DEFAULT_COINBASE);
        assert_eq!(calc_coinbase(10_081, 80), 195_031_250_000);
        assert_eq!(calc_coinbase(28_912, 10_000), 2_367_488_000);
        assert_eq!(calc_coinbase(183_928, 100_000), 13_594_983_000);
        assert_eq!(calc_coinbase(10_160, 80), 0);
    }

    fn create_dummy_blockchainstate() -> BlockchainState {
        let mut account_set: Accounts = HashMap::new();
        let mut name_set: Names = HashMap::new();

        let header = Header {
            prev_block_hash: [0; 32],
            merkle_root: [0; 32],
            time: 820,
            nonce: 0,
        };

        let previous_block = Block {
            header,
            txns: vec![],
            name_changes: vec![],
        };

        BlockchainState {
            account_set,
            name_set,
            difficulty: [
                0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ],
            height: 0,
            last_720_times: [100; 720],
            last_100_block_sizes: [10_000; 100],
            previous_block,
        }
    }
}
