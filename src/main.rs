mod i2c;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread::{spawn, JoinHandle};

use vhost_user_backend::VhostUserDaemon;
use vm_memory::{GuestMemoryAtomic, GuestMemoryMmap};

use i2c::VhostUserI2cBackend;

pub fn generate_socket_paths(path: PathBuf) -> PathBuf {
    let socket_file_name = path
        .file_name()
        .expect("socket_path has no filename.");
    let socket_file_parent = path
        .parent()
        .expect("socket_path has no parent directory.");

    let make_socket_path = |i: usize| -> PathBuf {
        let mut file_name = socket_file_name.to_os_string();
        file_name.push(std::ffi::OsStr::new(&i.to_string()));
        socket_file_parent.join(&file_name)
    };
    make_socket_path(0)
}

fn start_backend() {
    let socket = generate_socket_paths("/tmp/vi2c.sock".into());
    let handle: JoinHandle<()> = spawn(move || loop {
        let backend = Arc::new(RwLock::new(
            VhostUserI2cBackend::new().unwrap(),
        ));

        let mut daemon = VhostUserDaemon::new(
            String::from("vhost-device-i2c-backend"),
            backend.clone(),
            GuestMemoryAtomic::new(GuestMemoryMmap::new()),
        ).unwrap();

        let _ = daemon.serve(&socket);
    });

    handle.join().map_err(std::panic::resume_unwind).unwrap();
}

fn main() {
    start_backend();
}

// use extism_convert::{Json, ToBytes};
// use serde::Serialize;
//
// #[derive(ToBytes, Serialize)]
// #[encoding(Json)]
// struct Struct {
//     hello: String,
//     len: u32,
//     buf: Vec<u8>
// }
//
// fn main() {
//     let msg = Struct {
//         hello: "hi".into(),
//         len: 7,
//         buf: vec![0, 1, 2, 3, 4, 5, 6]
//     };
//     println!("{}", String::from_utf8(msg.to_bytes().unwrap()).unwrap());
// }
