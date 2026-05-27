//! Amlogic USB Boot Protocol Library

mod error;
mod socid;

pub use error::{AmlError, Result};
pub use socid::SocId;

use nusb::transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Out, Recipient};
use nusb::Interface;
use std::time::{Duration, Instant};
use tokio::time::sleep;
// ── Protocol request codes ──────────────────────────────────────────────────

const REQ_WRITE_MEM: u8 = 0x01;
const REQ_READ_MEM: u8 = 0x02;
// const REQ_FILL_MEM: u8 = 0x03;
const REQ_MODIFY_MEM: u8 = 0x04;
const REQ_RUN_IN_ADDR: u8 = 0x05;
// const REQ_WRITE_AUX: u8 = 0x06;
// const REQ_READ_AUX: u8 = 0x07;

const REQ_WR_LARGE_MEM: u8 = 0x11;
const REQ_RD_LARGE_MEM: u8 = 0x12;
const REQ_IDENTIFY_HOST: u8 = 0x20;

const REQ_TPL_CMD: u8 = 0x30;
const REQ_TPL_STAT: u8 = 0x31;

const REQ_WRITE_MEDIA: u8 = 0x32;
const REQ_READ_MEDIA: u8 = 0x33;

const REQ_BULKCMD: u8 = 0x34;

const REQ_PASSWORD: u8 = 0x35;
const REQ_NOP: u8 = 0x36;

const REQ_GET_AMLC: u8 = 0x50;
const REQ_WRITE_AMLC: u8 = 0x60;

const FLAG_KEEP_POWER_ON: u32 = 0x10;

const AMLC_AMLS_BLOCK_LENGTH: usize = 0x200;
const AMLC_MAX_BLOCK_LENGTH: usize = 0x4000;
const AMLC_MAX_TRANSFER_LENGTH: usize = 65536;

const MAX_LARGE_BLOCK_COUNT: usize = 65535;

// const WRITE_MEDIA_CHECKSUM_ALG_NONE: u16 = 0x00ee;
const WRITE_MEDIA_CHECKSUM_ALG_ADDSUM: u16 = 0x00ef;
// const WRITE_MEDIA_CHECKSUM_ALG_CRC32: u16 = 0x00f0;

const DEFAULT_VENDOR_ID: u16 = 0x1b8e;
const DEFAULT_PRODUCT_ID: u16 = 0xc003;

const CTRL_TIMEOUT: Duration = Duration::from_secs(1);
const BULK_TIMEOUT: Duration = Duration::from_secs(2);
const BULK_REPLY_LEN: usize = 512;

// ── Helper: little-endian packing ───────────────────────────────────────────

fn pack_u32x4(a: u32, b: u32, c: u32, d: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(16);
    v.extend_from_slice(&a.to_le_bytes());
    v.extend_from_slice(&b.to_le_bytes());
    v.extend_from_slice(&c.to_le_bytes());
    v.extend_from_slice(&d.to_le_bytes());
    v
}

// ── AmlogicSoC ──────────────────────────────────────────────────────────────

/// Represents an Amlogic SoC in USB boot mode.
pub struct AmlogicSoC {
    iface: Interface,
}

impl AmlogicSoC {
    /// Open the first matching Amlogic device.
    pub async fn new(
        vendor: u16,
        product: u16,
        timeout: Duration,
    ) -> Result<Self> {
        let vid = vendor;
        let pid = product;

        let start = Instant::now();
        let device = loop {
            let found = nusb::list_devices()
                .await
                .map_err(AmlError::Usb)?
                .find(|d| d.vendor_id() == vid && d.product_id() == pid);

            if let Some(info) = found {
                break info.open().await.map_err(AmlError::Usb)?;
            }

            if timeout > Duration::ZERO {
                if start.elapsed() >= timeout {
                    return Err(AmlError::DeviceNotFound {
                        vendor: vid,
                        product: pid,
                    });
                }
                sleep(Duration::from_millis(100)).await;
            } else {
                return Err(AmlError::DeviceNotFound {
                    vendor: vid,
                    product: pid,
                });
            }
        };

        let iface = device.claim_interface(0).await.map_err(AmlError::Usb)?;

        Ok(Self {
            iface,
        })
    }

