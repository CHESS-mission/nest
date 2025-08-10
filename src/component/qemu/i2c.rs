// vhost device i2c
//
// Copyright 2021 Linaro Ltd. All Rights Reserved.
//          Viresh Kumar <viresh.kumar@linaro.org>
//
// SPDX-License-Identifier: Apache-2.0 or BSD-3-Clause
//
// Modifications made in 2025 by Nathan Felber
// Adapted for use in NEST

use std::{convert, io, path::Path, sync::{Arc, RwLock}, thread::{spawn, JoinHandle}, time::Duration};

use crossbeam_channel::{unbounded, Receiver, Sender};
use thiserror::Error;
use vhost::vhost_user::{VhostUserProtocolFeatures, VhostUserVirtioFeatures};
use vhost_user_backend::{VhostUserBackendMut, VhostUserDaemon, VringRwLock, VringT};
use virtio_bindings::{
    virtio_config::{VIRTIO_F_NOTIFY_ON_EMPTY, VIRTIO_F_VERSION_1},
    virtio_ring::{VIRTIO_RING_F_EVENT_IDX, VIRTIO_RING_F_INDIRECT_DESC},
};
use virtio_queue::QueueOwnedT;
use vm_memory::{
    Bytes, GuestAddressSpace, GuestMemoryAtomic, GuestMemoryLoadGuard, GuestMemoryMmap,
};
use vmm_sys_util::{
    epoll::EventSet,
    eventfd::{EFD_NONBLOCK, EventFd},
};

use crate::component::qemu::i2c::{
    messaging::{I2cOperation, I2cRequest, I2cResponse},
    virtio::{
        I2cDescriptorChain, VIRTIO_I2C_F_ZERO_LENGTH_REQUEST, VIRTIO_I2C_FLAGS_M_RD,
        VIRTIO_I2C_MSG_ERR, VIRTIO_I2C_MSG_OK, VirtioI2cInHdr, VirtioI2cOutHdr,
    },
};

/// Custom Error type relative to the vhost device i2c backend.
#[derive(Error, Debug)]
pub enum I2cError {
    #[error("Failed to handle event, didn't match EPOLLIN")]
    HandleEventNotEpollIn,
    #[error("Failed to handle unknown device_event: {0}")]
    HandleEventUnknown(u16),
    #[error("Received unexpected write only descriptor at index {0}")]
    UnexpectedWriteOnlyDescriptor(usize),
    #[error("Received unexpected readable descriptor at index {0}")]
    UnexpectedReadableDescriptor(usize),
    #[error("Invalid descriptor count {0}")]
    UnexpectedDescriptorCount(usize),
    #[error("Invalid descriptor size, expected: {0}, found: {1}")]
    UnexpectedDescriptorSize(usize, u32),
    #[error("Descriptor not found")]
    DescriptorNotFound,
    #[error("Descriptor read failed")]
    DescriptorReadFailed,
    #[error("Descriptor write failed")]
    DescriptorWriteFailed,
    #[error("Couldn't return used descriptors to the ring")]
    DescriptorReturnFailed,
    #[error("Failed to send notification")]
    NotificationFailed,
    #[error("Failed to create new EventFd")]
    EventFdFailed,
    #[error("Couldn't send i2c message to the simulated device: {0}")]
    SendError(#[from] crossbeam_channel::SendError<I2cRequest>),
    #[error("Couldn't start the vhost-user i2c backend daemon")]
    StartBackendFailed,
}

type Result<T> = std::result::Result<T, I2cError>;

impl convert::From<I2cError> for io::Error {
    fn from(e: I2cError) -> Self {
        io::Error::other(e)
    }
}

/// Represents the vhost-user I2C backend.
pub struct VhostUserI2cBackend {
    event_idx: bool,
    pub exit_event: EventFd,
    mem: Option<GuestMemoryLoadGuard<GuestMemoryMmap>>,
    request_tx: Sender<I2cRequest>,
    response_rx: Receiver<I2cResponse>,
    timeout: Duration,
}

impl VhostUserI2cBackend {
    /// Create a new [VhostUserI2cBackend].
    pub fn new(request_tx: Sender<I2cRequest>, response_rx: Receiver<I2cResponse>, timeout: Duration) -> Result<Self> {
        Ok(Self {
            event_idx: false,
            exit_event: EventFd::new(EFD_NONBLOCK).map_err(|_| I2cError::EventFdFailed)?,
            mem: None,
            request_tx,
            response_rx,
            timeout,
        })
    }

