use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

use crate::{
    error::FirmwareError,
    header::*,
    status::{Status, StatusError},
};

#[derive(Debug, Clone, Copy)] #[repr(u8)] pub enum I2cSpeed { Slow = 0, Fast = 1 }
#[derive(Debug, Clone, Copy)] #[repr(u8)] pub enum UartStopBits { One = 0, Two = 1 }
#[derive(Debug, Clone, Copy)] #[repr(u8)] pub enum UartParity { No = 0, Even = 1, Odd = 2 }

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("device not found")] NotFound,
    #[error("serial: {0}")] Serial(#[from] serialport::Error),
    #[error("io: {0}")] Io(#[from] std::io::Error),
    #[error("timeout")] Timeout,
    #[error("a request for {0:?} is already pending")] Pending(Header),
    #[error("firmware error:\n{0}")] Firmware(FirmwareError),
    #[error(transparent)] Header(#[from] HeaderError),
    #[error(transparent)] Status(#[from] StatusError),
    #[error("bridge closed")] Closed,
    #[error("data too long for a u16 length field")] TooLong,
    #[error("malformed response")] Malformed,
    #[error("a listener is already set for {0:?}")] ListenerSet(Header),
}

type Reply = Result<Vec<u8>, BridgeError>;
type Pool = Arc<Mutex<HashMap<Header, oneshot::Sender<Reply>>>>;
type Listeners = Arc<Mutex<HashMap<Header, Box<dyn Fn(&[u8]) + Send>>>>;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
const GRACE_PERIOD: Duration = Duration::from_secs(1);

pub struct Bridge {
    writer: Mutex<Box<dyn serialport::SerialPort>>,
    pool: Pool,
    listeners: Listeners,
    alive: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Bridge {
    const VID: u16 = 1155;
    const PID: u16 = 22336;

    pub fn open() -> Result<Self, BridgeError> {
        let info = serialport::available_ports()?
            .into_iter()
            .find(|p| matches!(&p.port_type,
                serialport::SerialPortType::UsbPort(u) if u.vid == Self::VID && u.pid == Self::PID))
            .ok_or(BridgeError::NotFound)?;
        println!("Found bridge at {}", info.port_name);

        let port = serialport::new(&info.port_name, 115_200)
            .timeout(Duration::from_millis(1))
            .open()?;
        let reader = port.try_clone()?;

        let pool: Pool = Default::default();
        let listeners: Listeners = Default::default();
        let alive = Arc::new(AtomicBool::new(true));

        let thread = {
            let (pool, listeners, alive) = (pool.clone(), listeners.clone(), alive.clone());
            thread::Builder::new()
                .name("bridge-events".into())
                .spawn(move || event_thread(reader, pool, listeners, alive))?
        };
        Ok(Self { writer: Mutex::new(port), pool, listeners, alive, thread: Some(thread) })
    }

    fn write(&self, packet: &[u8]) -> Result<(), BridgeError> {
        self.writer.lock().unwrap().write_all(packet)?;
        Ok(())
    }

    fn send(&self, header: Header, payload: &[u8]) -> Result<(), BridgeError> {
        let mut packet = Vec::with_capacity(1 + payload.len());
        packet.push(header.to_byte());
        packet.extend_from_slice(payload);
        self.write(&packet)
    }

    async fn wait_response(&self, header: Header, payload: &[u8]) -> Result<Vec<u8>, BridgeError> {
        let (tx, rx) = oneshot::channel();
        {
            let mut pool = self.pool.lock().unwrap();
            if pool.contains_key(&header) { return Err(BridgeError::Pending(header)); }
            pool.insert(header, tx);
        }
        if let Err(e) = self.send(header, payload) {
            self.pool.lock().unwrap().remove(&header);
            return Err(e);
        }
        match tokio::time::timeout(DEFAULT_TIMEOUT, rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err(BridgeError::Closed),
            Err(_) => { self.pool.lock().unwrap().remove(&header); Err(BridgeError::Timeout) }
        }
    }

    fn len16(n: usize) -> Result<[u8; 2], BridgeError> {
        Ok(u16::try_from(n).map_err(|_| BridgeError::TooLong)?.to_le_bytes())
    }

    // ---- Bridge commands ----
    pub async fn bridge_status(&self) -> Result<Status, BridgeError> {
        let r = self.wait_response(Header::Bridge(BridgeCommand::Status), &[]).await?;
        Ok(Status::from_bytes(&r)?)
    }

    pub async fn bridge_echo(&self, message: &str) -> Result<String, BridgeError> {
        let mut p = Self::len16(message.len())?.to_vec();
        p.extend_from_slice(message.as_bytes());
        let r = self.wait_response(Header::Bridge(BridgeCommand::Echo), &p).await?;
        Ok(String::from_utf8_lossy(r.get(2..).ok_or(BridgeError::Malformed)?).into_owned())
    }

    // ---- GPIO ----
    pub fn gpio_reset(&self, ch: GpioChannel) -> Result<(), BridgeError> {
        self.send(Header::Gpio(GpioCommand::Reset, ch), &[])
    }
    pub fn gpio_set(&self, ch: GpioChannel) -> Result<(), BridgeError> {
        self.send(Header::Gpio(GpioCommand::Set, ch), &[])
    }
    pub async fn gpio_get(&self, ch: GpioChannel) -> Result<bool, BridgeError> {
        let r = self.wait_response(Header::Gpio(GpioCommand::Get, ch), &[]).await?;
        match (r.as_slice(), r.first().map(|b| Header::from_byte(*b))) {
            ([_], Some(Ok(Header::Gpio(GpioCommand::Reset, _)))) => Ok(false),
            ([_], Some(Ok(Header::Gpio(GpioCommand::Set, _)))) => Ok(true),
            _ => Err(BridgeError::Malformed),
        }
    }

    // ---- I2C ----
    pub fn i2c_write(&self, ch: I2cChannel, address: u8, data: &[u8]) -> Result<(), BridgeError> {
        let mut p = vec![address];
        p.extend_from_slice(&Self::len16(data.len())?);
        p.extend_from_slice(data);
        self.send(Header::I2c(I2cCommand::Write, ch), &p)
    }

    pub async fn i2c_read(&self, ch: I2cChannel, address: u8, length: u16) -> Result<Vec<u8>, BridgeError> {
        let mut p = vec![address];
        p.extend_from_slice(&length.to_le_bytes());
        let r = self.wait_response(Header::I2c(I2cCommand::Read, ch), &p).await?;
        Ok(r.get(3..).ok_or(BridgeError::Malformed)?.to_vec())
    }

    pub async fn i2c_write_read(&self, ch: I2cChannel, address: u8, data: &[u8], read_length: u16)
        -> Result<Vec<u8>, BridgeError>
    {
        let mut p = vec![address];
        p.extend_from_slice(&Self::len16(data.len())?);
        p.extend_from_slice(data);
        p.extend_from_slice(&read_length.to_le_bytes());
        let r = self.wait_response(Header::I2c(I2cCommand::WriteRead, ch), &p).await?;
        Ok(r.get(3..).ok_or(BridgeError::Malformed)?.to_vec())
    }

    pub fn i2c_set_speed(&self, ch: I2cChannel, speed: I2cSpeed) -> Result<(), BridgeError> {
        self.send(Header::I2c(I2cCommand::SetSpeed, ch), &[speed as u8])
    }

    // ---- UART ----
    pub fn uart_transmit(&self, ch: UartChannel, data: &[u8]) -> Result<(), BridgeError> {
        let mut p = Self::len16(data.len())?.to_vec();
        p.extend_from_slice(data);
        self.send(Header::Uart(UartCommand::Transmit, ch), &p)
    }

    pub fn uart_set_receive_listener(&self, ch: UartChannel, f: impl Fn(&[u8]) + Send + 'static)
        -> Result<(), BridgeError>
    {
        let h = Header::Uart(UartCommand::Transmit, ch);
        let mut map = self.listeners.lock().unwrap();
        if map.contains_key(&h) { return Err(BridgeError::ListenerSet(h)); }
        map.insert(h, Box::new(f));
        Ok(())
    }

    pub fn uart_clear_receive_listener(&self, ch: UartChannel) {
        self.listeners.lock().unwrap().remove(&Header::Uart(UartCommand::Transmit, ch));
    }

    pub fn uart_set_baudrate(&self, ch: UartChannel, baudrate: u32) -> Result<(), BridgeError> {
        self.send(Header::Uart(UartCommand::SetBaudrate, ch), &baudrate.to_le_bytes())
    }
    pub fn uart_set_stop_bits(&self, ch: UartChannel, s: UartStopBits) -> Result<(), BridgeError> {
        self.send(Header::Uart(UartCommand::SetStopBits, ch), &[s as u8])
    }
    pub fn uart_set_parity(&self, ch: UartChannel, p: UartParity) -> Result<(), BridgeError> {
        self.send(Header::Uart(UartCommand::SetParity, ch), &[p as u8])
    }

    // ---- Shutdown (the __exit__ equivalent) ----
    pub fn close(&mut self) {
        // Let pending requests finish (bounded, so a dead reader can't hang us)
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.pool.lock().unwrap().is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        // Grace period so late firmware errors aren't missed
        thread::sleep(GRACE_PERIOD);
        self.alive.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) { if self.thread.is_some() { self.close(); } }
}

fn fail_all(pool: &Pool, mk: impl Fn() -> BridgeError) {
    for (_, tx) in pool.lock().unwrap().drain() { let _ = tx.send(Err(mk())); }
}

fn event_thread(mut port: Box<dyn serialport::SerialPort>, pool: Pool, listeners: Listeners, alive: Arc<AtomicBool>) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 256];

