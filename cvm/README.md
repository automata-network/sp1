# SP1 in TEE

As people place increasing importance on computational security and with the widespread adoption of Intel TDX / AMD SEV technologies, more and more Web3 projects can now operate within a Trusted Execution Environment (TEE). Below is a guidebook on how Automata runs SP1's zkvm in a TEE using Intel TDX.

## Getting Started

### Hardware Requirements
The following cloud service providers (CSP) have support for Intel TDX:

### GCP
- Instance Type: c3-standard-* family
- Operating System: containerOS, RHEL0, SLES-15-sp5, Ubuntu 22.04
- Supported Zones: asia-southeast-1-{a,b,c}, europe-west4-{a,b}, us-central1-{a,b,c} 
- For more information on supported operating systems, please check out the following article on GCP: [supported configurations](https://cloud.google.com/confidential-computing/confidential-vm/docs/supported-configurations#intel-tdx)
- Currently, TDX enabled VMs can only be created via gcloud or Rest API, please check out this article on how to do so: [create an instance](https://cloud.google.com/confidential-computing/confidential-vm/docs/create-a-confidential-vm-instance#gcloud), and VM instances with Intel TDX in GCP enabled don't support [custom images](https://cloud.google.com/confidential-computing/confidential-vm/docs/create-custom-confidential-vm-images)

  - Example: `gcloud beta compute instances create tdx-instance --machine-type=c3-standard-4  --zone=us-central1-a  --confidential-compute-type=TDX  --maintenance-policy=TERMINATE  --image-family=ubuntu-2204-lts  --image-project=tdx-guest-images  --project={replace to your gcp project}` 
  - We don't suggest to create TDX VMs with containerOS, it will prevent users to install and execute anything because of the security consideration.

### Download Dependencies
```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends ca-certificates clang curl libssl-dev pkg-config git dialog build-essential libtss2-dev
```

### Install Rust
```bash
curl --proto '=https' --tlsv1.2 --retry 10 --retry-connrefused -fsSL 'https://sh.rustup.rs' | sh -s -- -y
source "$HOME/.cargo/env"
export CARGO_TERM_COLOR=always
cargo --version
```

### Install Cargo-prove
```bash
curl -L https://sp1.succinct.xyz | bash && ~/.sp1/bin/sp1up
source ~/.bashrc
cargo prove --version
```

### Build SP1 Program
Here we use fibonacci example for the demo.
```bash
cd examples/fibonacci/script/
cargo build --release
cd ../../target/release/
./fibonacci-script
```

### Intel TDX Quote Generation Program

#### Build the tdx_quote_generation program
We use [Automata Proof-of-Machinehood SDK](https://github.com/automata-network/pom-sdk.git) to generate the corresponding Intel TDX Quote, by providing the hash of the cargo-prove, the SP1 zkVM program proof and the POM-SDK commit ID.
```bash
cd cvm/tdx_quote_generation/
git submodule update --init --recursive
cd pom-sdk/
git submodule update --init --recursive
cd ../
cargo build --release
cd ../../
```

#### Execute the tdx_quote_generation program
```bash
chmod +x cvm/scripts/exec.sh
./cvm/scripts/exec.sh
```
Once you get the Intel TDX raw quote, you can send it to [Automata Intel DCAP Quote Verification Contract](https://testnet-explorer.ata.network/address/0xefE368b17D137E86298eec8EbC5502fb56d27832?tab=read_contract) to validate the result.

You can use [Automata DCAP Library](https://github.com/automata-network/dcap-rs.git) to generate the corresponding Intel TDX DCAP Quote with zkproof improvement, which has lower gas cost to verify the quote on-chain, and it is already integrated in the [Automata Proof-of-Machinehood SDK](https://github.com/automata-network/pom-sdk.git).

## Future Work

### Integrate other cloud providers that support Intel TDX
#### Azure
- Instance Type: DCesv5-series, DCedsv5-series, ECesv5-series, ECedsv5-series
- Operating System:  Ubuntu 24.04 Server (Confidential VM)- x64 Gen 2 image, Ubuntu 22.04 Server (Confidential VM) - x64 Gen 2 image.
- Supported Region: West Europe, Central US, East US 2, North Europe
#### AWS
- Coming soon
#### Self-hosted machines
- If you wish to use a CSP that is not listed above or run your own host, please ensure that the CSP or host is running the following specs:
  - Linux Kernel >= 6.7
  - Virtual Machine (VM) runs under KVM hypervisor 
  - VM has access to `/sys/kernel/config/tsm/report` and able to create a temporary directory with sudo (eg. `sudo mkdir /sys/kernel/config/tsm/report/tmp`)
    > If you receive the error `mkdir: cannot create directory ‘tmp’: No such device or address`, it means that ConfigFS is not supported on your VM.

### Integrate AMD SEV
Coming soon