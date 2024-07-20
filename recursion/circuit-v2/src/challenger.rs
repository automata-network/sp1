use p3_field::AbstractField;
use sp1_recursion_compiler::circuit::CircuitV2Builder;
use sp1_recursion_compiler::prelude::MemIndex;
use sp1_recursion_compiler::prelude::MemVariable;
use sp1_recursion_compiler::prelude::Ptr;
use sp1_recursion_compiler::prelude::Variable;
use sp1_recursion_compiler::prelude::{Array, Builder, Config, DslVariable, Ext, Felt, Usize, Var};
use sp1_recursion_core_v2::runtime::{DIGEST_SIZE, HASH_RATE, PERMUTATION_WIDTH};

use crate::fri::types::DigestVariable;
use crate::types::VerifyingKeyVariable;

/// Reference: [p3_challenger::CanObserve].
pub trait CanObserveVariable<C: Config, V> {
    fn observe(&mut self, builder: &mut Builder<C>, value: V);

    fn observe_slice(&mut self, builder: &mut Builder<C>, values: impl IntoIterator<Item = V>);
}

pub trait CanSampleVariable<C: Config, V> {
    fn sample(&mut self, builder: &mut Builder<C>) -> V;
}

/// Reference: [p3_challenger::FieldChallenger].
pub trait FeltChallenger<C: Config>:
    CanObserveVariable<C, Felt<C::F>> + CanSampleVariable<C, Felt<C::F>> + CanSampleBitsVariable<C>
{
    fn sample_ext(&mut self, builder: &mut Builder<C>) -> Ext<C::F, C::EF>;
}

pub trait CanSampleBitsVariable<C: Config> {
    fn sample_bits(&mut self, builder: &mut Builder<C>, nb_bits: usize) -> Vec<Felt<C::F>>;
}

/// Reference: [p3_challenger::DuplexChallenger]
#[derive(Clone)]
pub struct DuplexChallengerVariable<C: Config> {
    pub sponge_state: [Felt<C::F>; PERMUTATION_WIDTH],
    // pub nb_inputs: usize,
    pub input_buffer: Vec<Felt<C::F>>,
    // pub nb_outputs: usize,
    pub output_buffer: Vec<Felt<C::F>>,
}

impl<C: Config> DuplexChallengerVariable<C> {
    /// Creates a new duplex challenger with the default state.
    pub fn new(builder: &mut Builder<C>) -> Self {
        DuplexChallengerVariable::<C> {
            sponge_state: core::array::from_fn(|_| builder.eval(C::F::zero())),
            // nb_inputs: 0,
            input_buffer: vec![],
            // nb_outputs: 0,
            output_buffer: vec![],
        }
    }

    /// Creates a new challenger with the same state as an existing challenger.
    pub fn copy(&self, builder: &mut Builder<C>) -> Self {
        let DuplexChallengerVariable {
            ref sponge_state,
            // nb_inputs,
            ref input_buffer,
            // nb_outputs,
            ref output_buffer,
        } = *self;
        let sponge_state = sponge_state.map(|x| builder.eval(x));
        let mut copy_vec = |v: &Vec<Felt<C::F>>| v.iter().map(|x| builder.eval(*x)).collect();
        DuplexChallengerVariable::<C> {
            sponge_state,
            // nb_inputs,
            input_buffer: copy_vec(input_buffer),
            // nb_outputs,
            output_buffer: copy_vec(output_buffer),
        }
    }

    // /// Asserts that the state of this challenger is equal to the state of another challenger.
    // pub fn assert_eq(&self, builder: &mut Builder<C>, other: &Self) {
    //     builder.assert_var_eq(self.nb_inputs, other.nb_inputs);
    //     builder.assert_var_eq(self.nb_outputs, other.nb_outputs);
    //     for i in 0..PERMUTATION_WIDTH {
    //         let element = self.sponge_state[i];
    //         let other_element = other.sponge_state[i];
    //         builder.assert_felt_eq(element, other_element);
    //     }
    //     builder.range(0, self.nb_inputs).for_each(|i, builder| {
    //         let element = self.input_buffer[i];
    //         let other_element = other.input_buffer[i];
    //         builder.assert_felt_eq(element, other_element);
    //     });
    //     builder.range(0, self.nb_outputs).for_each(|i, builder| {
    //         let element = self.output_buffer[i];
    //         let other_element = other.output_buffer[i];
    //         builder.assert_felt_eq(element, other_element);
    //     });
    // }

