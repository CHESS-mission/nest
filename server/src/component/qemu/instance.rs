use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};
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
    QmpConnect {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Timed out waiting for QMP socket to become available")]
    QmpSocketTimeout,
    #[error("QMP negotiation failed")]
    QmpNegotiation,
    #[error("QMP response indicates an error: {0:?}")]
    QmpResponse(Value),
}

type Result<T> = std::result::Result<T, QemuError>;

/// Represents a QEMU command line argument.
#[derive(Debug, Clone)]
enum QemuArg {
    Option { name: String, value: String },
    Flag(String),
}

/// Represents the configuration for a QEMU instance.
#[derive(Debug, Clone)]
pub struct QemuConfig {
    pub arch: String,
    pub memory_mb: u32,
    extra_args: Vec<QemuArg>,
    gpio_backend_socket: Option<PathBuf>,
    i2c_backend_socket: Option<PathBuf>,
    qmp_socket_path: PathBuf,
}

impl QemuConfig {
    /// Create a new configuration.
    pub fn new(arch: String, memory_mb: u32) -> Self {
        let qmp_socket_path =
            std::env::temp_dir().join(format!("qmp-{}.sock", uuid::Uuid::new_v4()));

        Self {
            arch,
            memory_mb,
            extra_args: Vec::new(),
            gpio_backend_socket: None,
            i2c_backend_socket: None,
            qmp_socket_path,
        }
    }

    /// Add a command-line option.
    pub fn add_option<N: Into<String>, V: Into<String>>(&mut self, name: N, value: V) {
        self.extra_args.push(QemuArg::Option {
            name: name.into(),
            value: value.into(),
        });
    }

    /// Add a command-line flag.
    pub fn add_flag<S: Into<String>>(&mut self, flag: S) {
        self.extra_args.push(QemuArg::Flag(flag.into()));
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
    pub fn spawn(config: QemuConfig) -> Result<Self> {
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
        let mem_backend_file_arg = format!(
            "memory-backend-file,id=mem,size={}M,mem-path=/dev/shm,share=on",
            config.memory_mb
        );
        cmd.arg("-object").arg(mem_backend_file_arg);
        cmd.arg("-numa").arg("node,memdev=mem");

        // Configure vhost-user gpio
        if let Some(socket) = &config.gpio_backend_socket {
            let chardev_arg = format!("socket,path={},id=vgpio", socket.display());
            cmd.arg("-chardev").arg(chardev_arg);
            cmd.arg("-device")
                .arg("vhost-user-gpio-pci,chardev=vgpio,id=gpio");
        }

        // Configure vhost-user i2c
        if let Some(socket) = &config.i2c_backend_socket {
            let chardev_arg = format!("socket,path={},id=vi2c", socket.display());
            cmd.arg("-chardev").arg(chardev_arg);
            cmd.arg("-device")
                .arg("vhost-user-i2c-pci,chardev=vi2c,id=i2c");
        }

        // Configure qmp
        let qmp_arg = format!("unix:{},server,nowait", config.qmp_socket_path.display());
        cmd.arg("-qmp").arg(qmp_arg);

        // Configure for non-interactive use
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Spawn qemu process
        let process = cmd.spawn().map_err(QemuError::ProcessSpawn)?;

        // Connect a QMP client to the socket
        let qmp_client = Self::connect_qmp_client(&config.qmp_socket_path, Duration::from_secs(1))?;

        Ok(QemuInstance {
            process,
            config,
            qmp_client,
            cmd,
        })
    }

    /// Connect the QMP client to the QEMU instance with a timeout
    fn connect_qmp_client(socket: &Path, timeout: Duration) -> Result<QmpClient> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            match QmpClient::connect(socket) {
                Ok(client) => return Ok(client),
                Err(_) => sleep(Duration::from_millis(10)),
            }
        }
        Err(QemuError::QmpSocketTimeout)
    }

    /// Sends a QMP command to the instance.
    pub fn qmp_command(&mut self, command: &str, args: Option<Value>) -> Result<Value> {
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
}

#[derive(Serialize)]
struct QmpCommand {
    execute: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<Value>,
}

#[derive(Deserialize, Debug)]
struct QmpResponse {
    #[serde(rename = "return")]
    ret: Option<Value>,
    error: Option<Value>,
}

impl QmpClient {
    /// Connect to the QMP Unix socket and performs the initial capabilities negotiation.
    fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path).map_err(|e| QemuError::QmpConnect {
            path: path.to_path_buf(),
            source: e,
        })?;
        let mut client = Self {
            stream: BufReader::new(stream),
        };

        // QMP negotiation
        let mut greeting = String::new();
        client.stream.read_line(&mut greeting)?;
        let greeting_json: Value = serde_json::from_str(&greeting)?;
        if greeting_json.get("QMP").is_none() {
            return Err(QemuError::QmpNegotiation);
        }

        // Do not enable any extra capabilities (e.g. oob)
        client.execute("qmp_capabilities", None)?;

        Ok(client)
    }

    /// Execute a QMP command and waits for the response.
    fn execute(&mut self, command: &str, arguments: Option<Value>) -> Result<Value> {
        let qmp_cmd = QmpCommand {
            execute: command.to_string(),
            arguments,
        };

        let cmd_json = serde_json::to_string(&qmp_cmd)? + "\n";
        self.stream.get_mut().write_all(cmd_json.as_bytes())?;

        let mut response_str = String::new();
        self.stream.read_line(&mut response_str)?;

        let response: QmpResponse = serde_json::from_str(&response_str)?;

        if let Some(err) = response.error {
            return Err(QemuError::QmpResponse(err));
        }

        Ok(response.ret.unwrap_or(Value::Null))
    }
}
