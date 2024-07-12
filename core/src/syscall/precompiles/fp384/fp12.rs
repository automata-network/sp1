use crate::air::{BaseAirBuilder, MachineAir, Polynomial, SP1AirBuilder, WORD_SIZE};
use crate::bytes::event::ByteRecord;
use crate::bytes::ByteLookupEvent;
use crate::memory::{value_as_limbs, MemoryReadCols, MemoryWriteCols};
use crate::operations::field::field_op::{FieldOpCols, FieldOperation};
use crate::operations::field::params::{FieldParameters, NumWords};
use crate::operations::field::params::{Limbs, NumLimbs};
use crate::operations::IsZeroOperation;
use crate::runtime::{ExecutionRecord, Program, Syscall, SyscallCode};
use crate::runtime::{MemoryReadRecord, MemoryWriteRecord};
use crate::stark::MachineRecord;
use crate::syscall::precompiles::SyscallContext;
use crate::utils::ec::uint::U384Field;
use crate::utils::{
    bytes_to_words_le, limbs_from_access, limbs_from_prev_access, pad_rows, words_to_bytes_le,
    words_to_bytes_le_vec,
};
use amcl::bls381::fp12;
use amcl::bls381::rom::MODULUS;
use generic_array::GenericArray;
use itertools::{izip, Itertools};
use num::Zero;
use num::{BigUint, One};
use p3_air::AirBuilder;
use p3_air::{Air, BaseAir};
use p3_field::AbstractField;
use p3_field::PrimeField32;
use p3_matrix::dense::RowMajorMatrix;
use p3_matrix::Matrix;
use serde::{Deserialize, Serialize};
use sp1_derive::AlignedBorrow;
use std::borrow::{Borrow, BorrowMut};
use std::iter::Sum;
use std::marker::PhantomData;
use std::mem::size_of;
use typenum::Unsigned;

use super::Fp12;

/// The number of columns in the FpMulCols.
const NUM_COLS: usize = size_of::<Fp12MulCols<u8>>();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fp12MulEvent {
    pub lookup_id: usize,
    pub shard: u32,
    pub channel: u32,
    pub clk: u32,
    pub a_ptr: u32,
    pub a: Vec<u32>,
    pub b_ptr: u32,
    pub b: Vec<u32>,
    pub a_memory_records: Vec<MemoryWriteRecord>,
    pub b_memory_records: Vec<MemoryReadRecord>,
}

type WordsFieldElement = <U384Field as NumWords>::WordsFieldElement;
const LIMBS_PER_WORD: usize = WordsFieldElement::USIZE;
const FP12_WORDS: usize = 12 * LIMBS_PER_WORD;
const NUM_FP_MULS: usize = 144;

#[derive(Debug, Clone, AlignedBorrow)]
#[repr(C)]
pub struct SumOfProductsAuxillaryCols<F> {
    pub b10_p_b11: FieldOpCols<F, U384Field>, // b.c1.c0 + b.c1.c1;
    pub b10_m_b11: FieldOpCols<F, U384Field>, // b.c1.c0 - b.c1.c1;
    pub b20_p_b21: FieldOpCols<F, U384Field>, // b.c2.c0 + b.c2.c1;
    pub b20_m_b21: FieldOpCols<F, U384Field>, // b.c2.c0 - b.c2.c1;
}

impl<F: PrimeField32> SumOfProductsAuxillaryCols<F> {
    fn pad_rows(&mut self) {
        [
            &mut self.b10_p_b11,
            &mut self.b10_m_b11,
            &mut self.b20_p_b21,
            &mut self.b20_m_b21,
        ]
        .iter_mut()
        .for_each(|dest| {
            dest.populate(
                &mut vec![],
                0,
                0,
                &BigUint::zero(),
                &BigUint::zero(),
                FieldOperation::Mul,
            );
        });
    }
}

#[derive(Debug, Clone, AlignedBorrow)]
#[repr(C)]
pub struct SumOfProductsCols<F> {
    pub a1_t_b1: FieldOpCols<F, U384Field>,
    pub a2_t_b2: FieldOpCols<F, U384Field>,
    pub a3_t_b3: FieldOpCols<F, U384Field>,
    pub a4_t_b4: FieldOpCols<F, U384Field>,
    pub a5_t_b5: FieldOpCols<F, U384Field>,
    pub a6_t_b6: FieldOpCols<F, U384Field>,

