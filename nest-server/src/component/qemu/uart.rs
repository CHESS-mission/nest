use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{os::unix::net::UnixStream, path::PathBuf, thread::{self, JoinHandle}};
use crossbeam_channel::{Receiver, Sender, unbounded};
use thiserror::Error;

use crate::component::Logger;
use crate::ports::BytesMessage;

type Result<T> = std::result::Result<T, UartError>;

/// Errors related to the UART backend.
#[derive(Error, Debug)]
pub enum UartError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Port out of range: {0}")]
    PortOutOfRange(usize),
    #[error("Failed to acquire lock")]
    AcquireLockFailed,
    #[error("Channel receiver dropped")]
    ReceiverDropped,
    #[error("Couldn't write to socket")]
    WriteError,
}

/// Represents a UART port
#[derive(Clone, Debug)]
pub struct UartPort {
    pub socket: PathBuf,
}

impl UartPort {
    pub fn new<P: Into<PathBuf>>(socket: P) -> Self {
        Self { socket: socket.into() }
    }
}

/// Represents a UART data transaction
#[derive(Clone, Debug)]
pub struct UartTransaction {
    pub port: usize,
    pub data: BytesMessage,
}

impl UartTransaction {
    pub fn new(port: usize, data: BytesMessage) -> Self {
        Self { port, data }
    }
}

/// A handle to the UART backend.
#[derive(Debug)]
pub struct UartBackendHandle {
    /// Sends UART data to the guest.
    pub data_tx: Sender<UartTransaction>,
    /// Receives UART data from the guest.
    pub data_rx: Receiver<UartTransaction>,

    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<Result<()>>>,
}

impl UartBackendHandle {
    /// Stop and join threads.
    pub fn stop_and_join(self) {
        self.stop.store(true, Ordering::Relaxed);
        for h in self.handles {
            let _ = h.join();
        }
    }
}

fn connect_with_retry(path: &Path, stop: &AtomicBool) -> Option<UnixStream> {
    let mut backoff = Duration::from_millis(50);
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }

        match UnixStream::connect(path) {
            Ok(s) => return Some(s),
            Err(_) => {
                // Typical if QEMU not ready yet
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(1));
            }
        }
    }
}

fn reconnect_replace(
    index: usize,
    path: &Path,
    stop: &AtomicBool,
    shared_streams: &Arc<Mutex<Vec<UnixStream>>>,
) -> Result<Option<UnixStream>> {
    let Some(new_stream) = connect_with_retry(path, stop)
    else { return Ok(None) };

    let w = new_stream.try_clone()?;
    {
        let mut streams = shared_streams.lock().map_err(|_| UartError::AcquireLockFailed)?;
        streams[index] = w;
    }

    Ok(Some(new_stream))
}

pub fn start_uart_backend(uart_ports: Vec<UartPort>, logger: Logger) -> Result<UartBackendHandle> {
    let (tx_to_backend, rx_to_backend) = unbounded::<UartTransaction>();
    let (tx_from_backend, rx_from_backend) = unbounded::<UartTransaction>();

    let stop = Arc::new(AtomicBool::new(false));
    let mut handles: Vec<JoinHandle<Result<()>>> = Vec::new();

    let shared_streams: Arc<Mutex<Vec<UnixStream>>> = Arc::new(Mutex::new(Vec::new()));

    // Connect to all sockets (with retries) and spawn reader threads
    for (index, port) in uart_ports.into_iter().enumerate() {
        let stop = Arc::clone(&stop);
        let tx_from_backend = tx_from_backend.clone();
        let shared_streams = Arc::clone(&shared_streams);
        let logger = logger.clone();

        // Connect + set up read thread
        let handle = thread::spawn(move || {
            (|| {
                let Some(mut stream) = connect_with_retry(&port.socket, &stop)
                else { return Ok(()) };

                // Put a cloned stream into the writable streams for dispatcher
                let w = stream.try_clone()?;
                shared_streams.lock()
                    .map_err(|_| UartError::AcquireLockFailed)?
                    .push(w);

                // Reader loop: blocking read, forward bytes to sim via tx_from_backend
                let mut buf = vec![0u8; 4096];
                while !stop.load(Ordering::Relaxed) {
                    match stream.read(&mut buf) {
                        Ok(0) => {
                            // Peer closed, reconnect and continue
                            let Some(new_stream) = reconnect_replace(index, &port.socket, &stop, &shared_streams)?
                            else { return Ok(()) };
                            stream = new_stream;
                        }
                        Ok(n) => {
                            // Forward data
                            let data = buf[..n].to_vec();
                            tx_from_backend.send(UartTransaction::new(index, data))
                                .map_err(|_| UartError::ReceiverDropped)?;
                        }
                        Err(_) => {
                            // Transient errors can happen; attempt reconnect
                            let Some(new_stream) = reconnect_replace(index, &port.socket, &stop, &shared_streams)?
                            else { return Ok(()) };
                            stream = new_stream;
                        }
                    }
                }

                Ok(())
            })()
            .inspect_err(|e: &UartError| {
                let _ = logger.error(format!("UartBackend error: {}", e.to_string()));
            })
        });

        handles.push(handle);
    }

    // Dispatcher thread: listens for outgoing transactions and writes to proper socket.
    {
        let stop = Arc::clone(&stop);
        let shared_streams = Arc::clone(&shared_streams);

        let handle = thread::spawn(move || {
            (|| {
                while !stop.load(Ordering::Relaxed) {
                    // Use recv_timeout so we can notice stop flag
                    match rx_to_backend.recv_timeout(Duration::from_millis(100)) {
                        Ok(msg) => {
                            let streams = shared_streams.lock().map_err(|_| UartError::AcquireLockFailed)?;
                            let mut stream = streams.get(msg.port).ok_or_else(|| UartError::PortOutOfRange(msg.port))?;
                            stream.write_all(&msg.data).map_err(|_| UartError::WriteError)?;
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                    }
                }

                Ok(())
            })()
            .inspect_err(|e: &UartError| {
                let _ = logger.error(format!("UartBackend error: {}", e.to_string()));
            })
        });

        handles.push(handle);
    }

    Ok(UartBackendHandle {
        data_tx: tx_to_backend,
        data_rx: rx_from_backend,
        stop,
        handles
    })
}

