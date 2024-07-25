#![allow(unused)]

use crate::io::{self, FD_ECRECOVER_HOOK};
use crate::unconstrained;
use crate::utils::{AffinePoint, CurveOperations};
use crate::{syscall_secp256k1_add, syscall_secp256k1_decompress, syscall_secp256k1_double};

use anyhow::Context;
use anyhow::{anyhow, Result};
use core::convert::TryInto;
use k256::ecdsa::hazmat::bits2field;
use k256::ecdsa::signature::hazmat::PrehashVerifier;
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use k256::elliptic_curve::ff::PrimeFieldBits;
use k256::elliptic_curve::ops::Invert;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::elliptic_curve::PrimeField;
use k256::{PublicKey, Scalar, Secp256k1};

const NUM_WORDS: usize = 16;

#[derive(Copy, Clone)]
pub struct Secp256k1Operations;

impl CurveOperations<NUM_WORDS> for Secp256k1Operations {
    // The values are taken from https://en.bitcoin.it/wiki/Secp256k1.
    const GENERATOR: [u32; NUM_WORDS] = [
        385357720, 1509065051, 768485593, 43777243, 3464956679, 1436574357, 4191992748, 2042521214,
        4212184248, 2621952143, 2793755673, 4246189128, 235997352, 1571093500, 648266853,
        1211816567,
    ];
    fn add_assign(limbs: &mut [u32; NUM_WORDS], other: &[u32; NUM_WORDS]) {
        unsafe {
            syscall_secp256k1_add(limbs.as_mut_ptr(), other.as_ptr());
        }
    }

    fn double(limbs: &mut [u32; NUM_WORDS]) {
        unsafe {
            syscall_secp256k1_double(limbs.as_mut_ptr());
        }
    }
}

/// Decompresses a compressed public key using secp256k1_decompress precompile.
pub fn decompress_pubkey(compressed_key: &[u8; 33]) -> Result<[u8; 65]> {
    cfg_if::cfg_if! {
        if #[cfg(all(target_os = "zkvm", target_vendor = "succinct"))] {
            let mut decompressed_key: [u8; 64] = [0; 64];
            decompressed_key[..32].copy_from_slice(&compressed_key[1..]);
            let is_odd = match compressed_key[0] {
                2 => false,
                3 => true,
                _ => return Err(anyhow!("Invalid compressed key")),
            };
            unsafe {
                syscall_secp256k1_decompress(&mut decompressed_key, is_odd);
            }

            let mut result: [u8; 65] = [0; 65];
            result[0] = 4;
            result[1..].copy_from_slice(&decompressed_key);
            Ok(result)
        } else {
            let public_key = PublicKey::from_sec1_bytes(compressed_key).context("invalid pubkey")?;
            let bytes = public_key.to_encoded_point(false).to_bytes();
            let mut result: [u8; 65] = [0; 65];
            result.copy_from_slice(&bytes);
            Ok(result)
        }
    }
}

/// Verifies a secp256k1 signature using the public key and the message hash. If the s_inverse is
/// provided, it will be validated and used to verify the signature. Otherwise, the inverse of s
/// will be computed and used.
///
/// Warning: this function does not check if the key is actually on the curve.
pub fn verify_signature(
    pubkey: &[u8; 65],
    msg_hash: &[u8; 32],
    signature: &Signature,
    s_inverse: Option<&Scalar>,
) -> bool {
    cfg_if::cfg_if! {
        if #[cfg(all(target_os = "zkvm", target_vendor = "succinct"))] {
            let pubkey_x = Scalar::from_repr(bits2field::<Secp256k1>(&pubkey[1..33]).unwrap()).unwrap();
            let pubkey_y = Scalar::from_repr(bits2field::<Secp256k1>(&pubkey[33..]).unwrap()).unwrap();

            let mut pubkey_x_le_bytes = pubkey_x.to_bytes();
            pubkey_x_le_bytes.reverse();
            let mut pubkey_y_le_bytes = pubkey_y.to_bytes();
            pubkey_y_le_bytes.reverse();

            // Convert the public key to an affine point
            let affine = AffinePoint::<Secp256k1Operations, NUM_WORDS>::from(&pubkey_x_le_bytes, &pubkey_y_le_bytes);

            const GENERATOR: AffinePoint<Secp256k1Operations, NUM_WORDS> = AffinePoint::<Secp256k1Operations, NUM_WORDS>::generator_in_affine();

            let field = bits2field::<Secp256k1>(msg_hash);
            if field.is_err() {
                return false;
            }
            let field = Scalar::from_repr(field.unwrap()).unwrap();
            let z = field;
            let (r, s) = signature.split_scalars();
            let computed_s_inv;
            let s_inv = match s_inverse {
                Some(s_inv) => {
                    assert_eq!(s_inv * s.as_ref(), Scalar::ONE);
                    s_inv
                }
                None => {
                    computed_s_inv = s.invert();
                    &computed_s_inv
                }
            };

            let u1 = z * s_inv;
            let u2 = *r * s_inv;

            let res = double_and_add_base(&u1, &GENERATOR, &u2, &affine).unwrap();
            let mut x_bytes_be = [0u8; 32];
            for i in 0..8 {
                x_bytes_be[i * 4..(i * 4) + 4].copy_from_slice(&res.limbs[i].to_le_bytes());
            }
            x_bytes_be.reverse();

            let x_field = bits2field::<Secp256k1>(&x_bytes_be);
            if x_field.is_err() {
                return false;
            }
            *r == Scalar::from_repr(x_field.unwrap()).unwrap()
        } else {
            let public_key = PublicKey::from_sec1_bytes(pubkey);
            if public_key.is_err() {
                return false;
            }
            let public_key = public_key.unwrap();

            let verify_key = VerifyingKey::from(&public_key);
            let res = verify_key
                .verify_prehash(msg_hash, signature)
                .context("invalid signature");

            res.is_ok()
        }
    }
}