    pub sum1: FieldOpCols<F, U384Field>,
    pub sum2: FieldOpCols<F, U384Field>,
    pub sum3: FieldOpCols<F, U384Field>,
    pub sum4: FieldOpCols<F, U384Field>,
    pub sum5: FieldOpCols<F, U384Field>,
}

impl<F: PrimeField32> SumOfProductsCols<F> {
    fn pad_rows(&mut self) {
        [
            &mut self.a1_t_b1,
            &mut self.a2_t_b2,
            &mut self.a3_t_b3,
            &mut self.a4_t_b4,
            &mut self.a5_t_b5,
            &mut self.a6_t_b6,
            &mut self.sum1,
            &mut self.sum2,
            &mut self.sum3,
            &mut self.sum4,
            &mut self.sum5,
        ]
        .iter_mut()
        .for_each(|dest| {
            dest.populate(
                &mut vec![],
                0,
                0,
                &BigUint::zero(),
                &BigUint::zero(),
                FieldOperation::Mul,
            );
        });
    }
}

#[derive(Debug, Clone, AlignedBorrow)]
#[repr(C)]
pub struct Fp6MulCols<F> {
    pub aux: SumOfProductsAuxillaryCols<F>,
    // [a.c0.c0, -a.c0.c1, a.c1.c0, -a.c1.c1, a.c2.c0, -a.c2.c1]
    // [b.c0.c0, b.c0.c1, b20_m_b21, b20_p_b21, b10_m_b11, b10_p_b11]
    pub c00: SumOfProductsCols<F>,

    // [a.c0.c0, a.c0.c1, a.c1.c0, a.c1.c1, a.c2.c0, a.c2.c1],
    // [b.c0.c1, b.c0.c0, b20_p_b21, b20_m_b21, b10_p_b11, b10_m_b11],
    pub c01: SumOfProductsCols<F>,

    // [a.c0.c0, -a.c0.c1, a.c1.c0, -a.c1.c1, a.c2.c0, -a.c2.c1],
    // [b.c1.c0, b.c1.c1, b.c0.c0, b.c0.c1, b20_m_b21, b20_p_b21],
    pub c10: SumOfProductsCols<F>,

    // [a.c0.c0, a.c0.c1, a.c1.c0, a.c1.c1, a.c2.c0, a.c2.c1],
    // [b.c1.c1, b.c1.c0, b.c0.c1, b.c0.c0, b20_p_b21, b20_m_b21],
    pub c11: SumOfProductsCols<F>,

    // [a.c0.c0, -a.c0.c1, a.c1.c0, -a.c1.c1, a.c2.c0, -a.c2.c1],
    // [b.c2.c0, b.c2.c1, b.c1.c0, b.c1.c1, b.c0.c0, b.c0.c1],
    pub c20: SumOfProductsCols<F>,

    // [a.c0.c0, a.c0.c1, a.c1.c0, a.c1.c1, a.c2.c0, a.c2.c1],
    // [b.c2.c1, b.c2.c0, b.c1.c1, b.c1.c0, b.c0.c1, b.c0.c0],
    pub c21: SumOfProductsCols<F>,
}

impl<F: PrimeField32> Fp6MulCols<F> {
    fn pad_rows(&mut self) {
        self.aux.pad_rows();
        self.c00.pad_rows();
        self.c01.pad_rows();
        self.c10.pad_rows();
        self.c11.pad_rows();
        self.c20.pad_rows();
        self.c21.pad_rows();
    }
}

#[derive(Debug, Clone, AlignedBorrow)]
#[repr(C)]
pub struct Fp6AddCols<F> {
    pub a00_p_b00: FieldOpCols<F, U384Field>, // a.c0.c0 + b.c0.c0
    pub a01_p_b01: FieldOpCols<F, U384Field>, // a.c0.c1 + b.c0.c1
    pub a10_p_b10: FieldOpCols<F, U384Field>, // a.c1.c0 + b.c1.c0
    pub a11_p_b11: FieldOpCols<F, U384Field>, // a.c1.c1 + b.c1.c1
    pub a20_p_b20: FieldOpCols<F, U384Field>, // a.c2.c0 + b.c2.c0
    pub a21_p_b21: FieldOpCols<F, U384Field>, // a.c2.c1 + b.c2.c1
}

