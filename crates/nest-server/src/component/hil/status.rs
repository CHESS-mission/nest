use std::fmt;

pub const STATUS_SIZE: usize = 3 * 4;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StatusError {
    #[error("status payload must be {STATUS_SIZE} bytes, got {0}")]
    BadLength(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub loop_cycles: u32,
    pub downstream_transfers: u32,
    pub upstream_transfers: u32,
}

impl Status {
    pub fn from_bytes(b: &[u8]) -> Result<Self, StatusError> {
        if b.len() != STATUS_SIZE { return Err(StatusError::BadLength(b.len())); }
        let word = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        Ok(Self { loop_cycles: word(0), downstream_transfers: word(4), upstream_transfers: word(8) })
    }
}

/// 1234567 -> "1'234'567"
fn fmt_grouped(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 { out.push('\''); }
        out.push(c);
    }
    out
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bridge status : ↺ {} cycles, ↓ {} bytes, ↑ {} bytes",
               fmt_grouped(self.loop_cycles),
               fmt_grouped(self.downstream_transfers),
               fmt_grouped(self.upstream_transfers))
    }
}