#[allow(non_snake_case)]
fn double_and_add_base(
    a: &Scalar,
    A: &AffinePoint<Secp256k1Operations, NUM_WORDS>,
    b: &Scalar,
    B: &AffinePoint<Secp256k1Operations, NUM_WORDS>,
) -> Option<AffinePoint<Secp256k1Operations, NUM_WORDS>> {
    let mut res: Option<AffinePoint<Secp256k1Operations, NUM_WORDS>> = None;
    let mut temp_A = *A;
    let mut temp_B = *B;

    let a_bits = a.to_le_bits();
    let b_bits = b.to_le_bits();
    for (a_bit, b_bit) in a_bits.iter().zip(b_bits) {
        if *a_bit {
            match res.as_mut() {
                Some(res) => res.add_assign(&temp_A),
                None => res = Some(temp_A),
            };
        }

        if b_bit {
            match res.as_mut() {
                Some(res) => res.add_assign(&temp_B),
                None => res = Some(temp_B),
            };
        }

        temp_A.double();
        temp_B.double();
    }

    res
}

/// Outside of the VM, computes the pubkey and s_inverse value from a signature and a message hash.
///
/// WARNING: The values are read from outside of the VM and are not constrained to be correct.
/// Either use `decompress_pubkey` and `verify_signature` to verify the results of this function, or
/// use `ecrecover`.
pub fn unconstrained_ecrecover(sig: &[u8; 65], msg_hash: &[u8; 32]) -> ([u8; 33], Scalar) {
    // The `unconstrained!` wrapper is used since none of these computations directly affect
    // the output values of the VM. The remainder of the function sets the constraints on the values
    // instead. Removing the `unconstrained!` wrapper slightly increases the cycle count.
    unconstrained! {
        let mut buf = [0; 65 + 32];
        let (buf_sig, buf_msg_hash) = buf.split_at_mut(sig.len());
        buf_sig.copy_from_slice(sig);
        buf_msg_hash.copy_from_slice(msg_hash);
        io::write(FD_ECRECOVER_HOOK, &buf);
    }

    let recovered_bytes: [u8; 33] = io::read_vec().try_into().unwrap();

    let s_inv_bytes: [u8; 32] = io::read_vec().try_into().unwrap();
    let s_inverse = Scalar::from_repr(bits2field::<Secp256k1>(&s_inv_bytes).unwrap()).unwrap();

    (recovered_bytes, s_inverse)
}

/// Recovers the public key that signed a given message hash using the provided signature.
///
/// # Arguments
///
/// * `sig` - A 65-byte array containing:
///   - The first 64 bytes: The ECDSA signature (r, s) in big-endian format
///   - The last byte: The recovery ID (0, 1, 2, or 3)
/// * `msg_hash` - A 32-byte array containing the Keccak-256 hash of the signed message
///
/// # Example
///
/// ```
/// let sig: [u8; 65] = [/* 64 bytes of signature */, /* 1 byte recovery ID */];
/// let msg_hash: [u8; 32] = [/* 32 bytes of message hash */];
/// let pubkey = ecrecover(&sig, &msg_hash)?;
/// let eth_address = keccak256(&pubkey[1..])[12..];
/// ```
pub fn ecrecover(sig: &[u8; 65], msg_hash: &[u8; 32]) -> Result<[u8; 65]> {
    let (pubkey, s_inv) = unconstrained_ecrecover(sig, msg_hash);
    let pubkey = decompress_pubkey(&pubkey).context("decompress pubkey failed")?;
    let verified = verify_signature(
        &pubkey,
        msg_hash,
        &Signature::from_slice(&sig[..64]).unwrap(),
        Some(&s_inv),
    );
    if verified {
        Ok(pubkey)
    } else {
        Err(anyhow!("failed to verify signature"))
    }
}