    /// Process the requests in the vring and dispatch replies
    fn process_requests(
        &self,
        requests: Vec<I2cDescriptorChain>,
        vring: &VringRwLock,
    ) -> Result<bool> {
        let mut reqs: Vec<I2cRequest> = Vec::new();

        if requests.is_empty() {
            return Ok(true);
        }

        // Iterate over each I2C request and push it to "reqs" vector.
        for desc_chain in requests.clone() {
            let descriptors: Vec<_> = desc_chain.clone().collect();

            if (descriptors.len() != 2) && (descriptors.len() != 3) {
                return Err(I2cError::UnexpectedDescriptorCount(descriptors.len()));
            }

            let desc_out_hdr = descriptors[0];

            if desc_out_hdr.is_write_only() {
                return Err(I2cError::UnexpectedWriteOnlyDescriptor(0));
            }

            if desc_out_hdr.len() as usize != size_of::<VirtioI2cOutHdr>() {
                return Err(I2cError::UnexpectedDescriptorSize(
                    size_of::<VirtioI2cOutHdr>(),
                    desc_out_hdr.len(),
                ));
            }

            let out_hdr = desc_chain
                .memory()
                .read_obj::<VirtioI2cOutHdr>(desc_out_hdr.addr())
                .map_err(|_| I2cError::DescriptorReadFailed)?;

            // Is a read request
            let i2c_m_rd = (out_hdr.flags.to_native() & VIRTIO_I2C_FLAGS_M_RD) != 0;

            let desc_in_hdr = descriptors[descriptors.len() - 1];
            if !desc_in_hdr.is_write_only() {
                return Err(I2cError::UnexpectedReadableDescriptor(descriptors.len() - 1));
            }

            if desc_in_hdr.len() as usize != size_of::<u8>() {
                return Err(I2cError::UnexpectedDescriptorSize(
                    size_of::<u8>(),
                    desc_in_hdr.len(),
                ));
            }

            let (buf, len) = match descriptors.len() {
                // Buffer is available
                3 => {
                    let desc_buf = descriptors[1];
                    let len = desc_buf.len();

                    if len == 0 {
                        return Err(I2cError::UnexpectedDescriptorSize(1, len));
                    }
                    let mut buf = vec![0; len as usize];

                    if !i2c_m_rd {
                        if desc_buf.is_write_only() {
                            return Err(I2cError::UnexpectedWriteOnlyDescriptor(1));
                        }

                        desc_chain
                            .memory()
                            .read(&mut buf, desc_buf.addr())
                            .map_err(|_| I2cError::DescriptorReadFailed)?;
                    } else if !desc_buf.is_write_only() {
                        return Err(I2cError::UnexpectedReadableDescriptor(1));
                    }

                    (buf, len)
                }

                _ => (Vec::<u8>::new(), 0),
            };

            reqs.push(I2cRequest {
                addr: out_hdr.addr.to_native() >> 1,
                op: match i2c_m_rd {
                    true => I2cOperation::Read(len as usize),
                    false => I2cOperation::Write(buf),
                }
            });
        }

        // Send requests to simulated device and handle responses
        let mut status = VIRTIO_I2C_MSG_OK;
        let mut responses = Vec::new();
        for req in reqs {
            self.request_tx.send(req).map_err(I2cError::from)?;
            let response = self.response_rx.recv_timeout(self.timeout);
            if response.is_err() {
                responses.push(response);
                status = VIRTIO_I2C_MSG_ERR;
                break;
            }
            responses.push(response);
        }

        let in_hdr = VirtioI2cInHdr { status };

        for (i, desc_chain) in requests.iter().enumerate() {
            let descriptors: Vec<_> = desc_chain.clone().collect();
            let desc_in_hdr = descriptors[descriptors.len() - 1];
            let mut len = size_of::<VirtioI2cInHdr>() as u32;

            if descriptors.len() == 3 {
                let desc_buf = descriptors[1];

                // Write the data read from the I2C device
                if let Ok(I2cResponse::Read(buf)) = &responses[i] {
                    desc_chain
                        .memory()
                        .write(buf, desc_buf.addr())
                        .map_err(|_| I2cError::DescriptorWriteFailed)?;
                }

                if in_hdr.status == VIRTIO_I2C_MSG_OK {
                    len += desc_buf.len();
                }
            }

            // Write the transfer status
            desc_chain
                .memory()
                .write_obj::<VirtioI2cInHdr>(in_hdr, desc_in_hdr.addr())
                .map_err(|_| I2cError::DescriptorWriteFailed)?;

            if vring.add_used(desc_chain.head_index(), len).is_err() {
                return Err(I2cError::DescriptorReturnFailed);
            }
        }

        Ok(true)
    }

