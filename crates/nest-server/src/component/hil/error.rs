use std::{fmt, fs, path::Path};
use object::{Object, ObjectSymbol, SymbolKind};

const DEBUG_ELF_PATH: &str = "./Firmware/Debug/nest-fw-bridge.elf";
const RELEASE_ELF_PATH: &str = "./Firmware/Release/nest-fw-bridge.elf";

const ERROR_CODE_MASK: u8 = 0x7F;
const ADDRESS_TYPE_MASK: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressType { Debug, Release }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    NotImplementedYet, AssertionError, LogicError, MTUExceeded,
    InvalidTarget, InvalidCommand, InvalidChannel,
    UsbFailure,
    InvalidI2cSpeed, I2cOverUnderrun, I2cAckFailure, I2cArbitrationLost, I2cBusError,
    InvalidUartBaudrate, InvalidUartStopBits, InvalidUartParity,
    UartOverrun, UartNoiseError, UartFramingError, UartParityError,
    DmaTransferError, DmaDirectModeError, DmaFifoError,
    /// Firmware newer than this host library: don't panic, keep the raw value
    Unknown(u8),
}

impl From<u8> for Code {
    fn from(v: u8) -> Self {
        use Code::*;
        match v {
            0x00 => NotImplementedYet, 0x01 => AssertionError, 0x02 => LogicError, 0x03 => MTUExceeded,
            0x10 => InvalidTarget, 0x11 => InvalidCommand, 0x12 => InvalidChannel,
            0x20 => UsbFailure,
            0x30 => InvalidI2cSpeed, 0x31 => I2cOverUnderrun, 0x32 => I2cAckFailure,
            0x33 => I2cArbitrationLost, 0x34 => I2cBusError,
            0x40 => InvalidUartBaudrate, 0x41 => InvalidUartStopBits, 0x42 => InvalidUartParity,
            0x43 => UartOverrun, 0x44 => UartNoiseError, 0x45 => UartFramingError, 0x46 => UartParityError,
            0x50 => DmaTransferError, 0x51 => DmaDirectModeError, 0x52 => DmaFifoError,
            other => Unknown(other),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FirmwareError {
    pub address_type: AddressType,
    pub code: Code,
    pub address: u32,
}

impl FirmwareError {
    /// Payload layout: u32 LE address, then one byte (bit 7 = build type, bits 0..7 = code)
    pub fn parse(payload: &[u8]) -> Option<Self> {
        let address = u32::from_le_bytes(payload.get(..4)?.try_into().ok()?);
        let flags = *payload.get(4)?;
        Some(Self {
            address_type: if flags & ADDRESS_TYPE_MASK != 0 { AddressType::Release } else { AddressType::Debug },
            code: Code::from(flags & ERROR_CODE_MASK),
            address,
        })
    }

    /// Resolve the address against the firmware ELF (reads files, so call it on demand)
    pub fn traceback(&self) -> String {
        let build = match self.address_type { AddressType::Debug => "debug", AddressType::Release => "release" };
        let header = format!("{:?} raised at address 0x{:08X} in {build} build", self.code, self.address);

        let elf_path = Path::new(match self.address_type {
            AddressType::Debug => DEBUG_ELF_PATH,
            AddressType::Release => RELEASE_ELF_PATH,
        });
        if !elf_path.exists() {
            return format!("{header}\n  Firmware ELF not found ({}), it is needed to resolve the address.",
                           elf_path.display());
        }

        // Return addresses point after the call: drop the Thumb bit, step back into the call
        let addr = u64::from(self.address & !1).saturating_sub(2);

        let function = find_function(elf_path, addr);
        let location = find_line(elf_path, addr);

        if function.is_none() && location.is_none() {
            return format!("{header}\n  Address not in the ELF, the firmware probably does not match it.");
        }

        let mut lines = vec![header, format!("  in {}", function.as_deref().unwrap_or("??"))];
        match location {
            None => lines.push("  no line info (built without -g?)".into()),
            Some((path, line)) => {
                let name = Path::new(&path).file_name().map_or(path.clone(), |n| n.to_string_lossy().into_owned());
                lines.push(format!("  at {name}:{line}"));
                lines.push(match read_source_line(&path, line) {
                    Some(src) => format!("  {line:>5} | {src}"),
                    None => "        (source file not found)".into(),
                });
            }
        }
        lines.join("\n")
    }
}

impl fmt::Display for FirmwareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.traceback()) }
}
impl std::error::Error for FirmwareError {}

/// Demangle a C++ symbol, keeping GCC suffixes like ".part.0". Falls back to the raw name.
fn demangle(name: &str) -> String {
    let (base, suffix) = name.split_at(name.find('.').unwrap_or(name.len()));
    match cpp_demangle::Symbol::new(base) {
        Ok(sym) => format!("{sym}{suffix}"),
        Err(_) => name.to_string(),
    }
}

/// Name of the function whose symbol range contains addr.
fn find_function(elf_path: &Path, addr: u64) -> Option<String> {
    let data = fs::read(elf_path).ok()?;
    let file = object::File::parse(&*data).ok()?;
    file.symbols()
        .filter(|s| s.kind() == SymbolKind::Text)
        .find(|s| {
            let start = s.address() & !1;                 // drop the Thumb bit
            start <= addr && addr < start + s.size()
        })
        .and_then(|s| s.name().ok())
        .map(demangle)
}

/// (source file, line) of the line table row covering addr.
/// addr2line handles DWARF 4/5 file numbering and joins comp_dir for us.
fn find_line(elf_path: &Path, addr: u64) -> Option<(String, u32)> {
    let loader = addr2line::Loader::new(elf_path).ok()?;
    let loc = loader.find_location(addr).ok()??;
    Some((loc.file?.to_string(), loc.line?))
}

fn read_source_line(path: &str, line: u32) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    String::from_utf8_lossy(&bytes)
        .lines()
        .nth(usize::try_from(line).ok()?.checked_sub(1)?)
        .map(|l| l.trim().to_string())
}
