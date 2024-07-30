#!/bin/bash

# update and install deps
sudo apt-get update
sudo apt-get install -y --no-install-recommends ca-certificates clang curl libssl-dev pkg-config git dialog build-essential libtss2-dev

# install rust
curl --proto '=https' --tlsv1.2 --retry 10 --retry-connrefused -fsSL 'https://sh.rustup.rs' | sh -s -- -y
source "$HOME/.cargo/env"
export CARGO_TERM_COLOR=always
cargo --version

# install cargo-prove
curl -L https://sp1.succinct.xyz | bash && ~/.sp1/bin/sp1up
cargo prove --version

# build and execute the target sp1 program
cd ../examples/fibonacci/script/
cargo build --release
cd ../../target/release/
. fibonacci-script

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
REPORT_DATA_HASH=$(sha256sum ${REPORT_DATA} | awk '{print $1}')
echo REPORT_DATA_HASH=${REPORT_DATA_HASH}

./cvm/tdx_quote_generation/target/release/tdx_quote_generation --report_data ${REPORT_DATA_HASH}