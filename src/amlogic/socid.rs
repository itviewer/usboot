use std::fmt;

/// Stage minor constants
pub const STAGE_MINOR_IPL: u8 = 0;  // Initial Program Loader
pub const STAGE_MINOR_SPL: u8 = 8;  // Secondary Program Loader
pub const STAGE_MINOR_TPL: u8 = 16; // Tertiary Program Loader

/// Amlogic SoC ID parsed from the identify response.
#[derive(Debug, Clone)]
pub struct SocId {
    raw: Vec<u8>,
}

impl SocId {
    pub fn new(data: &str) -> Self {
        Self {
            raw: data.as_bytes().to_vec(),
        }
    }

    pub fn from_bytes(data: &[u8]) -> Self {
        Self {
            raw: data.to_vec(),
        }
    }

    pub fn major(&self) -> u8 {
        self.raw[0]
    }

    pub fn minor(&self) -> u8 {
        self.raw[1]
    }

    pub fn stage_major(&self) -> u8 {
        self.raw[2]
    }

    pub fn stage_minor(&self) -> u8 {
        self.raw[3]
    }

    pub fn need_password(&self) -> bool {
        assert!(self.raw.len() > 4);
        self.raw[4] != 0
    }

    pub fn password_ok(&self) -> bool {
        assert!(self.raw.len() > 5);
        self.raw[5] != 0
    }
}

impl fmt::Display for SocId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage_name = match (self.stage_major(), self.stage_minor()) {
            (0, STAGE_MINOR_IPL) => "IPL",
            (0, STAGE_MINOR_SPL) => "SPL",
            (0, STAGE_MINOR_TPL) => "TPL",
            _ => "UNKNOWN",
        };

        let mut pad = String::new();
        for &b in &self.raw[4..] {
            pad.push_str(&format!("-{}", b));
        }

        write!(
            f,
            "{}-{}-{}-{}{} ({})",
            self.major(),
            self.minor(),
            self.stage_major(),
            self.stage_minor(),
            pad,
            stage_name,
        )
    }
}