mod package;
mod protocol;
mod transport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let name = args.next().ok_or("missing DFU target name")?;
    let path = args.next().ok_or("missing DFU package path")?;
    let (init_pkt, fw_pkt) = package::extract(&path)?;
    let transport = &transport::btleplug::NrfDfuTransport::new(&name).await?;

    protocol::dfu_run(&transport, &init_pkt, &fw_pkt).await
}
