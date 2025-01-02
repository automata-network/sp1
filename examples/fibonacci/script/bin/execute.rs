use serde::{Deserialize, Serialize};
use sp1_sdk::{include_elf, utils, ProverClient, SP1Stdin, HashableKey};

/// The ELF we want to execute inside the zkVM.
const ELF: &[u8] = include_elf!("fibonacci-program");

/// The mode used when generating the proof.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(i32)]
pub enum ProofMode {
    /// Unspecified or invalid proof mode.
    #[default] Unspecified = 0,
    /// The proof mode for an SP1 core proof.
    Core = 1,
    /// The proof mode for a compressed proof.
    Compressed = 2,
    /// The proof mode for a PlonK proof.
    Plonk = 3,
    /// The proof mode for a Groth16 proof.
    Groth16 = 4,
    /// The proof mode for a TEE proof
    TEE = 5,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RequestProofBody {
    pub elf: Vec<u8>,
    pub stdin: SP1Stdin,
    pub proof_mode: ProofMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TEEProof {
    pub signature: Vec<u8>,
    pub vk: Vec<u8>,
    pub public_values: Vec<u8>,
}

#[tokio::main]
async fn main() {
    // Setup logging.
    utils::setup_logger();

    // Create an input stream and write '500' to it.
    let n = 500u32;

    let mut stdin = SP1Stdin::new();
    stdin.write(&n);

    // Only execute the program and get a `SP1PublicValues` object.
    let client = ProverClient::from_env();
    let (mut public_values, execution_report) = client.execute(ELF, &stdin).run().unwrap();
    // println!("ELF: {:?}", ELF);
    // println!("stdin: {:?}", stdin);
    // println!("public_values: {:?}", public_values);
    // let (_, vk) = client.setup(ELF);
    // println!("vk: {:?}", vk.hash_bytes().to_vec());

    // Print the total number of cycles executed and the full execution report with a breakdown of
    // the RISC-V opcode and syscall counts.
    println!(
        "Executed program with {} cycles",
        execution_report.total_instruction_count() + execution_report.total_syscall_count()
    );
    println!("Full execution report:\n{:?}", execution_report);

    // Read and verify the output.
    let _ = public_values.read::<u32>();
    let a = public_values.read::<u32>();
    let b = public_values.read::<u32>();

    println!("a: {}", a);
    println!("b: {}", b);

    let request = RequestProofBody {
        elf: ELF.to_vec(),
        stdin: stdin,
        proof_mode: ProofMode::TEE,
    };
    let request_buf = serde_json::to_vec(&request).unwrap();

    let client = reqwest::Client::new();
    let response = client
        .post("http://127.0.0.1:3000/request_proof")
        .header("Content-Type", "application/json")
        .body(request_buf.to_owned())
        .send()
        .await
        .unwrap();

    println!("request_proof status Code: {}", response.status());

    let proof_id = response.text().await.unwrap();

    println!("Proof ID: {}", proof_id);

    let response = client
        .post("http://127.0.0.1:3000/wait_proof")
        .header("Content-Type", "text/plain")
        .body(proof_id.to_owned())
        .send()
        .await
        .unwrap();

    println!("wait_proof status Code: {}", response.status());

    let tee_proof = response.text().await.unwrap();
    let tee_proof: TEEProof = serde_json::from_slice(&tee_proof.as_bytes()).unwrap();

    println!("TEE Proof: {:?}", tee_proof);
}
