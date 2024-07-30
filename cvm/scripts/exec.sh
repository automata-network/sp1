#!/bin/bash

# build the tdx_quote_generation program
cd ../../../cvm/tdx_quote_generation/
git submodule update --init --recursive
cd pom-sdk/
git submodule update --init --recursive
cd ../
cargo build --release
cd ../../

CARGO_PROVE_HASH=$(sha256sum ~/.sp1/bin/cargo-prove | awk '{print $1}')
echo CARGO_PROVE_HASH=${CARGO_PROVE_HASH}

PROOF_HASH=$(sha256sum ./examples/target/release/proof-with-pis.bin | awk '{print $1}')
echo PROOF_HASH=${PROOF_HASH}

cd cvm/tdx_quote_generation/pom-sdk/
POM_SDK_COMMIT=$(git log --pretty=tformat:"%H" -n1)
echo POM_SDK_COMMIT=${POM_SDK_COMMIT}
cd ../../../

REPORT_DATA="${CARGO_PROVE_HASH}||${PROOF_HASH}||${POM_SDK_COMMIT}"
REPORT_DATA_HASH=$(echo -n "$REPORT_DATA" | sha512sum | awk '{print $1}')
echo REPORT_DATA_HASH=${REPORT_DATA_HASH}

sudo ./cvm/tdx_quote_generation/target/release/tdx_quote_generation --report_data ${REPORT_DATA_HASH}