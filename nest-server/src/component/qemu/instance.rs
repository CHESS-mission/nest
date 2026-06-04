use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Custom Error type relative to QEMU.
#[derive(Error, Debug)]
pub enum QemuError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("QEMU process failed to spawn: {0}")]
    ProcessSpawn(std::io::Error),
    #[error("Failed to connect to QMP socket at {path}: {source}")]
    QmpConnect { path: PathBuf, source: std::io::Error },
    #[error("Timed out waiting for QMP socket to become available")]
    QmpSocketTimeout,
    #[error("QMP socket disconnected")]
    QmpSocketDisconnected,
    #[error("QMP negotiation failed")]
    QmpNegotiation,
    #[error("QMP response indicates an error: {0:?}")]
    QmpResponse(serde_json::Value),
    #[error("QMP response must containt an id")]
    QmpResponseNoID,
}

type Result<T> = std::result::Result<T, QemuError>;

/// Represents a QEMU command line argument.
#[derive(Debug, Clone)]
enum QemuArg {
    Option { name: String, value: String },
    Flag(String)
}

/// Represents the configuration for a QEMU instance.
#[derive(Debug, Clone)]
pub struct QemuConfig {
    pub arch: String,
    pub memory_mb: u32,
    extra_args: Vec<QemuArg>,
    uart_sockets: Vec<PathBuf>,
    gpio_backend_socket: Option<PathBuf>,
    i2c_backend_socket: Option<PathBuf>,
    qmp_socket_path: PathBuf,
}

impl QemuConfig {
    /// Create a new configuration.
    pub fn new(arch: String, memory_mb: u32) -> Self {
        let qmp_socket_path = std::env::temp_dir().join(format!("qmp-{}.sock", uuid::Uuid::new_v4()));

        Self {
            arch,
            memory_mb,
            extra_args: Vec::new(),
            uart_sockets: Vec::new(),
            gpio_backend_socket: None,
            i2c_backend_socket: None,
            qmp_socket_path,
        }
    }

    /// Add a command-line option.
    pub fn add_option<N: Into<String>, V: Into<String>>(&mut self, name: N, value: V) {
        self.extra_args.push(QemuArg::Option { name: name.into(), value: value.into() });
    }

    /// Add a command-line flag.
    pub fn add_flag<S: Into<String>>(&mut self, flag: S) {
        self.extra_args.push(QemuArg::Flag(flag.into()));
    }

    /// Add a socket to connect to a UART port.
    pub fn add_uart_socket<P: Into<PathBuf>>(&mut self, socket: P) {
        self.uart_sockets.push(socket.into());
    }

    /// Enable communication with the vhost-user gpio backend.
    pub fn set_gpio_backend_socket<P: Into<PathBuf>>(&mut self, socket: P) {
        self.gpio_backend_socket = Some(socket.into());
    }

    /// Enable communication with the vhost-user i2c backend.
    pub fn set_i2c_backend_socket<P: Into<PathBuf>>(&mut self, socket: P) {
        self.i2c_backend_socket = Some(socket.into());
    }
}

impl Default for QemuConfig {
    fn default() -> Self {
        Self::new("arm".into(), 1024)
    }
}

/// Represents a running QEMU instance.
pub struct QemuInstance {
    pub process: Child,
    pub config: QemuConfig,
    qmp_client: QmpClient,
    cmd: Command,
}