    // pub fn reset(&mut self, builder: &mut Builder<C>) {
    //     let zero: Var<_> = builder.eval(C::N::zero());
    //     let zero_felt: Felt<_> = builder.eval(C::F::zero());
    //     for i in 0..PERMUTATION_WIDTH {
    //         builder.set(&mut self.sponge_state, i, zero_felt);
    //     }
    //     builder.assign(self.nb_inputs, zero);
    //     for i in 0..PERMUTATION_WIDTH {
    //         builder.set(&mut self.input_buffer, i, zero_felt);
    //     }
    //     builder.assign(self.nb_outputs, zero);
    //     for i in 0..PERMUTATION_WIDTH {
    //         builder.set(&mut self.output_buffer, i, zero_felt);
    //     }
    // }

    pub fn duplexing(&mut self, builder: &mut Builder<C>) {
        self.sponge_state[0..self.input_buffer.len()].copy_from_slice(self.input_buffer.as_slice());
        self.input_buffer.clear();

        self.sponge_state = builder.poseidon2_permute_v2(self.sponge_state);

        self.output_buffer[0..PERMUTATION_WIDTH].copy_from_slice(&self.sponge_state);
    }

    fn observe(&mut self, builder: &mut Builder<C>, value: Felt<C::F>) {
        self.output_buffer.clear();

        self.input_buffer.push(value);

        if self.input_buffer.len() == HASH_RATE {
            self.duplexing(builder);
        }
    }

    pub fn observe_commitment(&mut self, builder: &mut Builder<C>, commitment: DigestVariable<C>) {
        for element in commitment.into_iter().take(DIGEST_SIZE) {
            self.observe(builder, element);
        }
    }

    fn sample(&mut self, builder: &mut Builder<C>) -> Felt<C::F> {
        if !self.input_buffer.is_empty() || self.output_buffer.is_empty() {
            self.clone().duplexing(builder);
        }

        self.output_buffer
            .pop()
            .expect("output buffer should be non-empty")
    }

    fn sample_ext(&mut self, builder: &mut Builder<C>) -> Ext<C::F, C::EF> {
        let a = self.sample(builder);
        let b = self.sample(builder);
        let c = self.sample(builder);
        let d = self.sample(builder);
        builder.ext_from_base_slice(&[a, b, c, d])
    }

    fn sample_bits(&mut self, builder: &mut Builder<C>, nb_bits: usize) -> Vec<Felt<C::F>> {
        let rand_f = self.sample(builder);
        let mut rand_f_bits = builder.num2bits_v2_f(rand_f);
        rand_f_bits.truncate(nb_bits);
        rand_f_bits
    }

    pub fn check_witness(&mut self, builder: &mut Builder<C>, nb_bits: usize, witness: Felt<C::F>) {
        self.observe(builder, witness);
        let element_bits = self.sample_bits(builder, nb_bits);
        for bit in element_bits {
            builder.assert_felt_eq(bit, C::F::zero());
        }
    }
}

impl<C: Config> CanObserveVariable<C, Felt<C::F>> for DuplexChallengerVariable<C> {
    fn observe(&mut self, builder: &mut Builder<C>, value: Felt<C::F>) {
        DuplexChallengerVariable::observe(self, builder, value);
    }

    fn observe_slice(
        &mut self,
        builder: &mut Builder<C>,
        values: impl IntoIterator<Item = Felt<C::F>>,
    ) {
        for value in values {
            self.observe(builder, value);
        }
    }
}

impl<C: Config> CanSampleVariable<C, Felt<C::F>> for DuplexChallengerVariable<C> {
    fn sample(&mut self, builder: &mut Builder<C>) -> Felt<C::F> {
        DuplexChallengerVariable::sample(self, builder)
    }
}

