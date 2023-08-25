use crate::transport::DfuTransport;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::error::Error;

const OBJ_COMMAND: u8 = 0x01;
const OBJ_DATA: u8 = 0x02;

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
    Crc {
        offset: u32,
        checksum: u32,
    },
    Select {
        offset: u32,
        checksum: u32,
        max_size: u32,
    },
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
            return Ok(Response {
                request,
                result,
                data,
            });
        }

        if result == ResponseCode::Success && request == OpCode::ObjectSelect {
            let data = ResponseData::Select {
                max_size: u32::from_le_bytes(bytes[3..7].try_into()?),
                offset: u32::from_le_bytes(bytes[7..11].try_into()?),
                checksum: u32::from_le_bytes(bytes[11..15].try_into()?),
            };
            return Ok(Response {
                request,
                result,
                data,
            });
        }

        Ok(Response {
            request,
            result,
            data: ResponseData::Empty,
        })
    }
}

#[derive(Debug)]
enum Request {
    Create(u8, u32),
    SetPrn(u32),
    GetCrc,
    Execute,
    Select(u8),
}

impl Request {
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = vec![];
        match self {
            Request::Create(obj_type, len) => {
                bytes.push(OpCode::ObjectCreate.into());
                bytes.push(*obj_type);
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
                bytes.push(*obj_type);
            }
        }
        bytes
    }
}

async fn request(transport: &impl DfuTransport, req: &Request) -> Result<Response, Box<dyn Error>> {
    loop {
        transport.write_ctrl(&req.to_bytes())?;
        let res_raw = transport.listen_ctrl();
        match res_raw {
            Err(e) => {
                if e.is::<tokio::time::error::Elapsed>() {
                    println!("response timed out, retrying");
                    continue;
                } else {
                    return Err("fixme".into());
                }
            }
            Ok(r) => {
                let res = Response::try_from(&r)?;
                if res.failed() {
                    println!(">>> {:?}", req);
                    println!("<<< {:?}", res);
                    panic!();
                }
                return Ok(res);
            }
        }
    }
}

fn crc32(buf: &[u8], init: u32) -> u32 {
    let mut h = crc32fast::Hasher::new_with_initial(init);
    h.update(buf);
    h.finalize()
}

pub async fn dfu_run(
    transport: &impl DfuTransport,
    init_pkt: &[u8],
    fw_pkt: &[u8],
) -> Result<(), Box<dyn Error>> {
    // TODO enable and handle packet receipt notifications
    request(transport, &Request::SetPrn(0)).await?;
    // create init pkt
    let req_init = Request::Create(OBJ_COMMAND, init_pkt.len().try_into()?);
    request(transport, &req_init).await?;
    // write data
    transport.write_data(init_pkt)?;
    // verify CRC
    let res_crc = request(transport, &Request::GetCrc).await?;
    if let ResponseData::Crc { offset, checksum } = res_crc.data {
        assert_eq!(offset as usize, init_pkt.len());
        assert_eq!(checksum, crc32fast::hash(init_pkt));
    }
    request(transport, &Request::Execute).await?;

    let res_select = request(transport, &Request::Select(OBJ_DATA)).await?;
    if let ResponseData::Select {
        offset,
        checksum,
        max_size,
    } = res_select.data
    {
        assert_eq!(offset, 0);
        assert_eq!(checksum, 0);
        let mut crc: u32 = 0;
        for chunk in fw_pkt.chunks(max_size as usize) {
            let req_chunk = Request::Create(OBJ_DATA, chunk.len().try_into()?);
            request(transport, &req_chunk).await?;
            for shard in chunk.chunks(transport.mtu()) {
                transport.write_data(shard)?;
                let res_crc = request(transport, &Request::GetCrc).await?;
                if let ResponseData::Crc { offset, checksum } = res_crc.data {
                    // TODO add progress callback
                    println!("Uploaded {}/{} bytes", offset, fw_pkt.len());
                    // TODO implement bad checksum recovery
                    assert_eq!(checksum, crc32(shard, crc));
                    crc = checksum;
                }
            }
            request(transport, &Request::Execute).await?;
        }
        return Ok(());
    }
    unreachable!();
}
