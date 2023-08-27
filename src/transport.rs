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
