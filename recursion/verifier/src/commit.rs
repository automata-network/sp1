use p3_commit::{LagrangeSelectors, PolynomialSpace};
use sp1_recursion_compiler::{
    ir::{Array, Builder, Config, Ext, Var},
    verifier::fri::RoundVariable,
};

pub trait PolynomialSpaceVariable<C: Config>: Sized {
    type Constant: PolynomialSpace<Val = C::F>;

    fn from_constant(builder: &mut Builder<C>, constant: Self::Constant) -> Self;

    fn next_point(&self, builder: &mut Builder<C>, point: Ext<C::F, C::EF>) -> Ext<C::F, C::EF>;

    fn selectors_at_point(
        &self,
        builder: &mut Builder<C>,
        point: Ext<C::F, C::EF>,
    ) -> LagrangeSelectors<Ext<C::F, C::EF>>;

    fn zp_at_point(&self, builder: &mut Builder<C>, point: Ext<C::F, C::EF>) -> Ext<C::F, C::EF>;

    fn split_domains(&self, builder: &mut Builder<C>, log_num_chunks: usize) -> Vec<Self>;
}

pub trait PcsVariable<C: Config, Challenger> {
    type Domain: PolynomialSpaceVariable<C>;

    type Commitment;

    type Proof;

    fn natural_domain_for_log_degree(
        &self,
        builder: &mut Builder<C>,
        log_degree: Var<C::N>,
    ) -> Self::Domain;

    fn verify(
        &self,
        builder: &mut Builder<C>,
        rounds: Array<C, RoundVariable<C>>,
        proof: Self::Proof,
        challenger: &mut Challenger,
    );
}
