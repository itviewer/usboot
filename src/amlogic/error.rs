use thiserror::Error;

#[derive(Error, Debug)]
pub enum AmlError {
    #[error("Device not found (vendor=0x{vendor:04x}, product=0x{product:04x})")]
    DeviceNotFound { vendor: u16, product: u16 },

    #[error("USB error: {0}")]
    Usb(#[from] nusb::Error),

    #[error("USB transfer error: {0}")]
    Transfer(#[from] nusb::transfer::TransferError),

    #[error("Maximum size of 64 bytes exceeded (got {0})")]
    DataTooLarge(usize),

    #[error("Large data must be a multiple of block length ({block_length}), got {data_length}")]
    BlockAlignment {
        data_length: usize,
        block_length: usize,
    },

    #[error("TPL command must be shorter than 127 characters (got {0})")]
    TplCommandTooLong(usize),

    #[error("Bulk command must be shorter than 127 characters (got {0})")]
    BulkCommandTooLong(usize),

    #[error("Invalid AMLC request: {0:?}")]
    InvalidAmlcRequest(Vec<u8>),

    #[error("Invalid AMLC data write ack: {0:?}")]
    InvalidAmlcAck(Vec<u8>),

    #[error("Timeout: {0}")]
    Timeout(&'static str),
}

pub type Result<T> = std::result::Result<T, AmlError>;