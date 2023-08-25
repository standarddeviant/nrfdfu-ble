use std::io::prelude::*;

pub fn extract(path: &str) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let reader = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(reader)?;

    let manifest_raw = zip.by_name("manifest.json")?;
    let manifest: serde_json::Value = serde_json::from_reader(manifest_raw)?;
    let app = &manifest["manifest"]["application"];
    // TODO DFU package may contain other images: bootloader, softdevice
    // let bl = manifest["manifest"]["bootloader"];
    // let sd = manifest["manifest"]["softdevice"];
    let dat_name = app["dat_file"].as_str().unwrap();
    let bin_name = app["bin_file"].as_str().unwrap();

    let mut dat = Vec::new();
    zip.by_name(dat_name)?.read_to_end(&mut dat)?;

    let mut bin = Vec::new();
    zip.by_name(bin_name)?.read_to_end(&mut bin)?;

    Ok((dat, bin))
}