impl<C: Config> CanSampleBitsVariable<C> for DuplexChallengerVariable<C> {
    fn sample_bits(&mut self, builder: &mut Builder<C>, nb_bits: usize) -> Vec<Felt<C::F>> {
        DuplexChallengerVariable::sample_bits(self, builder, nb_bits)
    }
}

impl<C: Config> CanObserveVariable<C, DigestVariable<C>> for DuplexChallengerVariable<C> {
    fn observe(&mut self, builder: &mut Builder<C>, commitment: DigestVariable<C>) {
        DuplexChallengerVariable::observe_commitment(self, builder, commitment);
    }

    fn observe_slice(
        &mut self,
        _builder: &mut Builder<C>,
        _values: impl IntoIterator<Item = DigestVariable<C>>,
    ) {
        todo!()
    }
}

impl<C: Config> CanObserveVariable<C, VerifyingKeyVariable<C>> for DuplexChallengerVariable<C> {
    fn observe(&mut self, builder: &mut Builder<C>, value: VerifyingKeyVariable<C>) {
        self.observe_commitment(builder, value.commitment);
        self.observe(builder, value.pc_start)
    }

    fn observe_slice(
        &mut self,
        _builder: &mut Builder<C>,
        _values: impl IntoIterator<Item = VerifyingKeyVariable<C>>,
    ) {
        todo!()
    }
}

impl<C: Config> FeltChallenger<C> for DuplexChallengerVariable<C> {
    fn sample_ext(&mut self, builder: &mut Builder<C>) -> Ext<C::F, C::EF> {
        DuplexChallengerVariable::sample_ext(self, builder)
    }
}

#[cfg(test)]
mod tests {
    use p3_challenger::CanObserve;
    use p3_challenger::CanSample;
    use p3_field::AbstractField;
    use sp1_core::stark::StarkGenericConfig;
    use sp1_core::utils::BabyBearPoseidon2;
    use sp1_recursion_compiler::asm::AsmBuilder;
    use sp1_recursion_compiler::asm::AsmConfig;
    use sp1_recursion_compiler::ir::Felt;
    use sp1_recursion_compiler::ir::Usize;
    use sp1_recursion_compiler::ir::Var;

    use sp1_recursion_core::runtime::PERMUTATION_WIDTH;
    use sp1_recursion_core::stark::utils::run_test_recursion;
    use sp1_recursion_core::stark::utils::TestConfig;

    use crate::challenger::DuplexChallengerVariable;

    #[test]
    fn test_compiler_challenger() {
        type SC = BabyBearPoseidon2;
        type F = <SC as StarkGenericConfig>::Val;
        type EF = <SC as StarkGenericConfig>::Challenge;

        let config = SC::default();
        let mut challenger = config.challenger();
        challenger.observe(F::one());
        challenger.observe(F::two());
        challenger.observe(F::two());
        challenger.observe(F::two());
        let result: F = challenger.sample();
        println!("expected result: {}", result);

        let mut builder = AsmBuilder::<F, EF>::default();

        let width: Var<_> = builder.eval(F::from_canonical_usize(PERMUTATION_WIDTH));
        let mut challenger = DuplexChallengerVariable::<AsmConfig<F, EF>> {
            sponge_state: core::array::from_fn(|_| builder.uninit()),
            input_buffer: vec![],
            output_buffer: vec![],
        };
        let one: Felt<_> = builder.eval(F::one());
        let two: Felt<_> = builder.eval(F::two());
        builder.halt();
        challenger.observe(&mut builder, one);
        challenger.observe(&mut builder, two);
        challenger.observe(&mut builder, two);
        challenger.observe(&mut builder, two);
        let element = challenger.sample(&mut builder);

        let expected_result: Felt<_> = builder.eval(result);
        builder.assert_felt_eq(expected_result, element);

        let program = builder.compile_program();
        run_test_recursion(program, None, TestConfig::All);
    }
}