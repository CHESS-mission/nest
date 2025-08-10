// vhost device gpio
//
// Copyright 2022 Linaro Ltd. All Rights Reserved.
//          Viresh Kumar <viresh.kumar@linaro.org>
//
// SPDX-License-Identifier: Apache-2.0 or BSD-3-Clause
//
// Modifications made in 2025 by Nathan Felber
// Adapted for use in NEST

use std::{
    convert,
    io::{self, Result as IoResult},
    mem::size_of,
    path::Path,
    slice::from_raw_parts,
    sync::{Arc, Mutex, RwLock},
    thread::{JoinHandle, spawn},
    usize,
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use thiserror::Error as ThisError;
use vhost::vhost_user::message::{VhostUserProtocolFeatures, VhostUserVirtioFeatures};
use vhost_user_backend::{VhostUserBackendMut, VhostUserDaemon, VringRwLock, VringT};
use virtio_bindings::bindings::{
    virtio_config::{VIRTIO_F_NOTIFY_ON_EMPTY, VIRTIO_F_VERSION_1},
    virtio_ring::{VIRTIO_RING_F_EVENT_IDX, VIRTIO_RING_F_INDIRECT_DESC},
};
use virtio_queue::{DescriptorChain, QueueOwnedT};
use vm_memory::{
    Bytes, GuestAddress, GuestAddressSpace, GuestMemoryAtomic, GuestMemoryLoadGuard,
    GuestMemoryMmap,
};
use vmm_sys_util::{
    epoll::EventSet,
    eventfd::{EFD_NONBLOCK, EventFd},
};

use crate::component::qemu::gpio::{messaging::*, virtio::*};

type Result<T> = std::result::Result<T, GpioError>;

#[derive(Clone, Debug, ThisError)]
/// Errors related to vhost-device-gpio-daemon.
pub enum GpioError {
    #[error("Failed to handle event, didn't match EPOLLIN")]
    HandleEventNotEpollIn,
    #[error("Failed to handle unknown event")]
    HandleEventUnknown,
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
    #[error("Failed to send notification")]
    NotificationFailed,
    #[error("Failed to signal used queue")]
    UsedQueueFailed,
    #[error("Failed to create new EventFd")]
    EventFdFailed,
    #[error("Invalid gpio irq type: {0}")]
    GpioIrqTypeInvalid(u8),
    #[error("Invalid gpio direction: {0}")]
    GpioDirectionInvalid(u32),
    #[error("Invalid current gpio value")]
    GpioCurrentValueInvalid,
    #[error("Invalid gpio value: {0}")]
    GpioValueInvalid(u32),
    #[error("Invalid gpio message type: {0}")]
    GpioMessageInvalid(u16),
    #[error("Invalid gpio line: {0}")]
    GpioLineInvalid(u16),
    #[error("Couldn't send gpio event to the simulated device: {0}")]
    SendError(#[from] crossbeam_channel::SendError<GpioEvent>),
    #[error("Couldn't receive gpio event from the simulated device: {0}")]
    ReceiveError(#[from] crossbeam_channel::RecvError),
    #[error("Couldn't start the vhost-user gpio backend daemon")]
    StartBackendFailed,
}

impl convert::From<GpioError> for io::Error {
    fn from(e: GpioError) -> Self {
        io::Error::other(e)
    }
}

#[derive(Clone)]
struct IrqReservation {
    vring: VringRwLock,
    desc_chain: GpioDescriptorChain,
    addr: GuestAddress,
}

/// Represents a GPIO line
#[derive(Clone)]
pub struct GpioLine {
    /// The optional name of the line
    pub name: Option<String>,
    direction: u8,
    value: u8,
    irq_type: u8,
    irq_latched: bool,
    irq_reservation: Option<IrqReservation>
}

impl GpioLine {
    pub fn new(name: Option<String>) -> Self {
        Self {
            name,
            direction: VIRTIO_GPIO_DIRECTION_NONE,
            value: VIRTIO_GPIO_VALUE_LOW,
            irq_type: VIRTIO_GPIO_IRQ_TYPE_NONE,
            irq_latched: false,
            irq_reservation: None,
        }
    }
}

impl Default for GpioLine {
    fn default() -> Self {
        Self::new(None)
    }
}

pub struct VhostUserGpioBackend {
    #[allow(dead_code)]
    events_handle: JoinHandle<Result<()>>,
    event_idx: bool,
    pub exit_event: EventFd,
    mem: Option<GuestMemoryAtomic<GuestMemoryMmap>>,
    events_tx: Sender<GpioEvent>,
    config: VirtioGpioConfig,
    gpio_lines: Arc<Mutex<Vec<GpioLine>>>,
    gpio_names: String,
}

type GpioDescriptorChain = DescriptorChain<GuestMemoryLoadGuard<GuestMemoryMmap<()>>>;

impl VhostUserGpioBackend {
    /// Create a new [VhostUserGpioBackend].
    pub fn new(
        events_tx: Sender<GpioEvent>,
        events_rx: Receiver<GpioEvent>,
        gpio_lines: Vec<GpioLine>,
    ) -> Result<Self> {
        let ngpio = gpio_lines.len();
        let gpio_names = gpio_lines.iter()
            .map(|line| line.name.clone().unwrap_or_default() + "\0")
            .collect::<String>();

        let config = VirtioGpioConfig {
            ngpio: From::from(ngpio as u16),
            padding: From::from(0),
            gpio_names_size: From::from(gpio_names.len() as u32),
        };

        let gpio_lines = Arc::new(Mutex::new(gpio_lines));
        
        let events_handle = spawn({
            let gpio_lines = gpio_lines.clone();
            move || -> Result<()> {
                loop {
                    let event = events_rx.recv()?;

                    let (line, value) = match event {
                        GpioEvent::ValueLow(line) => (line, VIRTIO_GPIO_VALUE_LOW),
                        GpioEvent::ValueHigh(line) => (line, VIRTIO_GPIO_VALUE_HIGH),
                    };

                    // Access gpio_lines
                    {
                        let mut gpio_lines = gpio_lines.lock().unwrap();
                        let gpio_line = gpio_lines.get_mut(line as usize)
                            .ok_or_else(|| GpioError::GpioLineInvalid(line))?;
                        
                        // Check for matching interrupt
                        let irq_triggered = match gpio_line.irq_type {
                            VIRTIO_GPIO_IRQ_TYPE_NONE => false,
                            VIRTIO_GPIO_IRQ_TYPE_EDGE_RISING => gpio_line.value == VIRTIO_GPIO_VALUE_LOW && value == VIRTIO_GPIO_VALUE_HIGH,
                            VIRTIO_GPIO_IRQ_TYPE_EDGE_FALLING => gpio_line.value == VIRTIO_GPIO_VALUE_HIGH && value == VIRTIO_GPIO_VALUE_LOW,
                            VIRTIO_GPIO_IRQ_TYPE_EDGE_BOTH => gpio_line.value != value,
                            VIRTIO_GPIO_IRQ_TYPE_LEVEL_HIGH => value == VIRTIO_GPIO_VALUE_HIGH,
                            VIRTIO_GPIO_IRQ_TYPE_LEVEL_LOW => value == VIRTIO_GPIO_VALUE_LOW,
                            _ => return Err(GpioError::GpioIrqTypeInvalid(gpio_line.irq_type)),
                        };

                        if irq_triggered {
                            if let Some(res) = &gpio_line.irq_reservation {
                                Self::send_event_response(&res.vring, &res.desc_chain, res.addr, VIRTIO_GPIO_IRQ_STATUS_VALID)?;
                                gpio_line.irq_reservation = None;
                                gpio_line.irq_latched = false;
                            } else if gpio_line.irq_type == VIRTIO_GPIO_IRQ_TYPE_EDGE_RISING ||
                                      gpio_line.irq_type == VIRTIO_GPIO_IRQ_TYPE_EDGE_FALLING ||
                                      gpio_line.irq_type == VIRTIO_GPIO_IRQ_TYPE_EDGE_BOTH {
                                // Latch edge interrupts
                                gpio_line.irq_latched = true;
                            }
                        }

                        // Update value
                        gpio_line.value = value;
                    }
                }
            }
        });

        Ok(Self {
            events_handle,
            event_idx: false,
            exit_event: EventFd::new(EFD_NONBLOCK).map_err(|_| GpioError::EventFdFailed)?,
            mem: None,
            events_tx,
            config,
            gpio_lines,
            gpio_names,
        })
    }

    /// Process the requests in the request queue
    fn process_requests(
        &mut self,
        requests: Vec<GpioDescriptorChain>,
        vring: &VringRwLock,
    ) -> Result<bool> {
        if requests.is_empty() {
            return Ok(true);
        }

        for desc_chain in requests {
            let descriptors: Vec<_> = desc_chain.clone().collect();
            if descriptors.len() != 2 {
                return Err(GpioError::UnexpectedDescriptorCount(descriptors.len()));
            }

            let desc_request = descriptors[0];
            if desc_request.is_write_only() {
                return Err(GpioError::UnexpectedWriteOnlyDescriptor(0));
            }

            if desc_request.len() as usize != size_of::<VirtioGpioRequest>() {
                return Err(GpioError::UnexpectedDescriptorSize(
                    size_of::<VirtioGpioRequest>(),
                    desc_request.len(),
                ));
            }

            let request = desc_chain
                .memory()
                .read_obj::<VirtioGpioRequest>(desc_request.addr())
                .map_err(|_| GpioError::DescriptorReadFailed)?;

            let desc_response = descriptors[1];
            if !desc_response.is_write_only() {
                return Err(GpioError::UnexpectedReadableDescriptor(1));
            }

            let response_data = {
                // Access gpio_lines
                let mut gpio_lines = self.gpio_lines.lock().unwrap();
                let gpio_line = gpio_lines.get_mut(request.gpio.to_native() as usize)
                    .ok_or_else(|| GpioError::GpioLineInvalid(request.gpio.to_native()))?;

                match request.rtype.to_native() {
                    VIRTIO_GPIO_MSG_GET_LINE_NAMES => Ok(self.gpio_names.as_bytes().to_vec()),
                    VIRTIO_GPIO_MSG_GET_DIRECTION => Ok(vec![gpio_line.direction]),
                    VIRTIO_GPIO_MSG_SET_DIRECTION => {
                        gpio_line.direction = request.value.to_native() as u8;
                        Ok(vec![0])
                    }
                    VIRTIO_GPIO_MSG_GET_VALUE => Ok(vec![gpio_line.value]),
                    VIRTIO_GPIO_MSG_SET_VALUE => {
                        let event = match request.value.to_native() as u8 {
                            VIRTIO_GPIO_VALUE_LOW => GpioEvent::ValueLow(request.gpio.to_native()),
                            VIRTIO_GPIO_VALUE_HIGH => GpioEvent::ValueHigh(request.gpio.to_native()),
                            _ => return Err(GpioError::GpioValueInvalid(request.value.to_native())),
                        };
                        gpio_line.value = request.value.to_native() as u8;
                        self.events_tx.send(event)?;
                        Ok(vec![0])
                    }
                    VIRTIO_GPIO_MSG_SET_IRQ_TYPE => {
                        let irq_type = request.value.to_native() as u8;
                        if irq_type == VIRTIO_GPIO_IRQ_TYPE_NONE {
                            if let Some(res) = &gpio_line.irq_reservation {
                                // Cancel reservation
                                Self::send_event_response(&res.vring, &res.desc_chain, res.addr, VIRTIO_GPIO_IRQ_STATUS_INVALID)?;
                                gpio_line.irq_reservation = None;
                            }
                        }
                        gpio_line.irq_type = irq_type;
                        gpio_line.irq_latched = false;
                        Ok(vec![0])
                    }
                    msg => Err(GpioError::GpioMessageInvalid(msg)),
                }
            };

            let response = match response_data {
                Ok(mut data) => {
                    if data.len() != (desc_response.len() - 1) as usize {
                        vec![VIRTIO_GPIO_STATUS_ERR, 0]
                    } else {
                        let mut buf = vec![VIRTIO_GPIO_STATUS_OK];
                        buf.append(&mut data);
                        buf
                    }
                }

                Err(_) => {
                    vec![VIRTIO_GPIO_STATUS_ERR, 0]
                }
            };

            desc_chain
                .memory()
                .write_slice(response.as_slice(), desc_response.addr())
                .map_err(|_| GpioError::DescriptorWriteFailed)?;

            if vring
                .add_used(desc_chain.head_index(), desc_response.len())
                .is_err()
            {
                return Err(GpioError::UsedQueueFailed);
            }
        }

        Ok(true)
    }

    /// Process the messages in the vring and dispatch replies
    fn process_request_queue(&mut self, vring: &VringRwLock) -> Result<()> {
        let requests: Vec<_> = vring
            .get_mut()
            .get_queue_mut()
            .iter(self.mem.as_ref().unwrap().memory())
            .map_err(|_| GpioError::DescriptorNotFound)?
            .collect();

        if self.process_requests(requests, vring)? {
            // Send notification once all the requests are processed
            vring
                .signal_used_queue()
                .map_err(|_| GpioError::NotificationFailed)?;
        }

        Ok(())
    }

    /// Process the request in the event queue
    fn process_event(
        &mut self,
        vring: &VringRwLock,
        desc_chain: GpioDescriptorChain,
        gpio: u16,
        addr: GuestAddress,
    ) -> Result<()> {
        let mut gpio_lines = self.gpio_lines.lock().unwrap();
        let gpio_line = gpio_lines.get_mut(gpio as usize)
            .ok_or_else(|| GpioError::GpioLineInvalid(gpio))?;

        // Interrupt should be enabled before sending buffer and no other buffer
        // should have been received earlier for this GPIO pin.
        if gpio_line.irq_type == VIRTIO_GPIO_IRQ_TYPE_NONE || gpio_line.irq_reservation.is_some() {
            // Invalid
            Self::send_event_response(vring, &desc_chain, addr, VIRTIO_GPIO_IRQ_STATUS_INVALID)?;
        } else if gpio_line.irq_latched ||
                  gpio_line.irq_type == VIRTIO_GPIO_IRQ_TYPE_LEVEL_HIGH && gpio_line.value == VIRTIO_GPIO_VALUE_HIGH ||
                  gpio_line.irq_type == VIRTIO_GPIO_IRQ_TYPE_LEVEL_LOW && gpio_line.value == VIRTIO_GPIO_VALUE_LOW
        {
            // Answer directly
            Self::send_event_response(vring, &desc_chain, addr, VIRTIO_GPIO_IRQ_STATUS_VALID)?;
            gpio_line.irq_latched = false;
        } else {
            // Make a reservation
            gpio_line.irq_reservation = Some(IrqReservation { vring: vring.clone(), desc_chain, addr })
        }

        Ok(())
    }

    /// Process the requests in the event queue
    fn process_events(
        &mut self,
        requests: Vec<GpioDescriptorChain>,
        vring: &VringRwLock,
    ) -> Result<()> {
        if requests.is_empty() {
            return Ok(());
        }

        for desc_chain in requests {
            let descriptors: Vec<_> = desc_chain.clone().collect();
            if descriptors.len() != 2 {
                return Err(GpioError::UnexpectedDescriptorCount(descriptors.len()));
            }

            let desc_request = descriptors[0];
            if desc_request.is_write_only() {
                return Err(GpioError::UnexpectedWriteOnlyDescriptor(0));
            }

            if desc_request.len() as usize != size_of::<VirtioGpioIrqRequest>() {
                return Err(GpioError::UnexpectedDescriptorSize(
                    size_of::<VirtioGpioIrqRequest>(),
                    desc_request.len(),
                ));
            }

            let request = desc_chain
                .memory()
                .read_obj::<VirtioGpioIrqRequest>(desc_request.addr())
                .map_err(|_| GpioError::DescriptorReadFailed)?;

            let desc_response = descriptors[1];
            if !desc_response.is_write_only() {
                return Err(GpioError::UnexpectedReadableDescriptor(1));
            }

            if desc_response.len() as usize != size_of::<VirtioGpioIrqResponse>() {
                return Err(GpioError::UnexpectedDescriptorSize(
                    size_of::<VirtioGpioIrqResponse>(),
                    desc_response.len(),
                ));
            }

            self.process_event(
                vring,
                desc_chain,
                request.gpio.to_native(),
                desc_response.addr(),
            )?;
        }

        Ok(())
    }

    /// Process the messages in the vring and dispatch replies
    fn process_event_queue(&mut self, vring: &VringRwLock) -> Result<()> {
        let requests: Vec<_> = vring
            .get_mut()
            .get_queue_mut()
            .iter(self.mem.as_ref().unwrap().memory())
            .map_err(|_| GpioError::DescriptorNotFound)?
            .collect();

        self.process_events(requests, vring)?;
        Ok(())
    }

    /// Send response over the eventq virtqueue.
    fn send_event_response(
        vring: &VringRwLock,
        desc_chain: &GpioDescriptorChain,
        addr: GuestAddress,
        status: u8,
    ) -> Result<()> {
        let response = VirtioGpioIrqResponse { status };

        // Write the response to guest memory.
        if desc_chain
            .memory()
            .write_obj::<VirtioGpioIrqResponse>(response, addr)
            .is_err()
        {
            return Err(GpioError::DescriptorWriteFailed);
        }

        // Return the descriptor to the used ring.
        if vring
            .add_used(
                desc_chain.head_index(),
                size_of::<VirtioGpioIrqResponse>() as u32,
            )
            .is_err()
        {
            return Err(GpioError::UsedQueueFailed);
        }

        // Notify the guest that the descriptor is ready.
        if vring.signal_used_queue().is_err() {
            return Err(GpioError::NotificationFailed);
        }

        Ok(())
    }
}

/// VhostUserBackendMut trait methods
impl VhostUserBackendMut for VhostUserGpioBackend {
    type Vring = VringRwLock;
    type Bitmap = ();

    fn num_queues(&self) -> usize {
        NUM_QUEUES
    }

    fn max_queue_size(&self) -> usize {
        QUEUE_SIZE
    }

    fn features(&self) -> u64 {
        // this matches the current libvhost defaults except VHOST_F_LOG_ALL
        (1 << VIRTIO_F_VERSION_1)
            | (1 << VIRTIO_F_NOTIFY_ON_EMPTY)
            | (1 << VIRTIO_RING_F_INDIRECT_DESC)
            | (1 << VIRTIO_RING_F_EVENT_IDX)
            | (1 << VIRTIO_GPIO_F_IRQ)
            | VhostUserVirtioFeatures::PROTOCOL_FEATURES.bits()
    }

    fn protocol_features(&self) -> VhostUserProtocolFeatures {
        VhostUserProtocolFeatures::MQ
            | VhostUserProtocolFeatures::CONFIG
            | VhostUserProtocolFeatures::REPLY_ACK
    }

    fn get_config(&self, offset: u32, size: u32) -> Vec<u8> {
        // SAFETY: The layout of the structure is fixed and can be initialized by
        // reading its content from byte array.
        unsafe {
            from_raw_parts(
                self.config
                    .as_slice()
                    .as_ptr()
                    .offset(offset as isize)
                    .cast::<u8>(),
                size as usize,
            )
            .to_vec()
        }
    }

    fn set_event_idx(&mut self, enabled: bool) {
        self.event_idx = enabled;
    }

    fn update_memory(&mut self, mem: GuestMemoryAtomic<GuestMemoryMmap>) -> IoResult<()> {
        self.mem = Some(mem);
        Ok(())
    }

    fn handle_event(
        &mut self,
        device_event: u16,
        evset: EventSet,
        vrings: &[VringRwLock],
        _thread_id: usize,
    ) -> IoResult<()> {
        if evset != EventSet::IN {
            return Err(GpioError::HandleEventNotEpollIn.into());
        }

        match device_event {
            REQUEST_QUEUE => {
                let vring = &vrings[0];

                if self.event_idx {
                    // vm-virtio's Queue implementation only checks avail_index
                    // once, so to properly support EVENT_IDX we need to keep
                    // calling process_request_queue() until it stops finding
                    // new requests on the queue.
                    loop {
                        vring.disable_notification().unwrap();
                        self.process_request_queue(vring)?;
                        if !vring.enable_notification().unwrap() {
                            break;
                        }
                    }
                } else {
                    // Without EVENT_IDX, a single call is enough.
                    self.process_request_queue(vring)?;
                }
            }

            EVENT_QUEUE => {
                let vring = &vrings[1];

                if self.event_idx {
                    // vm-virtio's Queue implementation only checks avail_index
                    // once, so to properly support EVENT_IDX we need to keep
                    // calling process_event_queue() until it stops finding
                    // new requests on the queue.
                    loop {
                        vring.disable_notification().unwrap();
                        self.process_event_queue(vring)?;
                        if !vring.enable_notification().unwrap() {
                            break;
                        }
                    }
                } else {
                    // Without EVENT_IDX, a single call is enough.
                    self.process_event_queue(vring)?;
                }
            }

            _ => {
                return Err(GpioError::HandleEventUnknown.into());
            }
        }
        Ok(())
    }

    fn exit_event(&self, _thread_index: usize) -> Option<EventFd> {
        self.exit_event.try_clone().ok()
    }
}

/// A handle to the communication channels for the simulated GPIO device.
#[derive(Debug)]
pub struct GpioBackendHandle {
    /// Handle of the main backend thread.
    #[allow(dead_code)]
    pub join_handle: JoinHandle<Result<()>>,
    /// Sends gpio events to the guest.
    pub events_tx: Sender<GpioEvent>,
    /// Receives gpio events from the guest.
    pub events_rx: Receiver<GpioEvent>,
}

/// Start the vhost-user GPIO backend daemon.
pub fn start_gpio_backend<P: AsRef<Path>>(gpio_socket: P, gpio_lines: Vec<GpioLine>) -> GpioBackendHandle {
    let gpio_socket = gpio_socket.as_ref().to_owned();
    let (backend_events_tx, device_events_rx) = unbounded();
    let (device_events_tx, backend_events_rx) = unbounded();

    let join_handle = spawn(move || {
        let backend = Arc::new(RwLock::new(
            VhostUserGpioBackend::new(backend_events_tx, backend_events_rx, gpio_lines)?
        ));

        let mut daemon = VhostUserDaemon::new(
            String::from("nest-vhost-gpio-backend"),
            backend.clone(),
            GuestMemoryAtomic::new(GuestMemoryMmap::new()),
        ).map_err(|_| GpioError::StartBackendFailed)?;

        daemon.serve(gpio_socket).map_err(|_| GpioError::StartBackendFailed)?;

        Ok(())
    });

    GpioBackendHandle {
        join_handle,
        events_tx: device_events_tx,
        events_rx: device_events_rx,
    }
}


// Sub-module for messaging between the backend and the simulated device
pub mod messaging {
    use serde::{Deserialize, Serialize};

    /// A GPIO event sent between the [VhostUserGpioBackend] and simulated device.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum GpioEvent {
        ValueLow(u16),
        ValueHigh(u16),
    }
}

// Sub-module for virtio specific structs and constants
#[allow(dead_code)]
pub mod virtio {
    use std::mem::size_of;
    use vm_memory::{
        ByteValued, Le16, Le32,
    };

    /// Virtio GPIO Feature bits
    pub const VIRTIO_GPIO_F_IRQ: u16 = 0;

    pub const QUEUE_SIZE: usize = 256;
    pub const NUM_QUEUES: usize = 2;

    /// Queues
    pub const REQUEST_QUEUE: u16 = 0;
    pub const EVENT_QUEUE: u16 = 1;

    /// Possible values of the status field
    pub const VIRTIO_GPIO_STATUS_OK: u8 = 0x0;
    pub const VIRTIO_GPIO_STATUS_ERR: u8 = 0x1;

    /// Possible values of the interrupt status field
    pub const VIRTIO_GPIO_IRQ_STATUS_INVALID: u8 = 0x0;
    pub const VIRTIO_GPIO_IRQ_STATUS_VALID: u8 = 0x1;

    // Virtio GPIO message types
    pub const VIRTIO_GPIO_MSG_GET_LINE_NAMES: u16 = 0x0001;
    pub const VIRTIO_GPIO_MSG_GET_DIRECTION: u16 = 0x0002;
    pub const VIRTIO_GPIO_MSG_SET_DIRECTION: u16 = 0x0003;
    pub const VIRTIO_GPIO_MSG_GET_VALUE: u16 = 0x0004;
    pub const VIRTIO_GPIO_MSG_SET_VALUE: u16 = 0x0005;
    pub const VIRTIO_GPIO_MSG_SET_IRQ_TYPE: u16 = 0x0006;

    /* Virtio GPIO direction types */
    pub const VIRTIO_GPIO_DIRECTION_NONE: u8 = 0x00;
    pub const VIRTIO_GPIO_DIRECTION_OUT: u8 = 0x01;
    pub const VIRTIO_GPIO_DIRECTION_IN: u8 = 0x02;

    /* Virtio GPIO values */
    pub const VIRTIO_GPIO_VALUE_LOW: u8 = 0x00;
    pub const VIRTIO_GPIO_VALUE_HIGH: u8 = 0x01;

    /* Virtio GPIO interrupt types */
    pub const VIRTIO_GPIO_IRQ_TYPE_NONE: u8 = 0x00;
    pub const VIRTIO_GPIO_IRQ_TYPE_EDGE_RISING: u8 = 0x01;
    pub const VIRTIO_GPIO_IRQ_TYPE_EDGE_FALLING: u8 = 0x02;
    pub const VIRTIO_GPIO_IRQ_TYPE_EDGE_BOTH: u8 = 0x03;
    pub const VIRTIO_GPIO_IRQ_TYPE_LEVEL_HIGH: u8 = 0x04;
    pub const VIRTIO_GPIO_IRQ_TYPE_LEVEL_LOW: u8 = 0x08;

    #[derive(Copy, Clone, Default, Debug)]
    #[repr(C)]
    pub struct VirtioGpioRequest {
        pub rtype: Le16,
        pub gpio: Le16,
        pub value: Le32,
    }
    // SAFETY: The layout of the structure is fixed and can be initialized by reading its content.
    unsafe impl ByteValued for VirtioGpioRequest {}

    #[derive(Copy, Clone, Default)]
    #[repr(C)]
    pub struct VirtioGpioIrqRequest {
        pub gpio: Le16,
    }
    // SAFETY: The layout of the structure is fixed and can be initialized by reading its content.
    unsafe impl ByteValued for VirtioGpioIrqRequest {}

    #[derive(Copy, Clone, Default)]
    #[repr(C)]
    pub struct VirtioGpioIrqResponse {
        pub status: u8,
    }
    // SAFETY: The layout of the structure is fixed and can be initialized by reading its content.
    unsafe impl ByteValued for VirtioGpioIrqResponse {}

    #[derive(Copy, Clone, Default, Debug)]
    #[repr(C)]
    pub struct VirtioGpioConfig {
        pub ngpio: Le16,
        pub padding: Le16,
        pub gpio_names_size: Le32,
    }
    // SAFETY: The layout of the structure is fixed and can be initialized by reading its content.
    unsafe impl ByteValued for VirtioGpioConfig {}

    impl VirtioGpioConfig {
        pub fn as_slice(&self) -> &[u8] {
            // SAFETY: The structure is C-compatible and has a fixed layout.
            unsafe {
                std::slice::from_raw_parts(
                    (self as *const Self) as *const u8,
                    size_of::<Self>(),
                )
            }
        }
    }
}