impl<F: PrimeField32> Fp6AddCols<F> {
    fn pad_rows(&mut self) {
        [
            &mut self.a00_p_b00,
            &mut self.a01_p_b01,
            &mut self.a10_p_b10,
            &mut self.a11_p_b11,
            &mut self.a20_p_b20,
            &mut self.a21_p_b21,
        ]
        .iter_mut()
        .for_each(|dest| {
            dest.populate(
                &mut vec![],
                0,
                0,
                &BigUint::zero(),
                &BigUint::zero(),
                FieldOperation::Mul,
            );
        });
    }
}

#[derive(Debug, Clone, AlignedBorrow)]
#[repr(C)]
pub struct Fp6MulByNonResidueCols<F> {
    pub c00: FieldOpCols<F, U384Field>, // a.c2.c0 - a.c2.c1
    pub c01: FieldOpCols<F, U384Field>, // a.c2.c0 + a.c2.c1

    pub c10: FieldOpCols<F, U384Field>, // a.c0.c0
    pub c11: FieldOpCols<F, U384Field>, // a.c0.c1

    pub c20: FieldOpCols<F, U384Field>, // a.c1.c0
    pub c21: FieldOpCols<F, U384Field>, // a.c1.c1
}

impl<F: PrimeField32> Fp6MulByNonResidueCols<F> {
    fn pad_rows(&mut self) {
        [
            &mut self.c00,
            &mut self.c01,
            &mut self.c10,
            &mut self.c11,
            &mut self.c20,
            &mut self.c21,
        ]
        .iter_mut()
        .for_each(|dest| {
            dest.populate(
                &mut vec![],
                0,
                0,
                &BigUint::zero(),
                &BigUint::zero(),
                FieldOperation::Mul,
            );
        });
    }
}

#[derive(Debug, Clone, AlignedBorrow)]
#[repr(C)]
pub struct AuxFp12MulCols<F> {
    pub aa: Fp6MulCols<F>,             // self.c0 * other.c0;
    pub bb: Fp6MulCols<F>,             // self.c1 * other.c1;
    pub o: Fp6AddCols<F>,              // other.c0 + other.c1;
    pub y1: Fp6AddCols<F>,             // a.c1 + a.c0
    pub y2: Fp6MulCols<F>,             // (a.c1 + a.c0) * a.o
    pub y3: Fp6AddCols<F>,             // (a.c1 + a.c0) * o  - aa
    pub y: Fp6AddCols<F>,              // (a.c1 + a.c0) * o  - aa - bb
    pub x1: Fp6MulByNonResidueCols<F>, // bb * non_residue
    pub x: Fp6AddCols<F>,              // bb * non_residue + aa
}

impl<F: PrimeField32> AuxFp12MulCols<F> {
    fn pad_rows(&mut self) {
        self.aa.pad_rows();
        self.bb.pad_rows();
        self.o.pad_rows();
        self.y1.pad_rows();
        self.y2.pad_rows();
        self.y3.pad_rows();
        self.y.pad_rows();
        self.x1.pad_rows();
        self.x.pad_rows();
    }
}

/// A set of columns for the FpMul operation.
#[derive(Debug, Clone, AlignedBorrow)]
#[repr(C)]
pub struct Fp12MulCols<F> {
    pub is_real: F,
    pub shard: F,
    pub channel: F,
    pub clk: F,
    pub nonce: F,

    pub a_access: GenericArray<MemoryWriteCols<F>, WordsFieldElement>,
    pub b_access: GenericArray<MemoryWriteCols<F>, WordsFieldElement>,

    pub a_ptr: u32,
    pub b_ptr: u32,
    pub a: Vec<u32>,
    pub b: Vec<u32>,

    pub output: AuxFp12MulCols<F>,
}

#[derive(Default)]
pub struct Fp12MulChip;

impl Fp12MulChip {
    const MODULUS: &'static [u8] = &[
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
    ];

    // fn populate_field_ops(
    //     blu_events: &mut Vec<ByteLookupEvent>,
    //     shard: u32,
    //     channel: u32,
    //     cols: &mut WeierstrassAddAssignCols<F, E::BaseField>,

    // )
}

impl<F: PrimeField32> MachineAir<F> for Fp12MulChip {
    type Record = ExecutionRecord;

    type Program = Program;

    fn name(&self) -> String {
        "Fp12Mul".to_string()
    }