    pub async fn with_defaults(timeout: Duration) -> Result<Self> {
        Self::new(DEFAULT_VENDOR_ID, DEFAULT_PRODUCT_ID, timeout).await
    }

    // ── Control transfer helpers ────────────────────────────────────────

    /// Vendor OUT control transfer (bmRequestType = 0x40).
    async fn ctrl_out(
        &self,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
    ) -> Result<()> {
        self.iface
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request,
                    value,
                    index,
                    data,
                },
                CTRL_TIMEOUT,
            )
            .await?;
        Ok(())
    }

    /// Vendor IN control transfer (bmRequestType = 0xC0).
    async fn ctrl_in(
        &self,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
    ) -> Result<Vec<u8>> {
        let data = self
            .iface
            .control_in(
                ControlIn {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request,
                    value,
                    index,
                    length,
                },
                CTRL_TIMEOUT,
            )
            .await?;
        Ok(data)
    }

    // ── Bulk endpoint helpers ───────────────────────────────────────────

    /// Write to the first bulk-OUT endpoint (0x02).
    async fn bulk_write(&self, data: &[u8]) -> Result<()> {
        let mut ep_out = self
            .iface
            .endpoint::<Bulk, Out>(0x02)
            .map_err(AmlError::Usb)?;
        let buf: Buffer = data.to_vec().into();
        ep_out.submit(buf);
        // let completion = ep_out.next_complete().await;
        // completion.status?;
        // Ok(())

        if let Some(completion) = ep_out.wait_next_complete(BULK_TIMEOUT) {
            completion.status?;
            Ok(())
        } else {
            // not a TransferError
            Err(AmlError::Timeout("waiting for bulk out transfer to complete"))
        }
    }

    /// Read from the first bulk-IN endpoint (0x81).
    ///
    /// The internal buffer is rounded up to a multiple of the endpoint's
    /// max packet size as required by nusb. The returned `Vec` is then
    /// truncated to at most `len` bytes.
    async fn bulk_read(&self, len: usize) -> Result<Vec<u8>> {
        let mut ep_in = self
            .iface
            .endpoint::<Bulk, In>(0x81)
            .map_err(AmlError::Usb)?;

        // Buffer must be a nonzero multiple of max_packet_size.
        let mps = ep_in.max_packet_size();
        let alloc_len = if len == 0 { mps } else { len.div_ceil(mps) * mps };
        let buf = Buffer::new(alloc_len);

        ep_in.submit(buf);

        // let completion = ep_in.next_complete().await;
        // completion.status?;
        //
        // let mut data = completion.buffer.into_vec();
        // data.truncate(len);

        // Ok(data)

        if let Some(completion) = ep_in.wait_next_complete(BULK_TIMEOUT) {
            completion.status?;

            let mut data = completion.buffer.into_vec();
            data.truncate(len);

            Ok(data)
        } else {
            Err(AmlError::Timeout("waiting for bulk in transfer to complete"))
        }
    }

    // ── Simple memory operations (≤ 64 bytes via control transfers) ─────

    /// Write a chunk of data (max 64 bytes) to the given memory address.
    pub async fn write_simple_memory(&self, address: u32, data: &[u8]) -> Result<()> {
        if data.len() > 64 {
            return Err(AmlError::DataTooLarge(data.len()));
        }
        self.ctrl_out(
            REQ_WRITE_MEM,
            (address >> 16) as u16,
            (address & 0xffff) as u16,
            data,
        )
            .await
    }

    /// Write data to memory in 64-byte chunks via control transfers.
    pub async fn write_memory(&self, address: u32, data: &[u8]) -> Result<()> {
        let mut offset = 0usize;
        let length = data.len();
        loop {
            let end = (offset + 64).min(length);
            self.write_simple_memory(address + offset as u32, &data[offset..end])
                .await?;
            if end >= length {
                break;
            }
            offset += 64;
        }
        Ok(())
    }

    /// Read a chunk of data (max 64 bytes) from the given memory address.
    pub async fn read_simple_memory(&self, address: u32, length: usize) -> Result<Vec<u8>> {
        if length == 0 {
            return Ok(Vec::new());
        }
        if length > 64 {
            return Err(AmlError::DataTooLarge(length));
        }
        self.ctrl_in(
            REQ_READ_MEM,
            (address >> 16) as u16,
            (address & 0xffff) as u16,
            length as u16,
        )
            .await
    }

    /// Read data from memory in 64-byte chunks via control transfers.
    pub async fn read_memory(&self, address: u32, length: usize) -> Result<Vec<u8>> {
        let mut data = Vec::with_capacity(length);
        let mut remaining = length;
        let mut offset = 0u32;

        while remaining > 0 {
            let chunk = remaining.min(64);
            let part = self.read_simple_memory(address + offset, chunk).await?;
            data.extend_from_slice(&part);
            remaining -= chunk;
            offset += chunk as u32;
        }
        Ok(data)
    }

    // ── Modify memory ───────────────────────────────────────────────────

    pub async fn modify_memory(
        &self,
        opcode: u16,
        address1: u32,
        data: u32,
        mask: u32,
        address2: u32,
    ) -> Result<()> {
        let control_data = pack_u32x4(address1, data, mask, address2);
        self.ctrl_out(REQ_MODIFY_MEM, opcode, 0, &control_data)
            .await
    }

    pub async fn read_reg(&self, address: u32) -> Result<u32> {
        let reg = self.read_simple_memory(address, 4).await?;
        Ok(u32::from_le_bytes([reg[0], reg[1], reg[2], reg[3]]))
    }

    pub async fn write_reg(&self, address: u32, value: u32) -> Result<()> {
        self.modify_memory(0, address, value, 0, 0).await
    }

    pub async fn mask_reg_and(&self, address: u32, mask: u32) -> Result<()> {
        self.modify_memory(1, address, 0, mask, 0).await
    }

    pub async fn mask_reg_or(&self, address: u32, mask: u32) -> Result<()> {
        self.modify_memory(2, address, 0, mask, 0).await
    }

    pub async fn mask_reg_nand(&self, address: u32, mask: u32) -> Result<()> {
        self.modify_memory(3, address, 0, mask, 0).await
    }

    pub async fn write_reg_bits(&self, address: u32, mask: u32, value: u32) -> Result<()> {
        self.modify_memory(4, address, value, mask, 0).await
    }

    pub async fn copy_reg(&self, source: u32, dest: u32) -> Result<()> {
        self.modify_memory(5, dest, 0, 0, source).await
    }

    pub async fn copy_reg_mask_and(&self, source: u32, dest: u32, mask: u32) -> Result<()> {
        self.modify_memory(6, dest, 0, mask, source).await
    }

    pub async fn memcpy(&self, dest: u32, src: u32, n: u32) -> Result<()> {
        self.modify_memory(7, src, n, 0, dest).await
    }

    // ── Run ─────────────────────────────────────────────────────────────

    pub async fn run(&self, address: u32, keep_power: bool) -> Result<()> {
        let data_val = if keep_power {
            address | FLAG_KEEP_POWER_ON
        } else {
            address
        };
        let control_data = data_val.to_le_bytes().to_vec();
        self.ctrl_out(
            REQ_RUN_IN_ADDR,
            (address >> 16) as u16,
            (address & 0xffff) as u16,
            &control_data,
        )
            .await
    }

    // ── Large memory transfers (via bulk endpoints) ─────────────────────

    async fn write_large_memory_inner(
        &self,
        address: u32,
        data: &[u8],
        block_length: usize,
        append_zeros: bool,
    ) -> Result<()> {
        let mut buf;
        let payload: &[u8] = if append_zeros {
            let remainder = data.len() % block_length;
            if remainder != 0 {
                buf = data.to_vec();
                buf.resize(data.len() + (block_length - remainder), 0);
                &buf
            } else {
                data
            }
        } else {
            if data.len() % block_length != 0 {
                return Err(AmlError::BlockAlignment {
                    data_length: data.len(),
                    block_length,
                });
            }
            data
        };

        let mut block_count = payload.len() / block_length;
        if payload.len() % block_length > 0 {
            block_count += 1;
        }

        let control_data = pack_u32x4(address, payload.len() as u32, 0, 0);

        self.ctrl_out(
            REQ_WR_LARGE_MEM,
            block_length as u16,
            block_count as u16,
            &control_data,
        )
            .await?;

        let mut offset = 0;
        for _ in 0..block_count {
            let end = (offset + block_length).min(payload.len());
            self.bulk_write(&payload[offset..end]).await?;
            offset += block_length;
        }
        Ok(())
    }

    pub async fn write_large_memory(
        &self,
        address: u32,
        data: &[u8],
        block_length: usize,
        append_zeros: bool,
    ) -> Result<()> {
        let block_count = (data.len() + block_length - 1) / block_length;
        let transfer_count = (block_count + MAX_LARGE_BLOCK_COUNT - 1) / MAX_LARGE_BLOCK_COUNT;
        let mut offset = 0usize;

        for _ in 0..transfer_count {
            let write_length = if offset + MAX_LARGE_BLOCK_COUNT * block_length > data.len() {
                data.len() - offset
            } else {
                MAX_LARGE_BLOCK_COUNT * block_length
            };
            self.write_large_memory_inner(
                address + offset as u32,
                &data[offset..offset + write_length],
                block_length,
                append_zeros,
            )
                .await?;
            offset += write_length;
        }
        Ok(())
    }

    async fn read_large_memory_inner(
        &self,
        address: u32,
        length: usize,
        block_length: usize,
        append_zeros: bool,
    ) -> Result<Vec<u8>> {
        let actual_length = if append_zeros {
            length + (length % block_length)
        } else {
            if length % block_length != 0 {
                return Err(AmlError::BlockAlignment {
                    data_length: length,
                    block_length,
                });
            }
            length
        };

        let mut block_count = actual_length / block_length;
        if actual_length % block_length > 0 {
            block_count += 1;
        }

        let control_data = pack_u32x4(address, actual_length as u32, 0, 0);

        self.ctrl_out(
            REQ_RD_LARGE_MEM,
            block_length as u16,
            block_count as u16,
            &control_data,
        )
            .await?;

        let mut data = Vec::with_capacity(actual_length);
        for _ in 0..block_count {
            let chunk = self.bulk_read(block_length).await?;
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }

    pub async fn read_large_memory(
        &self,
        address: u32,
        length: usize,
        block_length: usize,
        append_zeros: bool,
    ) -> Result<Vec<u8>> {
        let block_count = (length + block_length - 1) / block_length;
        let transfer_count = (block_count + MAX_LARGE_BLOCK_COUNT - 1) / MAX_LARGE_BLOCK_COUNT;
        let mut offset = 0usize;
        let mut data = Vec::with_capacity(length);

        for _ in 0..transfer_count {
            let read_length = if offset + MAX_LARGE_BLOCK_COUNT * block_length > length {
                length - offset
            } else {
                MAX_LARGE_BLOCK_COUNT * block_length
            };
            let chunk = self
                .read_large_memory_inner(
                    address + offset as u32,
                    read_length,
                    block_length,
                    append_zeros,
                )
                .await?;
            data.extend_from_slice(&chunk);
            offset += read_length;
        }
        Ok(data)
    }

    // ── Identify ────────────────────────────────────────────────────────

    pub async fn identify(&self) -> Result<String> {
        let ret = self.ctrl_in(REQ_IDENTIFY_HOST, 0, 0, 8).await?;
        Ok(ret.iter().map(|&b| b as char).collect())
    }

    // ── TPL commands ────────────────────────────────────────────────────

    pub async fn tpl_command(&self, subcode: u16, command: &str) -> Result<()> {
        let terminated = format!("{}\0", command);
        if terminated.len() >= 128 {
            return Err(AmlError::TplCommandTooLong(terminated.len()));
        }
        self.ctrl_out(REQ_TPL_CMD, 0, subcode, terminated.as_bytes())
            .await
    }

    pub async fn tpl_stat(&self) -> Result<Vec<u8>> {
        self.ctrl_in(REQ_TPL_STAT, 0, 0, 0x40).await
    }

    // ── Password ────────────────────────────────────────────────────────

    pub async fn send_password(&self, password: &[u8]) -> Result<()> {
        self.ctrl_out(REQ_PASSWORD, 0, 0, password).await
    }

    // ── NOP ─────────────────────────────────────────────────────────────

    pub async fn nop(&self) -> Result<()> {
        self.ctrl_out(REQ_NOP, 0, 0, &[]).await
    }

    // ── AMLC / AMLS (BL2 boot) ─────────────────────────────────────────

    pub async fn get_boot_amlc(&self) -> Result<(u32, u32)> {
        self.ctrl_out(REQ_GET_AMLC, AMLC_AMLS_BLOCK_LENGTH as u16, 0, &[])
            .await?;

        let data = self.bulk_read(AMLC_AMLS_BLOCK_LENGTH).await?;

        if data.len() < 16 {
            return Err(AmlError::InvalidAmlcRequest(data));
        }
        let tag = &data[0..4];
        if tag != b"AMLC" {
            return Err(AmlError::InvalidAmlcRequest(data[0..16].to_vec()));
        }

        let length = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let offset = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

        // Ack with "OKAY"
        let mut okay = Vec::with_capacity(16);
        okay.extend_from_slice(b"OKAY");
        okay.extend_from_slice(&0u32.to_le_bytes());
        okay.extend_from_slice(&0u32.to_le_bytes());
        okay.extend_from_slice(&0u32.to_le_bytes());
        self.bulk_write(&okay).await?;

        Ok((length, offset))
    }

    pub async fn write_amlc_data(
        &self,
        seq: u8,
        amlc_offset: usize,
        data: &[u8],
    ) -> Result<()> {
        let data_len = data.len();
        let transfer_count = (data_len + AMLC_MAX_TRANSFER_LENGTH - 1) / AMLC_MAX_TRANSFER_LENGTH;
        let mut offset = 0usize;

        for _ in 0..transfer_count {
            let write_length = if offset + AMLC_MAX_TRANSFER_LENGTH > data_len {
                data_len - offset
            } else {
                AMLC_MAX_TRANSFER_LENGTH
            };
            self.write_amlc_data_inner(offset, &data[offset..offset + write_length])
                .await?;
            offset += write_length;
        }

        let checksum = Self::amls_checksum(data);
        let mut amls = Vec::with_capacity(AMLC_AMLS_BLOCK_LENGTH);
        amls.extend_from_slice(b"AMLS");
        amls.push(seq);
        amls.push(0);
        amls.push(0);
        amls.push(0);
        amls.extend_from_slice(&checksum.to_le_bytes());
        amls.extend_from_slice(&0u32.to_le_bytes());
        if data.len() > 16 {
            let end = data.len().min(512);
            amls.extend_from_slice(&data[16..end]);
        }
        if amls.len() < AMLC_AMLS_BLOCK_LENGTH {
            amls.resize(AMLC_AMLS_BLOCK_LENGTH, 0);
        }

        self.write_amlc_data_inner(amlc_offset, &amls).await
    }

    async fn write_amlc_data_inner(&self, offset: usize, data: &[u8]) -> Result<()> {
        let write_length = data.len();
        let mut block_count = write_length / AMLC_MAX_BLOCK_LENGTH;
        if write_length % AMLC_MAX_BLOCK_LENGTH > 0 {
            block_count += 1;
        }

        self.ctrl_out(
            REQ_WRITE_AMLC,
            (offset / AMLC_AMLS_BLOCK_LENGTH) as u16,
            (write_length - 1) as u16,
            &[],
        )
            .await?;

        let mut data_offset = 0;
        for _ in 0..block_count {
            let remain = write_length - data_offset;
            let block_len = remain.min(AMLC_MAX_BLOCK_LENGTH);
            self.bulk_write(&data[data_offset..data_offset + block_len])
                .await?;
            data_offset += block_len;
        }

        // Wait for ack
        let ack = self.bulk_read(16).await?;
        if ack.len() < 4 || &ack[0..4] != b"OKAY" {
            return Err(AmlError::InvalidAmlcAck(ack));
        }
        Ok(())
    }

    fn amls_checksum(data: &[u8]) -> u32 {
        let mut checksum: u32 = 0;
        let mut offset = 0usize;

        while offset < data.len() {
            let left = data.len() - offset;
            let val: u32 = if left >= 4 {
                u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ])
            } else if left >= 3 {
                u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], 0])
                    & 0x00ff_ffff
            } else if left >= 2 {
                u16::from_le_bytes([data[offset], data[offset + 1]]) as u32
            } else {
                data[offset] as u32
            };
            offset += 4;
            checksum = checksum.wrapping_add(val);
        }
        checksum
    }

    // ── Media read / write ──────────────────────────────────────────────

    pub async fn read_media(&self, size: usize) -> Result<Vec<u8>> {
        let block_length: usize = 0x1000;
        let blocks = (block_length + size - 1) / block_length;
        let control_data = pack_u32x4(0, size as u32, 0, 0);

        self.ctrl_out(REQ_READ_MEDIA, size as u16, blocks as u16, &control_data)
            .await?;

        self.bulk_read(size).await
    }

    pub async fn write_media(
        &self,
        data: &[u8],
        ack_len: u16,
        seq: u32,
        retry_times: u32,
    ) -> Result<bool> {
        let checksum = Self::amls_checksum(data);

        let mut control_data = Vec::with_capacity(0x20);
        control_data.extend_from_slice(&retry_times.to_le_bytes());
        control_data.extend_from_slice(&(data.len() as u32).to_le_bytes());
        control_data.extend_from_slice(&seq.to_le_bytes());
        control_data.extend_from_slice(&checksum.to_le_bytes());
        control_data.extend_from_slice(&WRITE_MEDIA_CHECKSUM_ALG_ADDSUM.to_le_bytes());
        control_data.extend_from_slice(&ack_len.to_le_bytes());
        control_data.resize(0x20, 0);

        self.ctrl_out(REQ_WRITE_MEDIA, 1, 0xffff, &control_data)
            .await?;

        self.bulk_write(data).await?;
        Ok(true)
    }

    // ── Bulk commands (U-Boot) ──────────────────────────────────────────

    pub async fn dev_read(&self, size: usize) -> Result<Vec<u8>> {
        self.bulk_read(size).await
    }

    pub async fn bulk_cmd(
        &self,
        command: &str,
        read_status: bool,
    ) -> Result<Option<Vec<u8>>> {
        let terminated = format!("{}\0", command);
        if terminated.len() >= 128 {
            return Err(AmlError::BulkCommandTooLong(terminated.len()));
        }

        self.ctrl_out(REQ_BULKCMD, 0, 2, terminated.as_bytes())
            .await?;

        if read_status {
            Ok(Some(self.bulk_cmd_stat().await?))
        } else {
            Ok(None)
        }
    }

    pub async fn bulk_cmd_stat(&self) -> Result<Vec<u8>> {
        self.bulk_read(BULK_REPLY_LEN).await
    }
}