    /// Process the requests in the vring and dispatch replies
    fn process_queue(&self, vring: &VringRwLock) -> Result<()> {
        let requests: Vec<_> = vring
            .get_mut()
            .get_queue_mut()
            .iter(self.mem.as_ref().unwrap().clone())
            .map_err(|_| I2cError::DescriptorNotFound)?
            .collect();

        if self.process_requests(requests, vring)? {
            // Send notification once all the requests are processed
            vring.signal_used_queue().map_err(|_| I2cError::NotificationFailed)?;
        }

        Ok(())
    }
}

/// VhostUserBackendMut trait methods
impl VhostUserBackendMut for VhostUserI2cBackend {
    type Vring = VringRwLock;
    type Bitmap = ();

    fn num_queues(&self) -> usize {
        // Not sure what the effect is, leaving 1.
        1
    }

    fn max_queue_size(&self) -> usize {
        // Not sure what the effect is, 1024 should be reasonable.
        1024
    }

    fn features(&self) -> u64 {
        // this matches the current libvhost defaults except VHOST_F_LOG_ALL
        1 << VIRTIO_F_VERSION_1
            | 1 << VIRTIO_F_NOTIFY_ON_EMPTY
            | 1 << VIRTIO_RING_F_INDIRECT_DESC
            | 1 << VIRTIO_RING_F_EVENT_IDX
            | 1 << VIRTIO_I2C_F_ZERO_LENGTH_REQUEST
            | VhostUserVirtioFeatures::PROTOCOL_FEATURES.bits()
    }

    fn protocol_features(&self) -> VhostUserProtocolFeatures {
        VhostUserProtocolFeatures::MQ
    }

    fn set_event_idx(&mut self, enabled: bool) {
        self.event_idx = enabled;
    }

    fn update_memory(&mut self, mem: GuestMemoryAtomic<GuestMemoryMmap>) -> io::Result<()> {
        self.mem = Some(mem.memory());
        Ok(())
    }

    fn handle_event(
        &mut self,
        device_event: u16,
        evset: EventSet,
        vrings: &[VringRwLock],
        _thread_id: usize,
    ) -> io::Result<()> {
        if evset != EventSet::IN {
            return Err(I2cError::HandleEventNotEpollIn.into());
        }

        match device_event {
            0 => {
                let vring = &vrings[0];

                if self.event_idx {
                    // vm-virtio's Queue implementation only checks avail_index
                    // once, so to properly support EVENT_IDX we need to keep
                    // calling process_queue() until it stops finding new
                    // requests on the queue.
                    loop {
                        vring.disable_notification().unwrap();
                        self.process_queue(vring)?;
                        if !vring.enable_notification().unwrap() {
                            break;
                        }
                    }
                } else {
                    // Without EVENT_IDX, a single call is enough.
                    self.process_queue(vring)?;
                }
            }

            _ => {
                return Err(I2cError::HandleEventUnknown(device_event).into());
            }
        }
        Ok(())
    }