    fn generate_trace(&self, input: &Self::Record, output: &mut Self::Record) -> RowMajorMatrix<F> {
        let rows_and_records = input
            .fp12_mul_events
            .chunks(1)
            .map(|events| {
                let mut records = ExecutionRecord::default();
                let mut new_byte_lookup_events = Vec::new();

                let rows = events
                    .iter()
                    .map(|event| {
                        let mut row: [F; NUM_COLS] = [F::zero(); NUM_COLS];
                        let cols: &mut Fp12MulCols<F> = row.as_mut_slice().borrow_mut();
                        // let x = [0..BigUint::from_bytes_le(bytes_to_words_le::<48>(&event.x[0..48]));
                        let x = (0..12)
                            .map(|i| {
                                BigUint::from_bytes_le(&words_to_bytes_le::<48>(
                                    &event.x[i * 48..(i + 1) * 48],
                                ))
                            })
                            .collect_vec();
                        let x: [BigUint; 12] = x.try_into().unwrap();

                        let y = (0..12)
                            .map(|i| {
                                BigUint::from_bytes_le(&words_to_bytes_le::<48>(
                                    &event.y[i * 48..(i + 1) * 48],
                                ))
                            })
                            .collect_vec();
                        let y: [BigUint; 12] = y.try_into().unwrap();

                        let modulus = BigUint::from_bytes_le(Self::MODULUS);

                        // Assign basic values to the columns.
                        cols.is_real = F::one();
                        cols.shard = F::from_canonical_u32(event.shard);
                        cols.channel = F::from_canonical_u32(event.channel);
                        cols.clk = F::from_canonical_u32(event.clk);
                        cols.a0_ptr = F::from_canonical_u32(event.a0_ptr);
                        cols.b0_ptr = F::from_canonical_u32(event.b0_ptr);

                        // Populate memory columns.
                        for i in 0..LIMBS_PER_WORD {
                            cols.a_access[i].populate(
                                event.channel,
                                event.a_memory_records[i],
                                &mut new_byte_lookup_events,
                            );
                            cols.b_access[i].populate(
                                event.channel,
                                event.b_memory_records[i],
                                &mut new_byte_lookup_events,
                            );
                        }

                        let constraints: Fp12MulChipTrace<F> = Fp12MulChipTrace::new(
                            event.shard,
                            event.channel,
                            new_byte_lookup_events.clone(),
                            modulus,
                        );
                        constraints.fp12_mul(&mut cols.output, x, y);
                        new_byte_lookup_events = constraints.new_byte_lookup_events;
                        row
                    })
                    .collect_vec();
                records.add_byte_lookup_events(new_byte_lookup_events);
                (rows, records)
            })
            .collect::<Vec<_>>();

        //  Generate the trace rows for each event.
        let mut rows = Vec::new();
        for (row, mut record) in rows_and_records {
            rows.extend(row);
            output.append(&mut record);
        }

        pad_rows(&mut rows, || {
            let mut row: [F; NUM_COLS] = [F::zero(); NUM_COLS];
            let cols: &mut Fp12MulCols<F> = row.as_mut_slice().borrow_mut();
            cols.output.pad_rows();
            row
        });

        // Convert the trace to a row major matrix.
        let mut trace =
            RowMajorMatrix::new(rows.into_iter().flatten().collect::<Vec<_>>(), NUM_COLS);

        // Write the nonces to the trace.
        for i in 0..trace.height() {
            let cols: &mut Fp12MulCols<F> =
                trace.values[i * NUM_COLS..(i + 1) * NUM_COLS].borrow_mut();
            cols.nonce = F::from_canonical_usize(i);
        }

        trace
    }

    fn included(&self, shard: &Self::Record) -> bool {
        !shard.fp_mul_events.is_empty()
    }
}

impl Syscall for Fp12MulChip {
    fn num_extra_cycles(&self) -> u32 {
        0
    }

    fn execute(&self, rt: &mut SyscallContext, arg1: u32, arg2: u32) -> Option<u32> {
        let a_ptr = arg1;
        if a_ptr % 4 != 0 {
            panic!();
        }
        let b_ptr = arg2;
        if b_ptr % 4 != 0 {
            panic!();
        }

        let num_fp12_words = <U384Field as NumWords>::WordsFieldElement::USIZE / LIMBS_PER_WORD;

        let a = rt.slice_unsafe(a_ptr, num_fp12_words);
        let (b_memory_records, b) = rt.mr_slice(b_ptr, num_fp12_words);
        rt.clk += 1;

        let result =
            Fp12::from_words(&a.try_into().unwrap()) * Fp12::from_words(&b.try_into().unwrap());

        let a_memory_records = rt.mw_slice(a_ptr, &result.to_words());

        let lookup_id = rt.syscall_lookup_id;
        let shard = rt.current_shard();
        let channel = rt.current_channel();
        let clk = rt.clk;

        rt.record_mut().fp12_mul_events.push(Fp12MulEvent {
            lookup_id,
            shard,
            channel,
            clk,
            a_ptr,
            a,
            b_ptr,
            b,
            a_memory_records,
            b_memory_records,
        });

        None
    }
}