impl QemuInstance {
    /// Spawn a new QEMU instance based on the provided configuration.
    pub fn spawn<FOut, FErr>(
        config: QemuConfig,
        on_stdout: FOut,
        on_stderr: FErr,
    ) -> Result<Self>
    where
        FOut: FnMut(String) + Send + 'static,
        FErr: FnMut(String) + Send + 'static,
    {
        // Create qemu command
        let qemu_binary = format!("qemu-system-{}", config.arch);
        let mut cmd = Command::new(qemu_binary);

        // Set memory size
        cmd.arg("-m").arg(config.memory_mb.to_string());
        
        // Append extra args
        for arg in &config.extra_args {
            match arg {
                QemuArg::Option { name, value } => {
                    cmd.arg(name);
                    cmd.arg(value);
                }
                QemuArg::Flag(flag) => {
                    cmd.arg(flag);
                }
            }
        }

        // Configure vhost-user shared memory backend file
        let mem_backend_file_arg = format!("memory-backend-file,id=mem,size={}M,mem-path=/dev/shm,share=on", config.memory_mb);
        cmd.arg("-object").arg(mem_backend_file_arg);
        cmd.arg("-numa").arg("node,memdev=mem");

        // Configure uart ports
        for (i, socket) in config.uart_sockets.iter().enumerate() {
            cmd.arg("-chardev").arg(format!("socket,path={},id=uart{},server=on,wait=off", socket.display(), i));
            cmd.arg("-serial").arg(format!("chardev:uart{}", i));
        }

        // Configure vhost-user gpio
        if let Some(socket) = &config.gpio_backend_socket {
            let chardev_arg = format!("socket,path={},id=vgpio", socket.display());
            cmd.arg("-chardev").arg(chardev_arg);
            cmd.arg("-device").arg("vhost-user-gpio-pci,chardev=vgpio,id=gpio");
        }

        // Configure vhost-user i2c
        if let Some(socket) = &config.i2c_backend_socket {
            let chardev_arg = format!("socket,path={},id=vi2c", socket.display());
            cmd.arg("-chardev").arg(chardev_arg);
            cmd.arg("-device").arg("vhost-user-i2c-pci,chardev=vi2c,id=i2c");
        }

        // Configure qmp
        let qmp_arg = format!("unix:{},server,nowait", config.qmp_socket_path.display());
        cmd.arg("-qmp").arg(qmp_arg);

        // Configure for non-interactive use
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Spawn qemu process
        let mut process = cmd.spawn().map_err(QemuError::ProcessSpawn)?;

        // stdout
        if let Some(stdout) = process.stdout.take() {
            let mut cb = on_stdout;
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().flatten() {
                    cb(line);
                }
            });
        }

        // stderr
        if let Some(stderr) = process.stderr.take() {
            let mut cb = on_stderr;
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    cb(line);
                }
            });
        }

        // Connect a QMP client to the socket
        let qmp_stream = Self::connect_stream(&config.qmp_socket_path, Duration::from_secs(30))?;
        let qmp_client = QmpClient::connect(qmp_stream, Some(Duration::from_secs(30)))?;

        Ok(QemuInstance {
            process,
            config,
            qmp_client,
            cmd,
        })
    }

    /// Connect the QMP socket to a stream with a timeout
    fn connect_stream(socket: &Path, timeout: Duration) -> Result<UnixStream> {
        let deadline = Instant::now() + timeout;

        loop {
            match UnixStream::connect(socket) {
                Ok(stream) => return Ok(stream),
                Err(err) => {
                    // Only retry if the socket path doesn't exist yet
                    if err.kind() != std::io::ErrorKind::NotFound {
                        return Err(QemuError::QmpConnect {
                            path: socket.to_path_buf(),
                            source: err
                        });
                    }

                    if Instant::now() >= deadline {
                        return Err(QemuError::QmpSocketTimeout);
                    }

                    sleep(Duration::from_millis(10));
                }
            }
        }
    }

    /// Sends a QMP command to the instance.
    pub fn qmp_command(&mut self, command: &str, args: Option<serde_json::Value>) -> Result<serde_json::Value> {
        self.qmp_client.execute(command, args)
    }

    /// Gracefully terminates the QEMU process via QMP.
    pub fn quit(&mut self) -> Result<()> {
        self.qmp_command("quit", None)?;
        Ok(())
    }

    /// Terminates the QEMU process forcefully.
    pub fn kill(&mut self) -> Result<()> {
        self.process.kill()?;
        Ok(())
    }

    /// Waits for the QEMU process to exit and returns its status.
    pub fn wait(&mut self) -> Result<std::process::ExitStatus> {
        self.process.wait().map_err(QemuError::Io)
    }

    /// Returns the command line used to spawn the instance.
    pub fn cmd_line(&self) -> String {
        let program = self.cmd.get_program().to_string_lossy();
        let args = self.cmd.get_args().map(|arg| {
            let s = arg.to_string_lossy();
            if s.contains(' ') || s.contains('"') {
                format!("{:?}", s) // Use debug escaping (e.g., wrap in quotes and escape as needed)
            } else {
                s.into_owned()
            }
        });

        std::iter::once(program.into_owned())
            .chain(args)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Drop for QemuInstance {
    /// Ensures the QEMU process is terminated when the instance goes out of scope.
    fn drop(&mut self) {
        // Attempt a graceful shutdown first, but ignore errors
        let _ = self.quit();
        // Ensure the process is killed
        let _ = self.kill();
        // Clean up the socket file
        let _ = std::fs::remove_file(&self.config.qmp_socket_path);
    }
}

/// A client for interacting with a QEMU instance via QMP.
struct QmpClient {
    stream: BufReader<UnixStream>,
    next_id: u64,
}

#[derive(Serialize)]
struct QmpCommand {
    execute: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<serde_json::Value>,
    id: u64,
}

#[derive(Deserialize, Debug)]
struct QmpCommandResponse {
    #[serde(rename = "return")]
    ret: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}


impl QmpClient {
    /// Connect to the QMP Unix socket and performs the initial capabilities negotiation.
    fn connect(stream: UnixStream, timeout: Option<Duration>) -> Result<Self> {
        stream.set_read_timeout(timeout)?;

        let mut client = Self {
            stream: BufReader::new(stream),
            next_id: 0,
        };

        // QMP negotiation
        let greeting_json = client.read_response()?;
        if greeting_json.get("QMP").is_none() {
            return Err(QemuError::QmpNegotiation);
        }

        // Do not enable any extra capabilities (e.g. oob)
        client.execute("qmp_capabilities", None)?;

        Ok(client)
    }

    /// Execute a QMP command and waits for the response.
    fn execute(&mut self, command: &str, arguments: Option<serde_json::Value>) -> Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;

        let qmp_cmd = QmpCommand {
            execute: command.to_string(),
            arguments,
            id,
        };

        let cmd_json = serde_json::to_string(&qmp_cmd)? + "\n";
        self.stream.get_mut().write_all(cmd_json.as_bytes())?;

        let response = self.read_cmd_response(id)?;

        if let Some(err) = response.error {
            return Err(QemuError::QmpResponse(err));
        }

        Ok(response.ret.unwrap_or(serde_json::Value::Null))
    }

    /// Wait for a response from QMP and return as JSON.
    fn read_response(&mut self) -> Result<serde_json::Value> {
        let mut response = String::new();
        match self.stream.read_line(&mut response) {
            Ok(0) => return Err(QemuError::QmpSocketDisconnected),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Err(QemuError::QmpSocketTimeout);
            }
            Err(e) => return Err(QemuError::Io(e)),
        }
        Ok(serde_json::from_str(&response)?)
    }

    /// Wait for a command response from QMP with matching ID.
    fn read_cmd_response(&mut self, expected_id: u64) -> Result<QmpCommandResponse> {
        loop {
            let value = self.read_response()?;

            // Ignore async events
            if value.get("event").is_some() {
                continue;
            }

            // Must contain an id
            let id = value.get("id")
                .and_then(|v| v.as_u64())
                .ok_or(QemuError::QmpResponseNoID)?;

            // Ignore non-matching IDs
            if id != expected_id {
                continue;
            }

            return Ok(serde_json::from_value(value)?);
        }
    }
}