    fn exit_event(&self, _thread_index: usize) -> Option<EventFd> {
        self.exit_event.try_clone().ok()
    }
}

/// A handle to the communication channels for the simulated I2C device.
pub struct I2cBackendHandle {
    /// Handle of the backend thread.
    pub join_handle: JoinHandle<Result<()>>,
    /// Receives requests from the guest.
    pub request_rx: Receiver<I2cRequest>,
    /// Sends responses back to the guest.
    pub response_tx: Sender<I2cResponse>,
}

/// Start the vhost-user i2c backend daemon.
pub fn start_i2c_backend<P: AsRef<Path>>(i2c_socket: P, timeout: Duration) -> I2cBackendHandle {
    let i2c_socket = i2c_socket.as_ref().to_owned();
    let (request_tx, request_rx) = unbounded();
    let (response_tx, response_rx) = unbounded();

    let join_handle = spawn(move || {
        let backend = Arc::new(RwLock::new(
            VhostUserI2cBackend::new(request_tx, response_rx, timeout)?,
        ));

        let mut daemon = VhostUserDaemon::new(
            String::from("nest-vhost-i2c-backend"),
            backend.clone(),
            GuestMemoryAtomic::new(GuestMemoryMmap::new()),
        ).map_err(|_| I2cError::StartBackendFailed)?;

        daemon.serve(i2c_socket).map_err(|_| I2cError::StartBackendFailed)?;

        Ok(())
    });

    I2cBackendHandle {
        join_handle,
        request_rx,
        response_tx
    }
}

// Sub-module for messaging between the backend and the simulated device
pub mod messaging {
    use serde::{Deserialize, Serialize};

    /// An I2C operation, part of a transfer.
    #[derive(Debug, Clone)]
    #[derive(Serialize, Deserialize)]
    pub enum I2cOperation {
        /// A write operation, containing the data to be written.
        Write(Vec<u8>),
        /// A read operation, indicating the number of bytes to read.
        Read(usize),
    }

    /// A request sent from the [VhostUserI2cBackend] to the simulated device.
    #[derive(Debug, Clone)]
    #[derive(Serialize, Deserialize)]
    pub struct I2cRequest {
        /// The 7-bit I2C device address.
        pub addr: u16,
        /// The operation of this transaction.
        pub op: I2cOperation,
    }

    impl I2cRequest {
        pub fn is_read(&self) -> bool {
            matches!(self.op, I2cOperation::Read(_))
        }
    }

    /// A response sent from the simulated device back to the [VhostUserI2cBackend].
    #[derive(Debug, Clone)]
    #[derive(Serialize, Deserialize)]
    pub enum I2cResponse {
        /// A write response, acknowledging that the data has been written.
        Write,
        /// A read response, with the content read from the device.
        Read(Vec<u8>),
    }
}

// Sub-module for virtio specific structs and constants
pub mod virtio {
    use virtio_queue::DescriptorChain;
    use vm_memory::{ByteValued, GuestMemoryLoadGuard, GuestMemoryMmap, Le16, Le32};

    /// Virtio I2C Feature bits
    pub const VIRTIO_I2C_F_ZERO_LENGTH_REQUEST: u16 = 0;

    /// The final status written by the device
    pub const VIRTIO_I2C_MSG_OK: u8 = 0;
    pub const VIRTIO_I2C_MSG_ERR: u8 = 1;

    #[derive(Copy, Clone, Default)]
    #[repr(C)]
    pub struct VirtioI2cOutHdr {
        pub addr: Le16,
        pub padding: Le16,
        pub flags: Le32,
    }
    // SAFETY: The layout of the structure is fixed and can be initialized by
    // reading its content from byte array.
    unsafe impl ByteValued for VirtioI2cOutHdr {}

    /// VirtioI2cOutHdr Flags
    pub const VIRTIO_I2C_FLAGS_M_RD: u32 = 1 << 1;

    #[derive(Copy, Clone, Default)]
    #[repr(C)]
    pub struct VirtioI2cInHdr {
        pub status: u8,
    }
    // SAFETY: The layout of the structure is fixed and can be initialized by
    // reading its content from byte array.
    unsafe impl ByteValued for VirtioI2cInHdr {}

    pub type I2cDescriptorChain = DescriptorChain<GuestMemoryLoadGuard<GuestMemoryMmap<()>>>;
}

