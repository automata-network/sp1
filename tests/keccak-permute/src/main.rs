#![no_main]
sp1_zkvm::entrypoint!(main);

use sp1_zkvm::syscalls::syscall_keccak_permute;

pub fn main() {
    for _ in 0..25 {
        let mut state = [1u64; 25];
        for i in 0..(1 << 20) {
            syscall_keccak_permute(state.as_mut_ptr());
        }
        println!("{:?}", state);
    }
}
