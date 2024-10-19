use crate::transport::dfu_uuids::*;
use crate::transport::DfuTransport;

use async_trait::async_trait;
use btleplug::api::BDAddr;
use btleplug::api::{Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::Adapter;
use btleplug::platform::Peripheral;
use futures::stream::StreamExt;
use std::error::Error;

async fn find_characteristic_by_uuid(
    peripheral: &Peripheral,
    uuid: uuid::Uuid,
) -> Result<Characteristic, Box<dyn Error>> {
    for char in peripheral.characteristics() {
        if uuid == char.uuid {
            return Ok(char);
        }
    }
    Err("characteristic not found".into())
}

async fn find_peripheral_by_name(central: &Adapter, name: &str) -> Result<Peripheral, Box<dyn Error>> {
    println!("Searching for {} ...", name);
    central.start_scan(ScanFilter::default()).await?;
    let mut events = central.events().await?;
    while let Some(event) = events.next().await {
        if let CentralEvent::DeviceDiscovered(id) = event {
            let local_name = central.peripheral(&id).await?.properties().await?.unwrap().local_name;
            if let Some(n) = local_name {
                println!("Found [{}] at [{}]", n, id);
                if n == name {
                    central.stop_scan().await?;
                    return Ok(central.peripheral(&id).await?);
                }
            }
        }
    }
    Err("unexpected end of stream".into())
}

async fn find_peripheral(central: &Adapter, in_name: &str, in_addr: Option<BDAddr>) -> Result<Peripheral, Box<dyn Error>> {
    println!("Searching for {:?} and {:?}...", in_name, in_addr);
    central.start_scan(ScanFilter::default()).await?;
    let mut events = central.events().await?;

    // handle string movement and deal w/ String b/c Clap talks 'String'
    // let in_name: Option<&str> = in_name.as_deref();
        // Some(inn) => {
        //     let tmps = inn.clone();
        //     Some(inn.clone().as_str())
        // }
        // _ => None
    // };
    while let Some(event) = events.next().await {
        if let CentralEvent::DeviceDiscovered(id) = event {
            let props = central.peripheral(&id).await?.properties().await?.unwrap();
            let loop_addr = props.address;
            let loop_name = props.local_name;

            // let local_addr= unsafe { central.peripheral(&id).await?.properties().await? }   
            if let Some(ina) = in_addr {
                if ina == loop_addr {
                    println!("Found [{:?}] at [{}]", ina, id);
                    central.stop_scan().await?;
                    return Ok(central.peripheral(&id).await?);
                }
            }
            if let Some(loopn) = loop_name {
                if in_name == loopn {
                    println!("Found [{:?}] at [{}]", in_name, id);
                    central.stop_scan().await?;
                    return Ok(central.peripheral(&id).await?);
                }
            }
        }
    }
    Err("unexpected end of stream".into())
}

async fn timeout<F: std::future::Future>(future: F) -> Result<F::Output, tokio::time::error::Elapsed> {
    tokio::time::timeout(std::time::Duration::from_millis(500), future).await
}

pub struct DfuTransportBtleplug {
    peripheral: Peripheral,
    control_point: Characteristic,
    data_point: Characteristic,
}

#[async_trait]
impl DfuTransport for &DfuTransportBtleplug {
    async fn mtu(&self) -> usize {
        // TODO fix once btleplug supports MTU lookup
        244
    }
    async fn write_data(&self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        self.write(&self.data_point, bytes, WriteType::WithoutResponse).await
    }
    async fn request_ctrl(&self, bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        self.request(&self.control_point, bytes, WriteType::WithResponse).await
    }
}

impl DfuTransportBtleplug {
    async fn write(&self, chr: &Characteristic, bytes: &[u8], write_type: WriteType) -> Result<(), Box<dyn Error>> {
        let res = timeout(self.peripheral.write(chr, bytes, write_type)).await?;
        Ok(res?)
    }
    async fn request(
        &self,
        chr: &Characteristic,
        bytes: &[u8],
        write_type: WriteType,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut notifications = self.peripheral.notifications().await.unwrap();
        timeout(self.peripheral.write(chr, bytes, write_type)).await??;
        loop {
            let ntf = timeout(notifications.next()).await?.unwrap();
            if ntf.uuid == chr.uuid {
                return Ok(ntf.value);
            }
        }
    }
    pub async fn new(name: String, addr: Option<BDAddr>) -> Result<Self, Box<dyn Error>> {
        let manager = btleplug::platform::Manager::new().await?;
        let adapters = manager.adapters().await?;
        let central = adapters.into_iter().next().unwrap();

        let mut peripheral: Peripheral = find_peripheral(&central, name.as_str(), addr).await?;
        peripheral.connect().await?;
        peripheral.discover_services().await?;

        // TODO find a better place for buttonless DFU
        if let Ok(buttonless) = find_characteristic_by_uuid(&peripheral, BTTNLSS).await {
            peripheral.subscribe(&buttonless).await?;
            let mut notifications = peripheral.notifications().await.unwrap();
            peripheral.write(&buttonless, &[0x01], WriteType::WithResponse).await?;
            let res = timeout(notifications.next()).await?.unwrap();
            assert_eq!(res.value, [0x20, 0x01, 0x01]);

            peripheral.connect().await?;
            peripheral.discover_services().await?;
        }

        let control_point = find_characteristic_by_uuid(&peripheral, CTRL_PT).await?;
        let data_point = find_characteristic_by_uuid(&peripheral, DATA_PT).await?;
        peripheral.subscribe(&control_point).await?;
        Ok(DfuTransportBtleplug {
            peripheral,
            control_point,
            data_point,
        })
    }
}