impl<F> BaseAir<F> for Fp12MulChip {
    fn width(&self) -> usize {
        NUM_COLS
    }
}

impl<AB> Air<AB> for Fp12MulChip
where
    AB: SP1AirBuilder,
    Limbs<AB::Var, <U384Field as NumLimbs>::Limbs>: Copy,
{
    fn eval(&self, builder: &mut AB) {
        let main = builder.main();
        let local = main.row_slice(0);
        let local: &Fp12MulCols<AB::Var> = (*local).borrow();
        let next = main.row_slice(1);
        let next: &Fp12MulCols<AB::Var> = (*next).borrow();
        let num_fp12_words = <U384Field as NumWords>::WordsFieldElement::USIZE / LIMBS_PER_WORD;

        // Constrain the incrementing nonce.
        builder.when_first_row().assert_zero(local.nonce);
        builder
            .when_transition()
            .assert_eq(local.nonce + AB::Expr::one(), next.nonce);

        let a: [Limbs<AB::Var, _>; 12] = local
            .a_access
            .iter()
            .chunks(FP12_WORDS)
            .map(|x| limbs_from_prev_access(x))
            .collect();
        let b: [Limbs<AB::Var, _>; 12] = local
            .b_access
            .iter()
            .chunks(FP12_WORDS)
            .map(|x| limbs_from_prev_access(x))
            .collect();

        let eval =
            Fp12MulChipEval::new(local.shard, local.channel, local.is_real, builder, MODULUS);

        eval.fp12_mul(&a, &b);

        for i in 0..FP12_WORDS * LIMBS_PER_WORD {
            builder
                .when(local.is_real)
                .assert_eq(local.output.x[i], local.a_access[i / 4].value()[i % 4]);
            builder.when(local.is_real).assert_eq(
                local.y.result[i],
                local.a_access[num_fp12_words + i / 4].value()[i % 4],
            );
        }

        builder.eval_memory_access_slice(
            local.shard,
            local.channel,
            local.clk.into(),
            local.b_ptr,
            &local.b_access,
            local.is_real,
        );

        builder.eval_memory_access_slice(
            local.shard,
            local.channel,
            local.clk + AB::F::from_canonical_u32(1),
            local.b_ptr,
            &local.b_access,
            local.is_real,
        );

        let syscall_id_felt = U384Field::from_canonical_u32(SyscallCode::Fp12Mul as u32);

        builder.receive_syscall(
            local.shard,
            local.channel,
            local.clk,
            local.nonce,
            syscall_id_felt,
            local.a_ptr,
            local.b_ptr,
            local.is_real,
        );
    }
}

