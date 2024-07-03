#![no_main]
sp1_zkvm::entrypoint!(main);

use tiny_keccak::{Hasher, Keccak};

pub fn main() {
    let num_cases = sp1_zkvm::io::read::<usize>();
    for _ in 0..num_cases {
        let input = sp1_zkvm::io::read::<Vec<u8>>();

        let mut output = [0u8; 32];
        for i in 0..(1 << 16) {
            let mut hasher = Keccak::v256();
            hasher.update(&input);
            hasher.finalize(&mut output);
        }

        sp1_zkvm::io::commit(&output);
    }
}
