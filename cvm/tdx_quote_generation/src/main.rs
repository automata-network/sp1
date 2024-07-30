use structopt::StructOpt;
use tdx::{device::DeviceOptions, Tdx};

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(
        long = "report_data",
        default_value = "",
        help = "The report data that needs to be included into the tdx quote"
    )]
    report_data: String,
}

fn main() {
    let opt = Opt::from_args();
    let tdx = Tdx::new();
    let report_data = hex::decode(opt.report_data.trim_start_matches("0x")).expect("Failed to decode hex string");
    let mut report_data_array = [0u8; 64];
    if report_data.len() < 64 {
        report_data_array[..report_data.len()].copy_from_slice(&report_data);
    } else {
        report_data_array.copy_from_slice(&report_data[..64]);
    };
    println!("report_data: {:?}", report_data);

    match tdx.get_attestation_report_raw_with_options(
        DeviceOptions {
            report_data: Some(report_data_array),
        }
    ) {
        Ok(raw_quote) => {
            println!("tdx raw quote: {:?}", hex::encode(raw_quote));
        }
        Err(err) => {
            println!("[ERROR] get_attestation_report_raw_with_options meets error: {:?}", err);
        }
    }
}