macro_rules! build_fp12_mul_constraints {
    ($DType:ty) => {
        fn sum_of_products_aux(
            &mut self,
            dest: &mut SumOfProductsAuxillaryCols<F>,
            b: [&$DType; 6],
        ) -> [$DType; 4] {
            let b00 = b[0];
            let b01 = b[1];
            let b10 = b[2];
            let b11 = b[3];
            let b20 = b[4];
            let b21 = b[5];

            let b10_p_b11 = self.add(&mut dest.b10_p_b11, b10, b11);
            let b10_m_b11 = self.sub(&mut dest.b10_m_b11, b10, b11);
            let b20_p_b21 = self.add(&mut dest.b20_p_b21, b20, b21);
            let b20_m_b21 = self.sub(&mut dest.b20_m_b21, b20, b21);

            [b10_p_b11, b10_m_b11, b20_p_b21, b20_m_b21]
        }

        fn sum_of_products(
            &mut self,
            dest: &mut SumOfProductsCols<F>,
            a: [(i8, &$DType); 6],
            b: [(i8, &$DType); 6],
        ) -> $DType {
            let a00 = a[0].1;
            let a01 = a[1].1;
            let a10 = a[2].1;
            let a11 = a[3].1;
            let a20 = a[4].1;
            let a21 = a[5].1;

            let b00 = b[0].1;
            let b01 = b[1].1;
            let b10 = b[2].1;
            let b11 = b[3].1;
            let b20 = b[4].1;
            let b21 = b[5].1;

            let a1_t_b1 = &self.mul(&mut dest.a1_t_b1, a00, b00);
            let a2_t_b2 = &self.mul(&mut dest.a2_t_b2, a01, b01);
            let a3_t_b3 = &self.mul(&mut dest.a3_t_b3, a10, b10);
            let a4_t_b4 = &self.mul(&mut dest.a4_t_b4, a11, b11);
            let a5_t_b5 = &self.mul(&mut dest.a5_t_b5, a20, b20);
            let a6_t_b6 = &self.mul(&mut dest.a6_t_b6, a21, b21);

            let products = [a1_t_b1, a2_t_b2, a3_t_b3, a4_t_b4, a5_t_b5, a6_t_b6];
            let dests = [
                &mut dest.sum1,
                &mut dest.sum2,
                &mut dest.sum3,
                &mut dest.sum4,
                &mut dest.sum5,
            ];

            // Get negative coefficients in the sum of products.
            let is_sub = a
                .iter()
                .zip(b.iter())
                .map(|(a, b)| a.0 != b.0)
                .collect_vec();

            let mut sum = a1_t_b1.clone();

            for (is_neg, dest, cur) in izip!(is_sub, dests, products).skip(1) {
                let _sum = &sum.clone();
                if is_neg {
                    sum = sum + &self.sub(dest, _sum, cur);
                } else {
                    sum = sum + &self.add(dest, _sum, cur);
                }
            }

            sum
        }
        fn fp6_mul(
            &mut self,
            dest: &mut Fp6MulCols<F>,
            a: &[$DType; 6],
            b: &[$DType; 6],
        ) -> [$DType; 6] {
            let a00 = &a[0];
            let a01 = &a[1];
            let a10 = &a[2];
            let a11 = &a[3];
            let a20 = &a[4];
            let a21 = &a[5];

            let b00 = &b[0];
            let b01 = &b[1];
            let b10 = &b[2];
            let b11 = &b[3];
            let b20 = &b[4];
            let b21 = &b[5];

            let [b10_p_b11, b10_m_b11, b20_p_b21, b20_m_b21] =
                self.sum_of_products_aux(&mut dest.aux, [b00, b01, b10, b11, b20, b21]);
            let c00 = self.sum_of_products(
                &mut dest.c00,
                [(1, a00), (1, a01), (1, a10), (1, a11), (1, a20), (1, a21)],
                [
                    (1, b00),
                    (-1, b01),
                    (1, &b20_m_b21),
                    (-1, &b20_p_b21),
                    (1, &b10_m_b11),
                    (-1, &b10_p_b11),
                ],
            );

            let c01 = self.sum_of_products(
                &mut dest.c01,
                [(1, a00), (1, a01), (1, a10), (1, a11), (1, a20), (1, a21)],
                [
                    (1, b01),
                    (1, b00),
                    (1, &b20_p_b21),
                    (1, &b20_m_b21),
                    (1, &b10_p_b11),
                    (1, &b10_m_b11),
                ],
            );

            let c10 = self.sum_of_products(
                &mut dest.c10,
                [
                    (1, a00),
                    (-1, a01),
                    (1, a10),
                    (-1, a11),
                    (1, a20),
                    (-1, a21),
                ],
                [
                    (1, b10),
                    (1, b11),
                    (1, b00),
                    (1, b01),
                    (1, &b20_m_b21),
                    (1, &b20_p_b21),
                ],
            );

            let c11 = self.sum_of_products(
                &mut dest.c11,
                [(1, a00), (1, a01), (1, a10), (1, a11), (1, a20), (1, a21)],
                [
                    (1, b11),
                    (1, b10),
                    (1, b01),
                    (1, b00),
                    (1, &b20_p_b21),
                    (1, &b20_m_b21),
                ],
            );

            let c20 = self.sum_of_products(
                &mut dest.c20,
                [
                    (1, a00),
                    (-1, a01),
                    (1, a10),
                    (-1, a11),
                    (1, a20),
                    (-1, a21),
                ],
                [(1, b20), (1, b21), (1, b10), (1, b11), (1, b00), (1, b01)],
            );

            let c21 = self.sum_of_products(
                &mut dest.c21,
                [(1, a00), (1, a01), (1, a10), (1, a11), (1, a20), (1, a21)],
                [(1, b21), (1, b20), (1, b11), (1, b10), (1, b01), (1, b00)],
            );

            [c00, c01, c10, c11, c20, c21]
        }
        fn fp6_add(
            &mut self,
            dest: &mut Fp6AddCols<F>,
            a: &[$DType; 6],
            b: &[$DType; 6],
        ) -> [$DType; 6] {
            let a00 = &a[0];
            let a01 = &a[1];
            let a10 = &a[2];
            let a11 = &a[3];
            let a20 = &a[4];
            let a21 = &a[5];

            let b00 = &b[0];
            let b01 = &b[1];
            let b10 = &b[2];
            let b11 = &b[3];
            let b20 = &b[4];
            let b21 = &b[5];

            let a00_p_b00 = self.add(&mut dest.a00_p_b00, a00, b00);
            let a01_p_b01 = self.add(&mut dest.a01_p_b01, a01, b01);
            let a10_p_b10 = self.add(&mut dest.a10_p_b10, a10, b10);
            let a11_p_b11 = self.add(&mut dest.a11_p_b11, a11, b11);
            let a20_p_b20 = self.add(&mut dest.a20_p_b20, a20, b20);
            let a21_p_b21 = self.add(&mut dest.a21_p_b21, a21, b21);

            [
                a00_p_b00, a01_p_b01, a10_p_b10, a11_p_b11, a20_p_b20, a21_p_b21,
            ]
        }

        fn fp6_sub(
            &mut self,
            dest: &mut Fp6AddCols<F>,
            a: &[$DType; 6],
            b: &[$DType; 6],
        ) -> [$DType; 6] {
            let a00 = &a[0];
            let a01 = &a[1];
            let a10 = &a[2];
            let a11 = &a[3];
            let a20 = &a[4];
            let a21 = &a[5];

            let b00 = &b[0];
            let b01 = &b[1];
            let b10 = &b[2];
            let b11 = &b[3];
            let b20 = &b[4];
            let b21 = &b[5];

            let a00_m_b00 = self.sub(&mut dest.a00_p_b00, a00, b00);
            let a01_m_b01 = self.sub(&mut dest.a01_p_b01, a01, b01);
            let a10_m_b10 = self.sub(&mut dest.a10_p_b10, a10, b10);
            let a11_m_b11 = self.sub(&mut dest.a11_p_b11, a11, b11);
            let a20_m_b20 = self.sub(&mut dest.a20_p_b20, a20, b20);
            let a21_m_b21 = self.sub(&mut dest.a21_p_b21, a21, b21);

            [
                a00_m_b00, a01_m_b01, a10_m_b10, a11_m_b11, a20_m_b20, a21_m_b21,
            ]
        }
        fn fp6_mul_by_non_residue(
            &mut self,
            dest: &mut Fp6MulByNonResidueCols<F>,
            a: &[$DType; 6],
        ) -> [$DType; 6] {
            let a00 = &a[0];
            let a01 = &a[1];
            let a10 = &a[2];
            let a11 = &a[3];
            let a20 = &a[4];
            let a21 = &a[5];

            let c00 = self.sub(&mut dest.c00, &a20, &a21);
            let c01 = self.add(&mut dest.c01, &a20, &a21);

            let c10 = a00;
            let c11 = a01;

            let c20 = a10;
            let c21 = a11;

            [c00, c01, c10.clone(), c11.clone(), c20.clone(), c21.clone()]
        }
        fn fp12_mul(
            &mut self,
            dest: &mut AuxFp12MulCols<F>,
            a: [$DType; 12],
            b: [$DType; 12],
        ) -> [$DType; 12] {
            let ac0 = a[0..6]
                .iter()
                .map(|x| x.clone())
                .collect_vec()
                .try_into()
                .unwrap();
            let ac1 = a[6..12]
                .iter()
                .map(|x| x.clone())
                .collect_vec()
                .try_into()
                .unwrap();
            let bc0 = b[0..6]
                .iter()
                .map(|x| x.clone())
                .collect_vec()
                .try_into()
                .unwrap();
            let bc1 = b[6..12]
                .iter()
                .map(|x| x.clone())
                .collect_vec()
                .try_into()
                .unwrap();

            let aa = self.fp6_mul(&mut dest.aa, &ac0, &bc0);
            let bb = self.fp6_mul(&mut dest.bb, &ac1, &bc1);

            let o = self.fp6_add(&mut dest.o, &bc0, &bc0);
            let y1 = self.fp6_add(&mut dest.y1, &ac1, &ac0);
            let y2 = self.fp6_mul(&mut dest.y2, &y1, &o);
            let y3 = self.fp6_sub(&mut dest.y3, &y2, &aa);
            let y = self.fp6_sub(&mut dest.y, &y3, &bb);
            let x1 = self.fp6_mul_by_non_residue(&mut dest.x1, &bb);
            let x = self.fp6_add(&mut dest.x, &x1, &aa);

            x.iter()
                .chain(y.iter())
                .cloned()
                .collect_vec()
                .try_into()
                .unwrap()
        }
    };
}
#[repr(C)]
struct Fp12MulChipTrace<F> {
    shard: u32,
    channel: u32,
    new_byte_lookup_events: Vec<ByteLookupEvent>,
    modulus: BigUint,
    _marker: PhantomData<F>,
}

