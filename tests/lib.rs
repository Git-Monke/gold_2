#[cfg(test)]
mod blockchain_validation {
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
}
