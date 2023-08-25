use std::error::Error;

pub trait DfuTransport {
    const MTU: usize;
    fn mtu(&self) -> usize {
        Self::MTU
    }
    fn write_ctrl(&self, bytes: &[u8]) -> Result<(), Box<dyn Error>>;
    fn write_data(&self, bytes: &[u8]) -> Result<(), Box<dyn Error>>;
    fn listen_ctrl(&self) -> Result<Vec<u8>, Box<dyn Error>>;
}

pub mod btleplug {
    use btleplug::api::{
        Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
    };
    use btleplug::platform::Peripheral;
    use futures::stream::StreamExt;
    use std::error::Error;

    use crate::transport::DfuTransport;

    const DFU_CONTROL_POINT: uuid::Uuid =
        uuid::Uuid::from_u128(0x8EC90001_F315_4F60_9FB8_838830DAEA50);
    const DFU_PACKET: uuid::Uuid = uuid::Uuid::from_u128(0x8EC90002_F315_4F60_9FB8_838830DAEA50);
    const BUTTONLESS_DFU_WITHOUT_BONDS: uuid::Uuid =
        uuid::Uuid::from_u128(0x8EC90003_F315_4F60_9FB8_838830DAEA50);

    async fn find_characteristic_by_uuid(
        peripheral: &Peripheral,
        uuid: uuid::Uuid,
    ) -> Result<Characteristic, Box<dyn Error>> {
        peripheral
            .characteristics()
            .iter()
            .find(|s| s.uuid == uuid)
            .ok_or("characteristic not found".into())
            .cloned()
    }

    async fn find_peripheral_by_name(name: &str) -> Result<Peripheral, Box<dyn Error>> {
        println!("Searching for {} ...", name);
        let manager = btleplug::platform::Manager::new().await?;
        let adapters = manager.adapters().await?;
        let central = adapters.into_iter().next().unwrap();

        let mut events = central.events().await?;

        central.start_scan(ScanFilter::default()).await?;
        while let Some(event) = events.next().await {
            if let CentralEvent::DeviceDiscovered(id) = event {
                let local_name = central
                    .peripheral(&id)
                    .await?
                    .properties()
                    .await?
                    .unwrap()
                    .local_name;
                if let Some(n) = local_name {
                    println!("Found [{}] at [{}]", n, id);
                    if n == name {
                        return Ok(central.peripheral(&id).await?);
                    }
                }
            }
        }
        unreachable!()
    }

    async fn timeout<F: std::future::Future>(
        future: F,
    ) -> Result<F::Output, tokio::time::error::Elapsed> {
        tokio::time::timeout(std::time::Duration::from_millis(500), future).await
    }

    pub struct NrfDfuTransport {
        peripheral: Peripheral,
        control_point: Characteristic,
        data_point: Characteristic,
    }

    impl DfuTransport for &NrfDfuTransport {
        const MTU: usize = 244;
        fn write_ctrl(&self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
            futures::executor::block_on(self.write(
                &self.control_point,
                bytes,
                WriteType::WithResponse,
            ))
        }
        fn write_data(&self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
            futures::executor::block_on(self.write(
                &self.data_point,
                bytes,
                WriteType::WithoutResponse,
            ))
        }
        fn listen_ctrl(&self) -> Result<Vec<u8>, Box<dyn Error>> {
            futures::executor::block_on(self.listen(&self.control_point))
        }
    }

    impl NrfDfuTransport {
        async fn write(
            &self,
            chr: &Characteristic,
            bytes: &[u8],
            write_type: WriteType,
        ) -> Result<(), Box<dyn Error>> {
            let res = timeout(self.peripheral.write(chr, bytes, write_type)).await?;
            Ok(res?)
        }
        async fn listen(&self, chr: &Characteristic) -> Result<Vec<u8>, Box<dyn Error>> {
            let mut notifications = self.peripheral.notifications().await.unwrap();
            loop {
                let ntf = timeout(notifications.next()).await?.unwrap();
                if ntf.uuid == chr.uuid {
                    return Ok(ntf.value);
                }
            }
        }
        pub async fn new(name: &str) -> Result<Self, Box<dyn Error>> {
            let mut peripheral = find_peripheral_by_name(name).await?;
            println!("{:?}", peripheral.properties().await?);
            peripheral.connect().await?;
            peripheral.discover_services().await?;

            // TODO find a better place for buttonless DFU
            if let Ok(buttonless) =
                find_characteristic_by_uuid(&peripheral, BUTTONLESS_DFU_WITHOUT_BONDS).await
            {
                peripheral.subscribe(&buttonless).await?;
                let mut notifications = peripheral.notifications().await.unwrap();
                peripheral
                    .write(&buttonless, &[0x01], WriteType::WithoutResponse)
                    .await?;
                let res = timeout(notifications.next()).await?.unwrap();
                assert_eq!(res.value, [0x20, 0x01, 0x01]);

                peripheral = find_peripheral_by_name("DfuTarg").await?;
                println!("{:?}", peripheral.properties().await?);
                peripheral.connect().await?;
                peripheral.discover_services().await?;
            }

            let control_point = find_characteristic_by_uuid(&peripheral, DFU_CONTROL_POINT).await?;
            let data_point = find_characteristic_by_uuid(&peripheral, DFU_PACKET).await?;
            peripheral.subscribe(&control_point).await?;
            Ok(NrfDfuTransport {
                peripheral,
                control_point,
                data_point,
            })
        }
    }
}
