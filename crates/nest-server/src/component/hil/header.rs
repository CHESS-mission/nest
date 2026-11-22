use crate::status::STATUS_SIZE;

// Sizes
pub const HEADER_SIZE: usize = 1;
pub const I2C_ADDR_SIZE: usize = 1;
pub const LENGTH_SIZE: usize = 2;
pub const ERROR_SIZE: usize = 4 + 1;

// Masks
const TARGET_MASK: u8 = 0b1100_0000;
const COMMAND_MASK: u8 = 0b0011_0000;
const CHANNEL_MASK: u8 = 0b0000_1111;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    #[error("invalid header byte 0x{0:02X}")]
    Invalid(u8),
    #[error("unexpected packet from device: {0:?}")]
    Unexpected(Header),
}

/// Generates a `#[repr(u8)]` enum plus a fallible `from_bits`.
macro_rules! bits_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident = $val:expr),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum $name { $($variant = $val),+ }

        impl $name {
            fn from_bits(v: u8) -> Option<Self> {
                match v { $(x if x == $val => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

// Commands (2 bits, unshifted). Each target has its own enum.
bits_enum!(BridgeCommand { Status = 0b00, Echo = 0b01, Error = 0b10 });
bits_enum!(GpioCommand   { Reset = 0b00, Set = 0b01, Get = 0b10 });
bits_enum!(I2cCommand    { Write = 0b00, Read = 0b01, WriteRead = 0b10, SetSpeed = 0b11 });
bits_enum!(UartCommand   { Transmit = 0b00, SetBaudrate = 0b01, SetStopBits = 0b10, SetParity = 0b11 });

// Channels (4 bits)
bits_enum!(GpioChannel {
    AdcsBoot = 0b0000, EpsHbA = 0b0001, EpsHbB = 0b0010, GnssEnable = 0b0011, GnssPwrOc = 0b0100,
    SpadCdone = 0b0101, SpadPgood = 0b0110, SpadReset = 0b0111, SpadBkpcfg = 0b1000,
});
bits_enum!(I2cChannel { Main = 0b0000, Eps = 0b0001 });
bits_enum!(UartChannel {
    Adcs = 0b0000, UhfA = 0b0001, UhfB = 0b0010, GnssA = 0b0011, GnssB = 0b0100,
    AdcsRs422 = 0b0101, SpadRs422 = 0b0110,
});

/// Only valid (target, command, channel) combinations are representable.
/// The bridge target has a single channel (0), so it carries none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Header {
    Bridge(BridgeCommand),
    Gpio(GpioCommand, GpioChannel),
    I2c(I2cCommand, I2cChannel),
    Uart(UartCommand, UartChannel),
}

impl Header {
    pub fn from_byte(b: u8) -> Result<Self, HeaderError> {
        let cmd = (b & COMMAND_MASK) >> 4;
        let ch = b & CHANNEL_MASK;
        let bad = || HeaderError::Invalid(b);

        Ok(match (b & TARGET_MASK) >> 6 {
            0 => {
                if ch != 0 { return Err(bad()); }
                Header::Bridge(BridgeCommand::from_bits(cmd).ok_or_else(bad)?)
            }
            1 => Header::Gpio(GpioCommand::from_bits(cmd).ok_or_else(bad)?,
                              GpioChannel::from_bits(ch).ok_or_else(bad)?),
            2 => Header::I2c(I2cCommand::from_bits(cmd).ok_or_else(bad)?,
                             I2cChannel::from_bits(ch).ok_or_else(bad)?),
            _ => Header::Uart(UartCommand::from_bits(cmd).ok_or_else(bad)?,   // target 0b11
                              UartChannel::from_bits(ch).ok_or_else(bad)?),
        })
    }

    pub fn to_byte(self) -> u8 {
        let pack = |target: u8, cmd: u8, ch: u8| (target << 6) | (cmd << 4) | ch;
        match self {
            Header::Bridge(c)  => pack(0b00, c as u8, 0),
            Header::Gpio(c, ch) => pack(0b01, c as u8, ch as u8),
            Header::I2c(c, ch)  => pack(0b10, c as u8, ch as u8),
            Header::Uart(c, ch) => pack(0b11, c as u8, ch as u8),
        }
    }

    /// Total packet size (header included) once enough bytes are buffered, `Ok(None)` if more
    /// bytes are needed. Err if the device sent a header that only the host should send.
    pub fn packet_size(&self, buf: &[u8]) -> Result<Option<usize>, HeaderError> {
        let fixed = |n: usize| (buf.len() >= n).then_some(n);
        // Fixed prefix, then a u16 LE length at `len_at`, then that many bytes
        let variable = |prefix: usize, len_at: usize| {
            if buf.len() < prefix { return None; }
            let len = u16::from_le_bytes([buf[len_at], buf[len_at + 1]]) as usize;
            (buf.len() >= prefix + len).then_some(prefix + len)
        };

        Ok(match self {
            Header::Bridge(BridgeCommand::Status) => fixed(HEADER_SIZE + STATUS_SIZE),
            Header::Bridge(BridgeCommand::Echo)   => variable(HEADER_SIZE + LENGTH_SIZE, 1),
            Header::Bridge(BridgeCommand::Error)  => fixed(HEADER_SIZE + ERROR_SIZE),

            Header::Gpio(GpioCommand::Reset | GpioCommand::Set, _) => Some(HEADER_SIZE),

            Header::I2c(I2cCommand::Read | I2cCommand::WriteRead, _) =>
                variable(HEADER_SIZE + I2C_ADDR_SIZE + LENGTH_SIZE, 2),

            Header::Uart(UartCommand::Transmit, _) => variable(HEADER_SIZE + LENGTH_SIZE, 1),

            _ => return Err(HeaderError::Unexpected(*self)),   // Python: assert False
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_roundtrip() {
        for b in 0..=255u8 {
            if let Ok(h) = Header::from_byte(b) {
                assert_eq!(h.to_byte(), b);
            }
        }
    }
}