    while alive.load(Ordering::Relaxed) {
        match port.read(&mut chunk) {
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => { fail_all(&pool, || BridgeError::Closed); return; }
        }

        while buf.len() >= HEADER_SIZE {
            // A bad header means the stream is out of sync: drop the byte and notify callers
            let header = match Header::from_byte(buf[0]) {
                Ok(h) => h,
                Err(e) => { buf.remove(0); fail_all(&pool, || BridgeError::Header(e.clone())); continue; }
            };
            let size = match header.packet_size(&buf) {
                Ok(Some(s)) => s,
                Ok(None) => break,
                Err(e) => { buf.remove(0); fail_all(&pool, || BridgeError::Header(e.clone())); continue; }
            };
            let payload: Vec<u8> = buf[HEADER_SIZE..size].to_vec();
            buf.drain(..size);

            match header {
                Header::Bridge(BridgeCommand::Error) => {
                    if let Some(err) = FirmwareError::parse(&payload) {
                        fail_all(&pool, || BridgeError::Firmware(err.clone()));
                    }
                }
                Header::Uart(UartCommand::Transmit, _) => {
                    if let Some(f) = listeners.lock().unwrap().get(&header) { f(&payload[2..]); }
                }
                Header::Gpio(GpioCommand::Reset | GpioCommand::Set, ch) => {
                    let req = Header::Gpio(GpioCommand::Get, ch);
                    if let Some(tx) = pool.lock().unwrap().remove(&req) {
                        let _ = tx.send(Ok(vec![header.to_byte()]));
                    }
                }
                _ => {
                    if let Some(tx) = pool.lock().unwrap().remove(&header) { let _ = tx.send(Ok(payload)); }
                }
            }
        }
    }
}
