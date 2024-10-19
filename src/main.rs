mod package;
mod protocol;
mod transport;
mod transport_btleplug;

use btleplug::api::BDAddr;
use clap::Parser;

/// Update firmware on nRF BLE DFU targets
#[derive(clap::Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// BLE DFU target name
    #[arg(short, long, default_value = "")]
    name: String,
    
    /// BLE Address
    #[arg(short, long, default_value = "")]
    addr: String,

    /// Firmware update package path
    #[arg(short, long, default_value = "")]
    pkg: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let (init_pkt, fw_pkt) = package::extract(&args.pkg)?;
    
    let addr: Result<BDAddr, btleplug::api::ParseBDAddrError> = 
        BDAddr::from_str_delim(&args.addr);
    
    // let transport = &transport_btleplug::DfuTransportBtleplug::new(args.name, None).await?;
    let transport = &transport_btleplug::DfuTransportBtleplug::new(args.name, addr).await?;

    protocol::dfu_run(&transport, &init_pkt, &fw_pkt).await
}