impl<F: PrimeField32> Fp12MulChipTrace<F> {
    fn new(
        shard: u32,
        channel: u32,
        new_byte_lookup_events: Vec<ByteLookupEvent>,
        modulus: BigUint,
    ) -> Self {
        Self {
            shard,
            channel,
            new_byte_lookup_events,
            modulus,
            _marker: PhantomData,
        }
    }

    fn populate_with_modulus(
        &mut self,
        dest: &mut FieldOpCols<F, U384Field>,
        a: &BigUint,
        b: &BigUint,
        op: FieldOperation,
    ) {
        dest.populate_with_modulus(
            &mut self.new_byte_lookup_events,
            self.shard,
            self.channel,
            a,
            b,
            &self.modulus,
            op,
        );
    }
    fn mul(&mut self, dest: &mut FieldOpCols<F, U384Field>, a: &BigUint, b: &BigUint) -> BigUint {
        self.populate_with_modulus(dest, a, b, FieldOperation::Mul);
        (a * b) % &self.modulus
    }
    fn add(&mut self, dest: &mut FieldOpCols<F, U384Field>, a: &BigUint, b: &BigUint) -> BigUint {
        self.populate_with_modulus(dest, a, b, FieldOperation::Add);
        (a + b) % &self.modulus
    }
    fn sub(&mut self, dest: &mut FieldOpCols<F, U384Field>, a: &BigUint, b: &BigUint) -> BigUint {
        self.populate_with_modulus(dest, a, b, FieldOperation::Sub);
        (a - b) % &self.modulus
    }

