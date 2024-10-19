use crate::transport::DfuTransport;
use indicatif::ProgressBar;

use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::error::Error;

// As defined in nRF5_SDK_17.1.0_ddde560/components/libraries/bootloader/dfu/nrf_dfu_req_handler.h

/// DFU Object variants
#[derive(Debug, Copy, Clone, IntoPrimitive)]
#[repr(u8)]
enum Object {
    Command = 0x01,
    Data = 0x02,
}

/// DFU Command opcodes
#[derive(Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
enum OpCode {
    ProtocolVersion = 0x00,
    ObjectCreate = 0x01,
    ReceiptNotifSet = 0x02,
    CrcGet = 0x03,
    ObjectExecute = 0x04,
    ObjectSelect = 0x06,
    MtuGet = 0x07,
    ObjectWrite = 0x08,
    Ping = 0x09,
    HardwareVersion = 0x0A,
    FirmwareVersion = 0x0B,
    Abort = 0x0C,
}

/// DFU Response codes
#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
enum ResponseCode {
    Invalid = 0x00,
    Success = 0x01,
    OpCodeNotSupported = 0x02,
    InvalidParameter = 0x03,
    InsufficientResources = 0x04,
    InvalidObject = 0x05,
    UnsupportedType = 0x07,
    OperationNotPermitted = 0x08,
    OperationFailed = 0x0A,
    ExtError = 0x0B,
}

fn crc32(buf: &[u8], init: u32) -> u32 {
    let mut h = crc32fast::Hasher::new_with_initial(init);
    h.update(buf);
    h.finalize()
}

// More requests are available when `NRF_DFU_PROTOCOL_REDUCED` is not defined
// in `nRF5_SDK_17.1.0_ddde560/components/libraries/bootloader/dfu/nrf_dfu_req_handler.c`
struct DfuTarget<'a, T: DfuTransport> {
    transport: &'a T,
}

impl<'a, T: DfuTransport> DfuTarget<'a, T> {
    fn verify_header(opcode: u8, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        if bytes.len() < 3 {
            return Err("invalid response length".into());
        }
        if bytes[0] != 0x60 {
            return Err("invalid response header".into());
        }
        if bytes[1] != opcode {
            return Err("invalid response opcode".into());
        }
        let result = ResponseCode::try_from(bytes[2])?;
        if result != ResponseCode::Success {
            return Err(format!("{:?}", result).into());
        }
        Ok(())
    }

    async fn write_data(&self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        self.transport.write_data(bytes).await
    }

    async fn request_ctrl(&self, bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        for _retry in 0..3 {
            match self.transport.request_ctrl(bytes).await {
                Err(e) => {
                    if e.is::<tokio::time::error::Elapsed>() {
                        // response timed out, retry
                        continue;
                    } else {
                        return Err(e);
                    }
                }
                Ok(r) => {
                    return Ok(r);
                }
            }
        }
        Err("No response after multiple tries".into())
    }

    async fn set_prn(&self, value: u32) -> Result<(), Box<dyn Error>> {
        let opcode: u8 = OpCode::ReceiptNotifSet.into();
        let mut payload: Vec<u8> = vec![opcode];
        payload.extend_from_slice(&value.to_le_bytes());
        let response = self.request_ctrl(&payload).await?;
        Self::verify_header(opcode, &response)?;
        Ok(())
    }

    async fn get_crc(&self) -> Result<(usize, u32), Box<dyn Error>> {
        let opcode: u8 = OpCode::CrcGet.into();
        let response = self.request_ctrl(&[opcode]).await?;
        Self::verify_header(opcode, &response)?;
        let offset = u32::from_le_bytes(response[3..7].try_into()?);
        let checksum = u32::from_le_bytes(response[7..11].try_into()?);
        Ok((offset as usize, checksum))
    }

    async fn select_object(&self, obj_type: Object) -> Result<(usize, usize, u32), Box<dyn Error>> {
        let opcode: u8 = OpCode::ObjectSelect.into();
        let arg: u8 = obj_type.into();
        let response = self.request_ctrl(&[opcode, arg]).await?;
        Self::verify_header(opcode, &response)?;
        let max_size = u32::from_le_bytes(response[3..7].try_into()?);
        let offset = u32::from_le_bytes(response[7..11].try_into()?);
        let checksum = u32::from_le_bytes(response[11..15].try_into()?);
        Ok((max_size as usize, offset as usize, checksum))
    }

    async fn create_object(&self, obj_type: Object, len: usize) -> Result<(), Box<dyn Error>> {
        let opcode: u8 = OpCode::ObjectCreate.into();
        let mut payload: Vec<u8> = vec![opcode, obj_type.into()];
        payload.extend_from_slice(&(len as u32).to_le_bytes());
        let response = self.request_ctrl(&payload).await?;
        Self::verify_header(opcode, &response)?;
        Ok(())
    }

    async fn execute(&self) -> Result<(), Box<dyn Error>> {
        let opcode: u8 = OpCode::ObjectExecute.into();
        let response = self.request_ctrl(&[opcode]).await?;
        Self::verify_header(opcode, &response)?;
        Ok(())
    }

    async fn verify_crc(&self, offset: usize, checksum: u32) -> Result<(), Box<dyn Error>> {
        let (off, crc) = self.get_crc().await?;
        if offset != off {
            return Err("Length mismatch".into());
        }
        if checksum != crc {
            return Err("CRC mismatch".into());
        }
        Ok(())
    }
}

/// Run DFU procedure as specified in
/// [DFU Protocol](https://infocenter.nordicsemi.com/topic/sdk_nrf5_v17.1.0/lib_dfu_transport_ble.html)
pub async fn dfu_run(transport: &impl DfuTransport, init_pkt: &[u8], fw_pkt: &[u8]) -> Result<(), Box<dyn Error>> {
    let target = DfuTarget { transport };
    target.set_prn(0).await?;

    target.create_object(Object::Command, init_pkt.len()).await?;
    target.write_data(init_pkt).await?;
    target.verify_crc(init_pkt.len(), crc32(init_pkt, 0)).await?;
    target.execute().await?;

    let pbar_len: u64 = fw_pkt.len() as u64;
    let bar = ProgressBar::new(pbar_len);

    let (max_size, offset, checksum) = target.select_object(Object::Data).await?;
    if offset != 0 || checksum != 0 {
        unimplemented!("DFU resumption is not supported");
    }
    let mut checksum: u32 = 0;
    let mut offset: usize = 0;

    println!("Started DFU upload of {} bytes", fw_pkt.len());
    for chunk in fw_pkt.chunks(max_size) {
        target.create_object(Object::Data, chunk.len()).await?;
        for shard in chunk.chunks(transport.mtu().await) {
            checksum = crc32(shard, checksum);
            offset += shard.len();
            target.write_data(shard).await?;
            target.verify_crc(offset, checksum).await?;
            // TODO add progress callback
            // println!("Uploaded {}/{} bytes", offset, fw_pkt.len());
            bar.set_position(offset as u64);
        }
        target.execute().await?;
    }

    println!("Finished DFU upload of {} bytes", fw_pkt.len());
    Ok(())
}
