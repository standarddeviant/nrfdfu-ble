mod package;
mod protocol;
mod transport;

use clap::Parser;

/// Update firmware on nRF BLE DFU targets
#[derive(clap::Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// BLE DFU target name
    name: String,

    /// Firmware update package path
    pkg: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let (init_pkt, fw_pkt) = package::extract(&args.pkg)?;
    let transport = &transport::btleplug::NrfDfuTransport::new(&args.name).await?;

    protocol::dfu_run(&transport, &init_pkt, &fw_pkt).await
}
