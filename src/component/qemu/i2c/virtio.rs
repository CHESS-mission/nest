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