    build_fp12_mul_constraints!(BigUint);
}

#[derive(Clone)]
#[repr(C)]
struct Fp12MulChipEval<F, AB>
where
    AB: SP1AirBuilder,
    Limbs<AB::Var, <U384Field as NumLimbs>::Limbs>: Copy,
{
    shard: u32,
    channel: u32,
    is_real: u32,
    builder: AB,
    modulus: BigUint,
    _marker: PhantomData<F>,
}

impl<F, AB> Fp12MulChipEval<F, AB>
where
    AB: SP1AirBuilder,
    Limbs<AB::Var, <U384Field as NumLimbs>::Limbs>: Copy,
{
    fn new(shard: u32, channel: u32, is_real: u32, builder: AB, modulus: BigUint) -> Self {
        Self {
            shard,
            channel,
            is_real,
            builder,
            modulus,
            _marker: PhantomData,
        }
    }

    fn eval(&mut self, dest: &mut FieldOpCols<F, U384Field>, x: &AB, y: &AB, op: FieldOperation) {
        dest.eval(
            &mut self.builder,
            &x,
            &y,
            op,
            self.shard,
            self.channel,
            self.is_real,
        );
    }
    fn mul(&mut self, dest: &mut FieldOpCols<F, _>, a: &AB, b: &AB) -> Limbs<F, _> {
        self.eval(dest, a, b, FieldOperation::Mul);
        dest.result
        // (a * b) % &self.modulus
    }
    fn add(&mut self, dest: &mut FieldOpCols<F, _>, a: &AB, b: &AB) -> AB {
        self.eval(dest, a, b, FieldOperation::Mul);
        (a + b) % &self.modulus
    }
    fn sub(&mut self, dest: &mut FieldOpCols<F, _>, a: &AB, b: &AB) -> AB {
        self.eval(dest, a, b, FieldOperation::Mul);
        (a - b) % &self.modulus
    }

    build_fp12_mul_constraints!(Limbs<AB::Var, _>);
}
