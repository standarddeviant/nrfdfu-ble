use crate::transport::DfuTransport;

use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::error::Error;

// As defined in nRF5_SDK_17.1.0_ddde560/components/libraries/bootloader/dfu/nrf_dfu_req_handler.h

/// DFU Object variants
#[derive(Debug, Copy, Clone, TryFromPrimitive, IntoPrimitive)]
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

#[derive(Debug)]
enum ResponseData {
    Empty,
    Crc { offset: u32, checksum: u32 },
    Select { offset: u32, checksum: u32, max_size: u32 },
}

#[derive(Debug)]
struct Response {
    #[allow(dead_code)]
    request: OpCode,
    result: ResponseCode,
    data: ResponseData,
}

impl Response {
    const HEADER: u8 = 0x60;

    fn failed(&self) -> bool {
        self.result != ResponseCode::Success
    }

    fn try_from(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        if bytes[0] != Self::HEADER {
            return Err("Response packets must start with 0x60".into());
        }
        let request = OpCode::try_from(bytes[1])?;
        let result = ResponseCode::try_from(bytes[2])?;
        if result == ResponseCode::Success && request == OpCode::CrcGet {
            let data = ResponseData::Crc {
                offset: u32::from_le_bytes(bytes[3..7].try_into()?),
                checksum: u32::from_le_bytes(bytes[7..11].try_into()?),
            };
            return Ok(Response { request, result, data });
        }

        if result == ResponseCode::Success && request == OpCode::ObjectSelect {
            let data = ResponseData::Select {
                max_size: u32::from_le_bytes(bytes[3..7].try_into()?),
                offset: u32::from_le_bytes(bytes[7..11].try_into()?),
                checksum: u32::from_le_bytes(bytes[11..15].try_into()?),
            };
            return Ok(Response { request, result, data });
        }

        Ok(Response {
            request,
            result,
            data: ResponseData::Empty,
        })
    }
}

/// DFU Requests
///
/// More requests are available when `NRF_DFU_PROTOCOL_REDUCED` is not defined
/// in `nRF5_SDK_17.1.0_ddde560/components/libraries/bootloader/dfu/nrf_dfu_req_handler.c`
#[derive(Debug)]
enum Request {
    /// Create DFU object
    Create(Object, u32),
    /// Set Packet Receipt Notification frequency
    SetPrn(u32),
    /// Get current CRC
    GetCrc,
    /// Execute current DFU object
    Execute,
    /// Select object
    Select(Object),
    // TODO consider adding Request::Write
}

impl Request {
    // TODO use something better than Vec<u8>, maybe https://crates.io/crates/bytes
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = vec![];
        match self {
            Request::Create(obj_type, len) => {
                bytes.push(OpCode::ObjectCreate.into());
                bytes.push((*obj_type).into());
                bytes.extend_from_slice(&len.to_le_bytes());
            }
            Request::SetPrn(value) => {
                bytes.push(OpCode::ReceiptNotifSet.into());
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            Request::GetCrc => bytes.push(OpCode::CrcGet.into()),
            Request::Execute => bytes.push(OpCode::ObjectExecute.into()),
            Request::Select(obj_type) => {
                bytes.push(OpCode::ObjectSelect.into());
                bytes.push((*obj_type).into());
            }
        }
        bytes
    }
}

async fn request(transport: &impl DfuTransport, req: &Request) -> Result<Response, Box<dyn Error>> {
    for _retry in 0..3 {
        let res_raw = transport.request_ctrl(&req.to_bytes()).await;
        match res_raw {
            Err(e) => {
                if e.is::<tokio::time::error::Elapsed>() {
                    // response timed out, retry
                    continue;
                } else {
                    return Err(e);
                }
            }
            Ok(r) => {
                let res = Response::try_from(&r)?;
                if res.failed() {
                    return Err(format!("{:?}", res).into());
                }
                return Ok(res);
            }
        }
    }
    Err("No response after multiple tries".into())
}

fn crc32(buf: &[u8], init: u32) -> u32 {
    let mut h = crc32fast::Hasher::new_with_initial(init);
    h.update(buf);
    h.finalize()
}

/// Run DFU procedure as specified in
/// [DFU Protocol](https://infocenter.nordicsemi.com/topic/sdk_nrf5_v17.1.0/lib_dfu_transport_ble.html)
pub async fn dfu_run(transport: &impl DfuTransport, init_pkt: &[u8], fw_pkt: &[u8]) -> Result<(), Box<dyn Error>> {
    // TODO use PRN instead of polling the CRC after each write
    // disable packet receipt notifications
    request(transport, &Request::SetPrn(0)).await?;
    // create init packet
    let req_init = Request::Create(Object::Command, init_pkt.len().try_into()?);
    request(transport, &req_init).await?;
    // write init packet
    transport.write_data(init_pkt).await?;
    // verify CRC
    let res_crc = request(transport, &Request::GetCrc).await?;
    if let ResponseData::Crc { offset, checksum } = res_crc.data {
        if offset as usize != init_pkt.len() {
            return Err("Init packet write failed, length mismatch".into());
        }
        if checksum != crc32(init_pkt, 0) {
            return Err("Init packet write failed, CRC mismatch".into());
        }
    } else {
        return Err("Got unexpected respnse for Request::GetCrc".into());
    }
    request(transport, &Request::Execute).await?;

    let res_select = request(transport, &Request::Select(Object::Data)).await?;
    if let ResponseData::Select {
        offset,
        checksum,
        max_size,
    } = res_select.data
    {
        if offset != 0 || checksum != 0 {
            unimplemented!("DFU resumption is not supported");
        }
        let mut crc: u32 = 0;
        for chunk in fw_pkt.chunks(max_size as usize) {
            let req_chunk = Request::Create(Object::Data, chunk.len().try_into()?);
            request(transport, &req_chunk).await?;
            for shard in chunk.chunks(transport.mtu().await) {
                transport.write_data(shard).await?;
                let res_crc = request(transport, &Request::GetCrc).await?;
                if let ResponseData::Crc { offset, checksum } = res_crc.data {
                    // TODO add progress callback
                    println!("Uploaded {}/{} bytes", offset, fw_pkt.len());
                    if checksum != crc32(shard, crc) {
                        unimplemented!("invalid CRC recovery is not supported");
                    }
                    crc = checksum;
                }
            }
            request(transport, &Request::Execute).await?;
        }
        return Ok(());
    }
    unreachable!();
}
