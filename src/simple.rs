//! A simple graphics API abstraction inspired by "No Graphics API" blog post.
//! Provides `gpuMalloc`/`gpuFree` style memory management and simplified rendering.
//!
//! # Buffer upload model
//!
//! Buffer uploads are architecture-aware:
//! - **UMA (integrated / unified memory):** buffers are uploaded directly through mapped memory.
//! - **Discrete GPU memory:** data is uploaded through an internal staging buffer and transfer copy.
//!
//! You can query this behavior with [`GraphicsContext::is_unified_memory`].
//!
//! # Typical indexed rendering flow
//!
//! ```no_run
//! use rust_and_vulkan::simple::{
//!     Buffer, CommandBuffer, GraphicsContext, IndexType,
//! };
//!
//! # fn demo(context: &GraphicsContext) -> rust_and_vulkan::simple::Result<()> {
//! #[repr(C)]
//! #[derive(Clone, Copy)]
//! struct Vertex {
//!     pos: [f32; 3],
//!     uv: [f32; 2],
//! }
//!
//! let vertices = [
//!     Vertex { pos: [-0.5, -0.5, 0.0], uv: [0.0, 0.0] },
//!     Vertex { pos: [ 0.5, -0.5, 0.0], uv: [1.0, 0.0] },
//!     Vertex { pos: [ 0.0,  0.5, 0.0], uv: [0.5, 1.0] },
//! ];
//! let indices: [u32; 3] = [0, 1, 2];
//!
//! let vertex_buffer = Buffer::vertex_buffer(context, &vertices)?;
//! let index_buffer = Buffer::index_buffer_u32(context, &indices)?;
//!
//! let cmd = CommandBuffer::allocate(context)?;
//! cmd.begin()?;
//! cmd.bind_vertex_buffer(0, &vertex_buffer, 0);
//! cmd.bind_index_buffer(&index_buffer, 0, IndexType::U32);
//! cmd.draw_indexed(indices.len() as u32, 1, 0, 0, 0);
//! cmd.end()?;
//! # Ok(()) }
//! ```

use std::ptr;

unsafe fn load_device_fn(
    device: crate::VkDevice,
    name: &'static [u8],
) -> Option<unsafe extern "C" fn()> {
    debug_assert!(name.last() == Some(&0));
    std::mem::transmute(crate::vkGetDeviceProcAddr(
        device,
        name.as_ptr() as *const i8,
    ))
}

unsafe fn vk_get_descriptor_ext_dynamic(
    device: crate::VkDevice,
    descriptor_info: *const crate::VkDescriptorGetInfoEXT,
    descriptor_size: usize,
    descriptor: *mut std::ffi::c_void,
) -> bool {
    let Some(func) = load_device_fn(device, b"vkGetDescriptorEXT\0") else {
        return false;
    };
    let f: unsafe extern "C" fn(
        crate::VkDevice,
        *const crate::VkDescriptorGetInfoEXT,
        usize,
        *mut std::ffi::c_void,
    ) = std::mem::transmute(func);
    f(device, descriptor_info, descriptor_size, descriptor);
    true
}

unsafe fn vk_cmd_bind_descriptor_buffers_ext_dynamic(
    device: crate::VkDevice,
    command_buffer: crate::VkCommandBuffer,
    binding_info_count: u32,
    binding_infos: *const crate::VkDescriptorBufferBindingInfoEXT,
) -> bool {
    let Some(func) = load_device_fn(device, b"vkCmdBindDescriptorBuffersEXT\0") else {
        return false;
    };
    let f: unsafe extern "C" fn(
        crate::VkCommandBuffer,
        u32,
        *const crate::VkDescriptorBufferBindingInfoEXT,
    ) = std::mem::transmute(func);
    f(command_buffer, binding_info_count, binding_infos);
    true
}

unsafe fn vk_cmd_set_descriptor_buffer_offsets_ext_dynamic(
    device: crate::VkDevice,
    command_buffer: crate::VkCommandBuffer,
    pipeline_bind_point: crate::VkPipelineBindPoint,
    layout: crate::VkPipelineLayout,
    first_set: u32,
    set_count: u32,
    buffer_indices: *const u32,
    offsets: *const u64,
) -> bool {
    let Some(func) = load_device_fn(device, b"vkCmdSetDescriptorBufferOffsetsEXT\0") else {
        return false;
    };
    let f: unsafe extern "C" fn(
        crate::VkCommandBuffer,
        crate::VkPipelineBindPoint,
        crate::VkPipelineLayout,
        u32,
        u32,
        *const u32,
        *const u64,
    ) = std::mem::transmute(func);
    f(
        command_buffer,
        pipeline_bind_point,
        layout,
        first_set,
        set_count,
        buffer_indices,
        offsets,
    );
    true
}

// Pipeline stage constants for barriers
pub const STAGE_TRANSFER: u32 =
    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_TRANSFER_BIT as u32;
pub const STAGE_COMPUTE: u32 =
    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT as u32;
pub const STAGE_GRAPHICS: u32 =
    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_ALL_GRAPHICS_BIT as u32;
pub const STAGE_ALL: u32 =
    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_ALL_COMMANDS_BIT as u32;
pub const STAGE_HOST: u32 = crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_HOST_BIT as u32;

// More granular graphics stages for hazard-aware barriers
pub const STAGE_VERTEX_SHADER: u32 =
    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_VERTEX_SHADER_BIT as u32;
pub const STAGE_PIXEL_SHADER: u32 =
    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT as u32;
pub const STAGE_RASTER_COLOR_OUT: u32 =
    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT as u32;
pub const STAGE_RASTER_DEPTH_OUT: u32 =
    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT as u32;
pub const STAGE_DRAW_INDIRECT: u32 =
    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_DRAW_INDIRECT_BIT as u32;

// Hazard flags for barrier cache invalidation
bitflags::bitflags! {
    pub struct HazardFlags: u32 {
        const DRAW_ARGUMENTS = 1 << 0;
        const DESCRIPTORS = 1 << 1;
        const DEPTH_STENCIL = 1 << 2;
    }
}

// Swapchain present-fence structure & constant for VK_KHR_swapchain_maintenance1.
// Some loaders/drivers may not expose this in the generated bindings; we provide
// a small local C-compatible struct (first member is the sType numeric value).
pub const VK_STRUCTURE_TYPE_SWAPCHAIN_PRESENT_FENCE_INFO_KHR: u32 = 1000275001;

#[repr(C)]
pub struct SwapchainPresentFenceInfoKHR {
    pub sType: u32,
    pub pNext: *const std::ffi::c_void,
    pub swapchainCount: u32,
    pub pFences: *const crate::VkFence,
}

// Shader stage constants for pipeline layouts
pub const SHADER_STAGE_VERTEX: u32 =
    crate::VkShaderStageFlagBits::VK_SHADER_STAGE_VERTEX_BIT as u32;
pub const SHADER_STAGE_FRAGMENT: u32 =
    crate::VkShaderStageFlagBits::VK_SHADER_STAGE_FRAGMENT_BIT as u32;
pub const SHADER_STAGE_COMPUTE: u32 =
    crate::VkShaderStageFlagBits::VK_SHADER_STAGE_COMPUTE_BIT as u32;
pub const SHADER_STAGE_ALL_GRAPHICS: u32 = SHADER_STAGE_VERTEX | SHADER_STAGE_FRAGMENT;

/// Specialization constants for pipeline compilation
#[derive(Default)]
pub struct SpecializationConstants {
    entries: Vec<crate::VkSpecializationMapEntry>,
    data: Vec<u8>,
}

impl SpecializationConstants {
    /// Create a new empty specialization constants builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a u32 constant
    pub fn add_u32(mut self, constant_id: u32, value: u32) -> Self {
        let offset = self.data.len();
        self.data.extend_from_slice(&value.to_ne_bytes());
        self.entries.push(crate::VkSpecializationMapEntry {
            constantID: constant_id,
            offset: offset as u32,
            size: std::mem::size_of::<u32>(),
        });
        self
    }

    /// Build Vulkan specialization info (returns None if no entries)
    pub fn build(&self) -> Option<crate::VkSpecializationInfo> {
        if self.entries.is_empty() {
            return None;
        }
        Some(crate::VkSpecializationInfo {
            mapEntryCount: self.entries.len() as u32,
            pMapEntries: self.entries.as_ptr(),
            dataSize: self.data.len(),
            pData: self.data.as_ptr() as *const std::ffi::c_void,
        })
    }
}

/// Memory types for allocation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    /// CPU-mapped GPU memory (write-combined, fast for CPU writes, GPU reads)
    CpuMapped,
    /// GPU-only memory (optimal for textures, compressed layouts)
    GpuOnly,
    /// CPU-cached memory (for readback from GPU)
    CpuCached,
}

/// Simple error type for the API
#[derive(Debug)]
pub enum Error {
    Vulkan(String),
    OutOfMemory,
    InvalidArgument,
    Unsupported,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Vulkan(msg) => write!(f, "Vulkan error: {}", msg),
            Error::OutOfMemory => write!(f, "Out of memory"),
            Error::InvalidArgument => write!(f, "Invalid argument"),
            Error::Unsupported => write!(f, "Unsupported feature"),
        }
    }
}

impl std::error::Error for Error {}

/// Result type for the simple API
pub type Result<T> = std::result::Result<T, Error>;

bitflags::bitflags! {
    /// Texture usage flags
    pub struct TextureUsage: u32 {
        const SAMPLED = 1 << 0;
        const RENDER_TARGET = 1 << 1;
        const DEPTH_STENCIL = 1 << 2;
        const TRANSFER_SRC = 1 << 3;
        const TRANSFER_DST = 1 << 4;
    }
}

bitflags::bitflags! {
    /// Buffer usage flags
    pub struct BufferUsage: u32 {
        const VERTEX = 1 << 0;
        const INDEX = 1 << 1;
        const UNIFORM = 1 << 2;
        const STORAGE = 1 << 3;
        const TRANSFER_SRC = 1 << 4;
        const TRANSFER_DST = 1 << 5;
    }
}

/// Texture format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Rgba8Unorm,
    Bgra8Unorm,
    Rgba32Float,
    Depth32Float,
}

impl Format {
    pub fn to_vk_format(&self) -> crate::VkFormat {
        match self {
            Format::Rgba8Unorm => crate::VkFormat::VK_FORMAT_R8G8B8A8_UNORM,
            Format::Bgra8Unorm => crate::VkFormat::VK_FORMAT_B8G8R8A8_UNORM,
            Format::Rgba32Float => crate::VkFormat::VK_FORMAT_R32G32B32A32_SFLOAT,
            Format::Depth32Float => crate::VkFormat::VK_FORMAT_D32_SFLOAT,
        }
    }

    pub fn aspect_mask(&self) -> u32 {
        match self {
            Format::Rgba8Unorm | Format::Bgra8Unorm | Format::Rgba32Float => {
                crate::VkImageAspectFlagBits::VK_IMAGE_ASPECT_COLOR_BIT as u32
            }
            Format::Depth32Float => crate::VkImageAspectFlagBits::VK_IMAGE_ASPECT_DEPTH_BIT as u32,
        }
    }
}

/// Main context for the simple graphics API
pub struct GraphicsContext {
    _instance: crate::VkInstance,
    _physical_device: crate::VkPhysicalDevice,
    device: crate::VkDevice,
    _graphics_queue: crate::VkQueue,
    _present_queue: crate::VkQueue,
    _command_pool: crate::VkCommandPool,
    memory_properties: crate::VkPhysicalDeviceMemoryProperties,
    has_uma: bool,
    descriptor_buffer_supported: bool,
}

impl GraphicsContext {
    /// Create a new graphics context from existing Vulkan and SDL objects
    pub fn new(
        instance: crate::VkInstance,
        physical_device: crate::VkPhysicalDevice,
        device: crate::VkDevice,
        graphics_queue: crate::VkQueue,
        present_queue: crate::VkQueue,
        command_pool: crate::VkCommandPool,
        descriptor_buffer_supported: bool,
    ) -> Result<Self> {
        unsafe {
            let mut memory_properties = std::mem::zeroed();
            crate::vkGetPhysicalDeviceMemoryProperties(physical_device, &mut memory_properties);

            let mut has_uma = false;
            for i in 0..memory_properties.memoryTypeCount {
                let flags = memory_properties.memoryTypes[i as usize].propertyFlags;
                let host_visible = (flags
                    & crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT as u32)
                    != 0;
                let device_local = (flags
                    & crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT as u32)
                    != 0;
                if host_visible && device_local {
                    has_uma = true;
                    break;
                }
            }

            Ok(GraphicsContext {
                _instance: instance,
                _physical_device: physical_device,
                device,
                _graphics_queue: graphics_queue,
                _present_queue: present_queue,
                _command_pool: command_pool,
                memory_properties,
                has_uma,
                descriptor_buffer_supported,
            })
        }
    }

    /// Returns true when the current adapter exposes unified memory (UMA),
    /// meaning at least one memory type is both host-visible and device-local.
    pub fn is_unified_memory(&self) -> bool {
        self.has_uma
    }

    pub fn descriptor_buffer_supported(&self) -> bool {
        self.descriptor_buffer_supported
    }

    /// Return the raw Vulkan device handle.
    pub fn vk_device(&self) -> crate::VkDevice {
        self.device
    }

    /// Find memory type index for given memory type
    fn find_memory_type(&self, memory_type: MemoryType) -> Result<u32> {
        let property_flags = match memory_type {
            MemoryType::CpuMapped => {
                (crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT as u32)
                    | (crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_HOST_COHERENT_BIT as u32)
            }
            MemoryType::GpuOnly => {
                crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT as u32
            }
            MemoryType::CpuCached => {
                (crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT as u32)
                    | (crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_HOST_CACHED_BIT as u32)
            }
        };

        for i in 0..self.memory_properties.memoryTypeCount {
            let properties = self.memory_properties.memoryTypes[i as usize].propertyFlags;
            if (properties & property_flags) == property_flags {
                return Ok(i);
            }
        }

        Err(Error::Unsupported)
    }

    /// Find a memory type that matches both property requirements and buffer memoryTypeBits
    fn find_compatible_memory_type(
        &self,
        memory_type: MemoryType,
        required_bits: u32,
    ) -> Result<u32> {
        let property_flags = match memory_type {
            MemoryType::CpuMapped => {
                (crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT as u32)
                    | (crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_HOST_COHERENT_BIT as u32)
            }
            MemoryType::GpuOnly => {
                crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT as u32
            }
            MemoryType::CpuCached => {
                (crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT as u32)
                    | (crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_HOST_CACHED_BIT as u32)
            }
        };

        for i in 0..self.memory_properties.memoryTypeCount {
            // Check both property flags AND buffer requirement bits
            if (required_bits & (1 << i)) != 0 {
                let properties = self.memory_properties.memoryTypes[i as usize].propertyFlags;
                if (properties & property_flags) == property_flags {
                    return Ok(i);
                }
            }
        }

        // Fallback: if no exact match, try just the property flags
        self.find_memory_type(memory_type)
    }

    /// Allocate GPU memory with specified size, alignment and type
    pub fn gpu_malloc(
        &self,
        size: usize,
        _alignment: usize,
        memory_type: MemoryType,
    ) -> Result<GpuAllocation> {
        // Create buffer first to get memory requirements
        let buffer_info = crate::VkBufferCreateInfo {
            sType: crate::VkStructureType::VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            pNext: ptr::null(),
            flags: 0,
            size: size as u64,
            usage: (crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_TRANSFER_SRC_BIT as u32)
                | (crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_TRANSFER_DST_BIT as u32)
                | (crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_SHADER_DEVICE_ADDRESS_BIT as u32)
                | (crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_STORAGE_BUFFER_BIT as u32),
            sharingMode: crate::VkSharingMode::VK_SHARING_MODE_EXCLUSIVE,
            queueFamilyIndexCount: 0,
            pQueueFamilyIndices: ptr::null(),
        };

        let mut buffer: crate::VkBuffer = ptr::null_mut();
        unsafe {
            let result = crate::vkCreateBuffer(self.device, &buffer_info, ptr::null(), &mut buffer);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create buffer: {:?}",
                    result
                )));
            }

            let mut requirements: crate::VkMemoryRequirements = std::mem::zeroed();
            crate::vkGetBufferMemoryRequirements(self.device, buffer, &mut requirements);

            // Find memory type that satisfies both property flags AND buffer requirements
            let memory_type_index =
                self.find_compatible_memory_type(memory_type, requirements.memoryTypeBits)?;

            // Adjust size for alignment
            let aligned_size = if requirements.size % requirements.alignment == 0 {
                requirements.size
            } else {
                (requirements.size / requirements.alignment + 1) * requirements.alignment
            };

            // Allocate memory with device address flag for buffer device address
            let mut flags_info = crate::VkMemoryAllocateFlagsInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_FLAGS_INFO,
                pNext: ptr::null(),
                flags: crate::VkMemoryAllocateFlagBits::VK_MEMORY_ALLOCATE_DEVICE_ADDRESS_BIT
                    as u32,
                deviceMask: 0,
            };

            let alloc_info = crate::VkMemoryAllocateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                pNext: &mut flags_info as *mut _ as *mut std::ffi::c_void,
                allocationSize: aligned_size,
                memoryTypeIndex: memory_type_index,
            };

            let mut memory: crate::VkDeviceMemory = ptr::null_mut();
            let result =
                crate::vkAllocateMemory(self.device, &alloc_info, ptr::null(), &mut memory);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyBuffer(self.device, buffer, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to allocate memory: {:?}",
                    result
                )));
            }

            // Bind memory to buffer
            let result = crate::vkBindBufferMemory(self.device, buffer, memory, 0);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyBuffer(self.device, buffer, ptr::null());
                crate::vkFreeMemory(self.device, memory, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to bind buffer memory: {:?}",
                    result
                )));
            }

            // Map memory if CPU accessible
            let cpu_ptr = if memory_type == MemoryType::CpuMapped
                || memory_type == MemoryType::CpuCached
            {
                let mut mapped_ptr: *mut std::ffi::c_void = ptr::null_mut();
                let result =
                    crate::vkMapMemory(self.device, memory, 0, aligned_size, 0, &mut mapped_ptr);
                if result != crate::VkResult::VK_SUCCESS {
                    crate::vkDestroyBuffer(self.device, buffer, ptr::null());
                    crate::vkFreeMemory(self.device, memory, ptr::null());
                    return Err(Error::Vulkan(format!("Failed to map memory: {:?}", result)));
                }
                mapped_ptr as *mut u8
            } else {
                ptr::null_mut()
            };

            // Get device address
            let addr_info = crate::VkBufferDeviceAddressInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_BUFFER_DEVICE_ADDRESS_INFO,
                pNext: ptr::null(),
                buffer: buffer,
            };
            let device_address = crate::vkGetBufferDeviceAddress(self.device, &addr_info);

            Ok(GpuAllocation {
                buffer: buffer,
                memory: memory,
                cpu_ptr: cpu_ptr,
                gpu_ptr: device_address,
                size: size,
                device: self.device,
            })
        }
    }

    /// Simplified gpu_malloc for common case (CPU-mapped, 16-byte aligned)
    pub fn gpu_malloc_simple<T>(&self, count: usize) -> Result<GpuAllocation> {
        let size = std::mem::size_of::<T>() * count;
        let alignment = std::mem::align_of::<T>();
        self.gpu_malloc(size, alignment, MemoryType::CpuMapped)
    }

    /// Free GPU memory (handled by Drop implementation of GpuAllocation)
    pub fn gpu_free(_allocation: GpuAllocation) {
        // Memory is freed when allocation goes out of scope
    }

    /// Allocate a descriptor buffer (VK_EXT_descriptor_buffer) suitable for binding with
    /// `VK_BUFFER_USAGE_SAMPLER_DESCRIPTOR_BUFFER_BIT_EXT`.
    ///
    /// This is needed for `TextureDescriptorHeap` to pass validation when binding via
    /// `vkCmdBindDescriptorBuffersEXT`.
    pub fn gpu_malloc_descriptor_buffer(
        &self,
        size: usize,
        alignment: usize,
    ) -> Result<GpuAllocation> {
        use std::ptr;

        if size == 0 {
            return Err(Error::InvalidArgument);
        }
        if !self.descriptor_buffer_supported() {
            return Err(Error::Unsupported);
        }

        unsafe {
            let buffer_info = crate::VkBufferCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                size: size as u64,
                usage:
                    (crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_SAMPLER_DESCRIPTOR_BUFFER_BIT_EXT
                        as u32)
                        | (crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_SHADER_DEVICE_ADDRESS_BIT
                            as u32),
                sharingMode: crate::VkSharingMode::VK_SHARING_MODE_EXCLUSIVE,
                queueFamilyIndexCount: 0,
                pQueueFamilyIndices: ptr::null(),
            };

            let mut buffer: crate::VkBuffer = ptr::null_mut();
            let result = crate::vkCreateBuffer(self.device, &buffer_info, ptr::null(), &mut buffer);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create descriptor buffer: {:?}",
                    result
                )));
            }

            let mut requirements: crate::VkMemoryRequirements = std::mem::zeroed();
            crate::vkGetBufferMemoryRequirements(self.device, buffer, &mut requirements);

            let memory_type_index = self
                .find_compatible_memory_type(MemoryType::CpuMapped, requirements.memoryTypeBits)?;

            // Align allocation size to Vulkan requirements (and caller alignment to be safe)
            let required_align = requirements.alignment.max(alignment as u64);
            let aligned_size = if requirements.size % required_align == 0 {
                requirements.size
            } else {
                (requirements.size / required_align + 1) * required_align
            };

            let mut flags_info = crate::VkMemoryAllocateFlagsInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_FLAGS_INFO,
                pNext: ptr::null(),
                flags: crate::VkMemoryAllocateFlagBits::VK_MEMORY_ALLOCATE_DEVICE_ADDRESS_BIT
                    as u32,
                deviceMask: 0,
            };

            let alloc_info = crate::VkMemoryAllocateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                pNext: &mut flags_info as *mut _ as *mut std::ffi::c_void,
                allocationSize: aligned_size,
                memoryTypeIndex: memory_type_index,
            };

            let mut memory: crate::VkDeviceMemory = ptr::null_mut();
            let result =
                crate::vkAllocateMemory(self.device, &alloc_info, ptr::null(), &mut memory);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyBuffer(self.device, buffer, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to allocate descriptor buffer memory: {:?}",
                    result
                )));
            }

            let result = crate::vkBindBufferMemory(self.device, buffer, memory, 0);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyBuffer(self.device, buffer, ptr::null());
                crate::vkFreeMemory(self.device, memory, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to bind descriptor buffer memory: {:?}",
                    result
                )));
            }

            let mut mapped_ptr: *mut std::ffi::c_void = ptr::null_mut();
            let result =
                crate::vkMapMemory(self.device, memory, 0, aligned_size, 0, &mut mapped_ptr);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyBuffer(self.device, buffer, ptr::null());
                crate::vkFreeMemory(self.device, memory, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to map descriptor buffer memory: {:?}",
                    result
                )));
            }

            let addr_info = crate::VkBufferDeviceAddressInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_BUFFER_DEVICE_ADDRESS_INFO,
                pNext: ptr::null(),
                buffer,
            };
            let device_address = crate::vkGetBufferDeviceAddress(self.device, &addr_info);

            Ok(GpuAllocation {
                buffer,
                memory,
                cpu_ptr: mapped_ptr as *mut u8,
                gpu_ptr: device_address,
                size: size,
                device: self.device,
            })
        }
    }

    /// Create a buffer and upload data efficiently.
    ///
    /// On UMA systems this allocates host-visible memory directly and writes in place.
    /// On discrete systems this allocates device-local memory and performs upload via
    /// a staging buffer and transfer copy.
    pub fn create_buffer_with_data(&self, usage: BufferUsage, data: &[u8]) -> Result<Buffer> {
        if data.is_empty() {
            return Err(Error::InvalidArgument);
        }

        if self.has_uma {
            let mut direct_usage = usage;
            direct_usage.insert(BufferUsage::TRANSFER_SRC);
            let buffer = Buffer::new(self, data.len(), direct_usage, MemoryType::CpuMapped)?;
            buffer.write(data)?;
            Ok(buffer)
        } else {
            let mut device_usage = usage;
            device_usage.insert(BufferUsage::TRANSFER_DST);
            let device_buffer = Buffer::new(self, data.len(), device_usage, MemoryType::GpuOnly)?;

            let staging = Buffer::new(
                self,
                data.len(),
                BufferUsage::TRANSFER_SRC,
                MemoryType::CpuMapped,
            )?;
            staging.write(data)?;

            let command_buffer = CommandBuffer::allocate(self)?;
            command_buffer.begin()?;
            command_buffer.copy_vk_buffer(
                staging.vk_buffer(),
                device_buffer.vk_buffer(),
                data.len(),
                0,
                0,
            )?;
            command_buffer.end()?;

            let fence = self.submit(&command_buffer)?;
            fence.wait_forever()?;

            Ok(device_buffer)
        }
    }

    /// Create and upload a typed vertex buffer.
    pub fn create_vertex_buffer<T: Copy>(&self, vertices: &[T]) -> Result<Buffer> {
        if vertices.is_empty() {
            return Err(Error::InvalidArgument);
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(
                vertices.as_ptr() as *const u8,
                std::mem::size_of_val(vertices),
            )
        };
        self.create_buffer_with_data(BufferUsage::VERTEX, bytes)
    }

    /// Create and upload a typed index buffer.
    pub fn create_index_buffer_u16(&self, indices: &[u16]) -> Result<Buffer> {
        if indices.is_empty() {
            return Err(Error::InvalidArgument);
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(
                indices.as_ptr() as *const u8,
                std::mem::size_of_val(indices),
            )
        };
        self.create_buffer_with_data(BufferUsage::INDEX, bytes)
    }

    /// Create and upload a typed index buffer.
    pub fn create_index_buffer_u32(&self, indices: &[u32]) -> Result<Buffer> {
        if indices.is_empty() {
            return Err(Error::InvalidArgument);
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(
                indices.as_ptr() as *const u8,
                std::mem::size_of_val(indices),
            )
        };
        self.create_buffer_with_data(BufferUsage::INDEX, bytes)
    }

    /// Upload texture data with optimal GPU memory allocation
    /// Allocates GPU-only memory and performs copy with DCC compression
    pub fn upload_texture(
        &self,
        command_buffer: &CommandBuffer,
        data: &[u8],
        width: u32,
        height: u32,
        format: Format,
        usage: TextureUsage,
    ) -> Result<Texture> {
        // Create texture with GPU-only memory
        let texture = Texture::new(self, width, height, format, usage)?;

        // Create staging buffer in CPU-mapped memory
        let staging_size = data.len();
        let staging = self.gpu_malloc(staging_size, 16, MemoryType::CpuMapped)?;

        // Copy data to staging buffer
        staging.write(data)?;

        // Begin command buffer recording
        command_buffer.begin()?;

        // Transition texture to transfer destination
        command_buffer.transition_to_transfer_dst(&texture);

        // Copy from staging buffer to texture
        command_buffer.copy_buffer_to_texture(&staging, &texture, width, height);

        // Transition texture to shader read-only
        command_buffer.transition_to_shader_read(&texture);

        // End command buffer
        command_buffer.end()?;

        // Submit and wait for completion
        let fence = self.submit(command_buffer)?;
        fence.wait_forever()?;

        Ok(texture)
    }

    /// Get memory requirements for a texture with given parameters
    pub fn texture_size_align(
        &self,
        width: u32,
        height: u32,
        format: Format,
        usage: TextureUsage,
    ) -> Result<(usize, usize)> {
        use std::ptr;

        // Convert usage flags to Vulkan image usage
        let mut vk_usage = 0u32;
        if usage.contains(TextureUsage::SAMPLED) {
            vk_usage |= crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_SAMPLED_BIT as u32;
        }
        if usage.contains(TextureUsage::RENDER_TARGET) {
            vk_usage |= crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT as u32;
        }
        if usage.contains(TextureUsage::DEPTH_STENCIL) {
            vk_usage |=
                crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT as u32;
        }
        if usage.contains(TextureUsage::TRANSFER_SRC) {
            vk_usage |= crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_TRANSFER_SRC_BIT as u32;
        }
        if usage.contains(TextureUsage::TRANSFER_DST) {
            vk_usage |= crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_TRANSFER_DST_BIT as u32;
        }

        unsafe {
            let image_info = crate::VkImageCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                imageType: crate::VkImageType::VK_IMAGE_TYPE_2D,
                format: format.to_vk_format(),
                extent: crate::VkExtent3D {
                    width,
                    height,
                    depth: 1,
                },
                mipLevels: 1,
                arrayLayers: 1,
                samples: crate::VkSampleCountFlagBits::VK_SAMPLE_COUNT_1_BIT,
                tiling: crate::VkImageTiling::VK_IMAGE_TILING_OPTIMAL,
                usage: vk_usage,
                sharingMode: crate::VkSharingMode::VK_SHARING_MODE_EXCLUSIVE,
                queueFamilyIndexCount: 0,
                pQueueFamilyIndices: ptr::null(),
                initialLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_UNDEFINED,
            };

            let mut image: crate::VkImage = ptr::null_mut();
            let result = crate::vkCreateImage(self.device, &image_info, ptr::null(), &mut image);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create temporary image for size query: {:?}",
                    result
                )));
            }

            let mut requirements: crate::VkMemoryRequirements = std::mem::zeroed();
            crate::vkGetImageMemoryRequirements(self.device, image, &mut requirements);
            crate::vkDestroyImage(self.device, image, ptr::null());

            Ok((requirements.size as usize, requirements.alignment as usize))
        }
    }

    /// Create a default sampler (linear filtering, repeat wrap)
    pub fn create_default_sampler(&self) -> Result<crate::VkSampler> {
        use std::ptr;

        unsafe {
            let sampler_info = crate::VkSamplerCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                magFilter: crate::VkFilter::VK_FILTER_LINEAR,
                minFilter: crate::VkFilter::VK_FILTER_LINEAR,
                mipmapMode: crate::VkSamplerMipmapMode::VK_SAMPLER_MIPMAP_MODE_LINEAR,
                addressModeU: crate::VkSamplerAddressMode::VK_SAMPLER_ADDRESS_MODE_REPEAT,
                addressModeV: crate::VkSamplerAddressMode::VK_SAMPLER_ADDRESS_MODE_REPEAT,
                addressModeW: crate::VkSamplerAddressMode::VK_SAMPLER_ADDRESS_MODE_REPEAT,
                mipLodBias: 0.0,
                anisotropyEnable: 0,
                maxAnisotropy: 1.0,
                compareEnable: 0,
                compareOp: crate::VkCompareOp::VK_COMPARE_OP_ALWAYS,
                minLod: 0.0,
                maxLod: 0.0,
                borderColor: crate::VkBorderColor::VK_BORDER_COLOR_FLOAT_TRANSPARENT_BLACK,
                unnormalizedCoordinates: 0,
            };

            let mut sampler: crate::VkSampler = ptr::null_mut();
            let result =
                crate::vkCreateSampler(self.device, &sampler_info, ptr::null(), &mut sampler);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create sampler: {:?}",
                    result
                )));
            }

            Ok(sampler)
        }
    }

    /// Create a semaphore for synchronization
    pub fn create_semaphore(&self) -> Result<crate::VkSemaphore> {
        use std::ptr;

        unsafe {
            let semaphore_info = crate::VkSemaphoreCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
            };

            let mut semaphore: crate::VkSemaphore = ptr::null_mut();
            let result =
                crate::vkCreateSemaphore(self.device, &semaphore_info, ptr::null(), &mut semaphore);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create semaphore: {:?}",
                    result
                )));
            }

            Ok(semaphore)
        }
    }

    /// Destroy a semaphore
    pub fn destroy_semaphore(&self, semaphore: crate::VkSemaphore) {
        unsafe {
            crate::vkDestroySemaphore(self.device, semaphore, std::ptr::null());
        }
    }

    /// Destroy a sampler
    pub fn destroy_sampler(&self, sampler: crate::VkSampler) {
        unsafe {
            crate::vkDestroySampler(self.device, sampler, std::ptr::null());
        }
    }

    /// Wait for all operations on the device to complete
    pub fn wait_idle(&self) -> Result<()> {
        unsafe {
            let result = crate::vkDeviceWaitIdle(self.device);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to wait for device idle: {:?}",
                    result
                )));
            }
        }
        Ok(())
    }

    /// Submit a command buffer to the graphics queue and return a fence
    pub fn submit(&self, command_buffer: &CommandBuffer) -> Result<Fence> {
        self.submit_with_semaphores(command_buffer, &[], &[])
    }

    /// Submit command buffer with optional wait and signal semaphores
    /// wait_semaphores: semaphores to wait on before execution
    /// signal_semaphores: semaphores to signal after execution completes
    pub fn submit_with_semaphores(
        &self,
        command_buffer: &CommandBuffer,
        wait_semaphores: &[crate::VkSemaphore],
        signal_semaphores: &[crate::VkSemaphore],
    ) -> Result<Fence> {
        use std::ptr;

        unsafe {
            let fence_info = crate::VkFenceCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_FENCE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
            };

            let mut fence: crate::VkFence = ptr::null_mut();
            let result = crate::vkCreateFence(self.device, &fence_info, ptr::null(), &mut fence);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create fence: {:?}",
                    result
                )));
            }

            // Create wait stage masks (all graphics)
            let wait_stages: Vec<u32> = wait_semaphores
                .iter()
                .map(|_| {
                    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                        as u32
                })
                .collect();

            let submit_info = crate::VkSubmitInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_SUBMIT_INFO,
                pNext: ptr::null(),
                waitSemaphoreCount: wait_semaphores.len() as u32,
                pWaitSemaphores: if wait_semaphores.is_empty() {
                    ptr::null()
                } else {
                    wait_semaphores.as_ptr()
                },
                pWaitDstStageMask: if wait_stages.is_empty() {
                    ptr::null()
                } else {
                    wait_stages.as_ptr()
                },
                commandBufferCount: 1,
                pCommandBuffers: &command_buffer.vk_buffer(),
                signalSemaphoreCount: signal_semaphores.len() as u32,
                pSignalSemaphores: if signal_semaphores.is_empty() {
                    ptr::null()
                } else {
                    signal_semaphores.as_ptr()
                },
            };

            let result = crate::vkQueueSubmit(self._graphics_queue, 1, &submit_info, fence);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyFence(self.device, fence, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to submit command buffer: {:?}",
                    result
                )));
            }

            Ok(Fence {
                fence,
                device: self.device,
            })
        }
    }

    /// Submit command buffer using an existing VkFence (no fence creation)
    /// The provided fence must be in the unsignaled state when passed to vkQueueSubmit.
    pub fn submit_with_fence(
        &self,
        command_buffer: &CommandBuffer,
        wait_semaphores: &[crate::VkSemaphore],
        signal_semaphores: &[crate::VkSemaphore],
        fence: crate::VkFence,
    ) -> Result<()> {
        use std::ptr;

        unsafe {
            // Create wait stage masks (all graphics)
            let wait_stages: Vec<u32> = wait_semaphores
                .iter()
                .map(|_| {
                    crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                        as u32
                })
                .collect();

            let submit_info = crate::VkSubmitInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_SUBMIT_INFO,
                pNext: ptr::null(),
                waitSemaphoreCount: wait_semaphores.len() as u32,
                pWaitSemaphores: if wait_semaphores.is_empty() {
                    ptr::null()
                } else {
                    wait_semaphores.as_ptr()
                },
                pWaitDstStageMask: if wait_stages.is_empty() {
                    ptr::null()
                } else {
                    wait_stages.as_ptr()
                },
                commandBufferCount: 1,
                pCommandBuffers: &command_buffer.vk_buffer(),
                signalSemaphoreCount: signal_semaphores.len() as u32,
                pSignalSemaphores: if signal_semaphores.is_empty() {
                    ptr::null()
                } else {
                    signal_semaphores.as_ptr()
                },
            };

            let result = crate::vkQueueSubmit(self._graphics_queue, 1, &submit_info, fence);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to submit command buffer with fence: {:?}",
                    result
                )));
            }

            Ok(())
        }
    }
}

/// Fence for synchronization
pub struct Fence {
    fence: crate::VkFence,
    device: crate::VkDevice,
}

impl Fence {
    /// Create a new fence (initially signaled for first frame)
    pub fn create(context: &GraphicsContext) -> Result<Self> {
        unsafe {
            let fence_info = crate::VkFenceCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_FENCE_CREATE_INFO,
                pNext: std::ptr::null(),
                flags: crate::VkFenceCreateFlagBits::VK_FENCE_CREATE_SIGNALED_BIT as u32,
            };

            let mut fence = std::ptr::null_mut();
            let result =
                crate::vkCreateFence(context.device, &fence_info, std::ptr::null(), &mut fence);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create fence: {:?}",
                    result
                )));
            }

            Ok(Fence {
                fence,
                device: context.device,
            })
        }
    }

    /// Wait for the fence to be signaled (with timeout in nanoseconds)
    pub fn wait(&self, timeout_ns: u64) -> Result<()> {
        unsafe {
            let result = crate::vkWaitForFences(self.device, 1, &self.fence, 1, timeout_ns);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to wait for fence: {:?}",
                    result
                )));
            }
            Ok(())
        }
    }

    /// Wait indefinitely
    pub fn wait_forever(&self) -> Result<()> {
        self.wait(u64::MAX)
    }

    /// Check if fence is signaled (non-blocking)
    pub fn is_signaled(&self) -> Result<bool> {
        unsafe {
            let result = crate::vkGetFenceStatus(self.device, self.fence);
            match result {
                crate::VkResult::VK_SUCCESS => Ok(true),
                crate::VkResult::VK_NOT_READY => Ok(false),
                _ => Err(Error::Vulkan(format!(
                    "Failed to get fence status: {:?}",
                    result
                ))),
            }
        }
    }

    /// Return the raw VkFence handle
    pub fn raw(&self) -> crate::VkFence {
        self.fence
    }

    /// Reset the fence to unsignaled state (must be in signaled state)
    pub fn reset(&self, context: &GraphicsContext) -> Result<()> {
        unsafe {
            let result = crate::vkResetFences(context.device, 1, &self.fence);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to reset fence: {:?}",
                    result
                )));
            }
            Ok(())
        }
    }
}

impl Drop for Fence {
    fn drop(&mut self) {
        unsafe {
            crate::vkDestroyFence(self.device, self.fence, std::ptr::null());
        }
    }
}

/// A GPU memory allocation with both CPU and GPU pointers
pub struct GpuAllocation {
    /// CPU-mapped pointer (for CPU writes)
    pub cpu_ptr: *mut u8,
    /// GPU virtual address (for GPU access)
    pub gpu_ptr: u64,
    /// Size in bytes
    pub size: usize,
    /// Buffer handle (for device address)
    buffer: crate::VkBuffer,
    /// Memory handle (for cleanup)
    memory: crate::VkDeviceMemory,
    /// Device (for cleanup)
    device: crate::VkDevice,
}

impl GpuAllocation {
    /// Get CPU pointer as mutable slice
    pub unsafe fn as_slice_mut(&self) -> &mut [u8] {
        std::slice::from_raw_parts_mut(self.cpu_ptr, self.size)
    }

    /// Get CPU pointer as slice
    pub unsafe fn as_slice(&self) -> &[u8] {
        std::slice::from_raw_parts(self.cpu_ptr, self.size)
    }

    /// Write data to the allocation
    pub fn write(&self, data: &[u8]) -> Result<()> {
        if self.cpu_ptr.is_null() {
            return Err(Error::Unsupported);
        }
        if data.len() > self.size {
            return Err(Error::InvalidArgument);
        }
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.cpu_ptr, data.len());
        }
        Ok(())
    }

    /// Convert a host pointer within this allocation to a device pointer
    pub fn host_to_device_ptr(&self, host_ptr: *const u8) -> u64 {
        if host_ptr.is_null() || self.cpu_ptr.is_null() {
            return 0;
        }
        let host_ptr = host_ptr as usize;
        let base_ptr = self.cpu_ptr as usize;
        if host_ptr < base_ptr || host_ptr >= base_ptr + self.size {
            return 0;
        }
        let offset = host_ptr - base_ptr;
        self.gpu_ptr + offset as u64
    }

    /// Get the Vulkan buffer handle
    pub fn buffer(&self) -> crate::VkBuffer {
        self.buffer
    }
}

impl Drop for GpuAllocation {
    fn drop(&mut self) {
        unsafe {
            if !self.cpu_ptr.is_null() {
                crate::vkUnmapMemory(self.device, self.memory);
            }
            crate::vkDestroyBuffer(self.device, self.buffer, ptr::null());
            crate::vkFreeMemory(self.device, self.memory, ptr::null());
        }
    }
}

/// Bump allocator for temporary GPU allocations
pub struct GpuBumpAllocator {
    base_allocation: GpuAllocation,
    offset: usize,
}

impl GpuBumpAllocator {
    /// Create a new bump allocator with given capacity
    pub fn new(context: &GraphicsContext, capacity: usize) -> Result<Self> {
        let allocation = context.gpu_malloc(capacity, 16, MemoryType::CpuMapped)?;
        Ok(GpuBumpAllocator {
            base_allocation: allocation,
            offset: 0,
        })
    }

    /// Allocate memory for type T
    pub fn allocate<T>(&mut self, count: usize) -> Result<(*mut T, u64)> {
        let size = std::mem::size_of::<T>() * count;
        let alignment = std::mem::align_of::<T>();

        // Align offset
        let aligned_offset = (self.offset + alignment - 1) & !(alignment - 1);

        if aligned_offset + size > self.base_allocation.size {
            return Err(Error::OutOfMemory);
        }

        let cpu_ptr = unsafe { self.base_allocation.cpu_ptr.add(aligned_offset) as *mut T };
        // GPU pointer is base GPU address + offset
        let gpu_ptr = self.base_allocation.gpu_ptr + aligned_offset as u64;

        self.offset = aligned_offset + size;

        Ok((cpu_ptr, gpu_ptr))
    }

    /// Reset the allocator (doesn't free memory, just resets offset)
    pub fn reset(&mut self) {
        self.offset = 0;
    }
}

// More functionality can be added here (textures, buffers, pipelines, etc.)

/// A GPU buffer with bound memory
pub struct Buffer {
    buffer: crate::VkBuffer,
    memory: crate::VkDeviceMemory,
    size: usize,
    cpu_ptr: *mut u8,
    device: crate::VkDevice,
}

/// Index format for index buffer binding and indexed drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexType {
    U16,
    U32,
}

impl IndexType {
    fn to_vk(self) -> crate::VkIndexType {
        match self {
            IndexType::U16 => crate::VkIndexType::VK_INDEX_TYPE_UINT16,
            IndexType::U32 => crate::VkIndexType::VK_INDEX_TYPE_UINT32,
        }
    }
}

impl Buffer {
    /// Create a new buffer with specified size, usage, and memory type
    pub fn new(
        context: &GraphicsContext,
        size: usize,
        usage: BufferUsage,
        memory_type: MemoryType,
    ) -> Result<Self> {
        use std::ptr;

        if size == 0 {
            return Err(Error::InvalidArgument);
        }

        // Convert usage flags to Vulkan buffer usage
        let mut vk_usage = 0u32;
        if usage.contains(BufferUsage::VERTEX) {
            vk_usage |= crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_VERTEX_BUFFER_BIT as u32;
        }
        if usage.contains(BufferUsage::INDEX) {
            vk_usage |= crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_INDEX_BUFFER_BIT as u32;
        }
        if usage.contains(BufferUsage::UNIFORM) {
            vk_usage |= crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT as u32;
        }
        if usage.contains(BufferUsage::STORAGE) {
            vk_usage |= crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_STORAGE_BUFFER_BIT as u32;
        }
        if usage.contains(BufferUsage::TRANSFER_SRC) {
            vk_usage |= crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_TRANSFER_SRC_BIT as u32;
        }
        if usage.contains(BufferUsage::TRANSFER_DST) {
            vk_usage |= crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_TRANSFER_DST_BIT as u32;
        }

        // Buffers accessed through device addresses (e.g. vertex pulling in shaders)
        // must be created with SHADER_DEVICE_ADDRESS usage.
        let needs_device_address = usage.intersects(
            BufferUsage::VERTEX | BufferUsage::INDEX | BufferUsage::STORAGE | BufferUsage::UNIFORM,
        );
        if needs_device_address {
            vk_usage |=
                crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_SHADER_DEVICE_ADDRESS_BIT as u32;
        }

        unsafe {
            // Create buffer
            let buffer_info = crate::VkBufferCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                size: size as u64,
                usage: vk_usage,
                sharingMode: crate::VkSharingMode::VK_SHARING_MODE_EXCLUSIVE,
                queueFamilyIndexCount: 0,
                pQueueFamilyIndices: ptr::null(),
            };

            let mut buffer: crate::VkBuffer = ptr::null_mut();
            let result =
                crate::vkCreateBuffer(context.device, &buffer_info, ptr::null(), &mut buffer);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create buffer: {:?}",
                    result
                )));
            }

            // Get memory requirements
            let mut requirements: crate::VkMemoryRequirements = std::mem::zeroed();
            crate::vkGetBufferMemoryRequirements(context.device, buffer, &mut requirements);

            let memory_type_index =
                context.find_compatible_memory_type(memory_type, requirements.memoryTypeBits)?;

            // Allocate memory
            let mut memory_flags_info = crate::VkMemoryAllocateFlagsInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_FLAGS_INFO,
                pNext: ptr::null(),
                flags: if needs_device_address {
                    crate::VkMemoryAllocateFlagBits::VK_MEMORY_ALLOCATE_DEVICE_ADDRESS_BIT as u32
                } else {
                    0
                },
                deviceMask: 0,
            };
            let alloc_info = crate::VkMemoryAllocateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                pNext: if needs_device_address {
                    &mut memory_flags_info as *mut _ as *mut std::ffi::c_void
                } else {
                    ptr::null_mut()
                },
                allocationSize: requirements.size,
                memoryTypeIndex: memory_type_index,
            };

            let mut memory: crate::VkDeviceMemory = ptr::null_mut();
            let result =
                crate::vkAllocateMemory(context.device, &alloc_info, ptr::null(), &mut memory);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyBuffer(context.device, buffer, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to allocate buffer memory: {:?}",
                    result
                )));
            }

            // Bind memory to buffer
            let result = crate::vkBindBufferMemory(context.device, buffer, memory, 0);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkFreeMemory(context.device, memory, ptr::null());
                crate::vkDestroyBuffer(context.device, buffer, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to bind buffer memory: {:?}",
                    result
                )));
            }

            // Map memory if CPU accessible
            let cpu_ptr =
                if memory_type == MemoryType::CpuMapped || memory_type == MemoryType::CpuCached {
                    let mut mapped_ptr: *mut std::ffi::c_void = ptr::null_mut();
                    let result = crate::vkMapMemory(
                        context.device,
                        memory,
                        0,
                        requirements.size,
                        0,
                        &mut mapped_ptr,
                    );
                    if result != crate::VkResult::VK_SUCCESS {
                        crate::vkFreeMemory(context.device, memory, ptr::null());
                        crate::vkDestroyBuffer(context.device, buffer, ptr::null());
                        return Err(Error::Vulkan(format!(
                            "Failed to map buffer memory: {:?}",
                            result
                        )));
                    }
                    mapped_ptr as *mut u8
                } else {
                    ptr::null_mut()
                };

            Ok(Buffer {
                buffer,
                memory,
                size,
                cpu_ptr,
                device: context.device,
            })
        }
    }

    /// Get CPU pointer for writing data (only if memory is CPU mapped)
    pub fn cpu_ptr(&self) -> Option<*mut u8> {
        if self.cpu_ptr.is_null() {
            None
        } else {
            Some(self.cpu_ptr)
        }
    }

    /// Write data to the buffer
    pub fn write(&self, data: &[u8]) -> Result<()> {
        if self.cpu_ptr.is_null() {
            return Err(Error::Unsupported);
        }
        if data.len() > self.size {
            return Err(Error::InvalidArgument);
        }
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.cpu_ptr, data.len());
        }
        Ok(())
    }

    /// Write data to the buffer at a byte offset.
    pub fn write_at(&self, offset: usize, data: &[u8]) -> Result<()> {
        if self.cpu_ptr.is_null() {
            return Err(Error::Unsupported);
        }
        if offset > self.size || data.len() > (self.size - offset) {
            return Err(Error::InvalidArgument);
        }
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.cpu_ptr.add(offset), data.len());
        }
        Ok(())
    }

    /// Create and upload a buffer from raw bytes.
    pub fn from_data(context: &GraphicsContext, usage: BufferUsage, data: &[u8]) -> Result<Self> {
        context.create_buffer_with_data(usage, data)
    }

    /// Create and upload a typed vertex buffer.
    pub fn vertex_buffer<T: Copy>(context: &GraphicsContext, vertices: &[T]) -> Result<Self> {
        context.create_vertex_buffer(vertices)
    }

    /// Create and upload a `u16` index buffer.
    pub fn index_buffer_u16(context: &GraphicsContext, indices: &[u16]) -> Result<Self> {
        context.create_index_buffer_u16(indices)
    }

    /// Create and upload a `u32` index buffer.
    pub fn index_buffer_u32(context: &GraphicsContext, indices: &[u32]) -> Result<Self> {
        context.create_index_buffer_u32(indices)
    }

    /// Create and upload a buffer for use with device addresses (bindless access).
    /// This creates a storage buffer that can be accessed via device address in shaders.
    pub fn from_device_address<T: Copy>(context: &GraphicsContext, data: &[T]) -> Result<Self> {
        let bytes = unsafe {
            std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(data))
        };
        context.create_buffer_with_data(BufferUsage::STORAGE, bytes)
    }

    /// Get the Vulkan buffer handle
    pub fn vk_buffer(&self) -> crate::VkBuffer {
        self.buffer
    }

    /// Get the GPU device address for this buffer.
    pub fn device_address(&self) -> u64 {
        unsafe {
            let info = crate::VkBufferDeviceAddressInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_BUFFER_DEVICE_ADDRESS_INFO,
                pNext: ptr::null(),
                buffer: self.buffer,
            };
            crate::vkGetBufferDeviceAddress(self.device, &info)
        }
    }

    /// Get buffer size in bytes
    pub fn size(&self) -> usize {
        self.size
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            if !self.cpu_ptr.is_null() {
                crate::vkUnmapMemory(self.device, self.memory);
            }
            crate::vkDestroyBuffer(self.device, self.buffer, std::ptr::null());
            crate::vkFreeMemory(self.device, self.memory, std::ptr::null());
        }
    }
}

/// A GPU texture with bound memory
pub struct Texture {
    image: crate::VkImage,
    image_view: crate::VkImageView,
    memory: crate::VkDeviceMemory,
    format: Format,
    width: u32,
    height: u32,
    device: crate::VkDevice,
}

impl Texture {
    /// Create a new texture with specified dimensions, format, and usage
    pub fn new(
        context: &GraphicsContext,
        width: u32,
        height: u32,
        format: Format,
        usage: TextureUsage,
    ) -> Result<Self> {
        use std::ptr;

        // Convert usage flags to Vulkan image usage
        let mut vk_usage = 0u32;
        if usage.contains(TextureUsage::SAMPLED) {
            vk_usage |= crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_SAMPLED_BIT as u32;
        }
        if usage.contains(TextureUsage::RENDER_TARGET) {
            vk_usage |= crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT as u32;
        }
        if usage.contains(TextureUsage::DEPTH_STENCIL) {
            vk_usage |=
                crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT as u32;
        }
        if usage.contains(TextureUsage::TRANSFER_SRC) {
            vk_usage |= crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_TRANSFER_SRC_BIT as u32;
        }
        if usage.contains(TextureUsage::TRANSFER_DST) {
            vk_usage |= crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_TRANSFER_DST_BIT as u32;
        }

        unsafe {
            let image_info = crate::VkImageCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                imageType: crate::VkImageType::VK_IMAGE_TYPE_2D,
                format: format.to_vk_format(),
                extent: crate::VkExtent3D {
                    width,
                    height,
                    depth: 1,
                },
                mipLevels: 1,
                arrayLayers: 1,
                samples: crate::VkSampleCountFlagBits::VK_SAMPLE_COUNT_1_BIT,
                tiling: crate::VkImageTiling::VK_IMAGE_TILING_OPTIMAL,
                usage: vk_usage,
                sharingMode: crate::VkSharingMode::VK_SHARING_MODE_EXCLUSIVE,
                queueFamilyIndexCount: 0,
                pQueueFamilyIndices: ptr::null(),
                initialLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_UNDEFINED,
            };

            let mut image: crate::VkImage = ptr::null_mut();
            let result = crate::vkCreateImage(context.device, &image_info, ptr::null(), &mut image);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create image: {:?}",
                    result
                )));
            }

            // Get memory requirements
            let mut requirements: crate::VkMemoryRequirements = std::mem::zeroed();
            crate::vkGetImageMemoryRequirements(context.device, image, &mut requirements);

            // Find memory type (GPU-only for optimal tiling)
            let property_flags =
                crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT as u32;
            let mut memory_type_index = !0u32;
            for i in 0..context.memory_properties.memoryTypeCount {
                let properties = context.memory_properties.memoryTypes[i as usize].propertyFlags;
                if (properties & property_flags) == property_flags {
                    memory_type_index = i;
                    break;
                }
            }

            if memory_type_index == !0u32 {
                crate::vkDestroyImage(context.device, image, ptr::null());
                return Err(Error::Unsupported);
            }

            // Allocate memory
            let alloc_info = crate::VkMemoryAllocateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                pNext: ptr::null(),
                allocationSize: requirements.size,
                memoryTypeIndex: memory_type_index,
            };

            let mut memory: crate::VkDeviceMemory = ptr::null_mut();
            let result =
                crate::vkAllocateMemory(context.device, &alloc_info, ptr::null(), &mut memory);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyImage(context.device, image, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to allocate image memory: {:?}",
                    result
                )));
            }

            // Bind memory to image
            let result = crate::vkBindImageMemory(context.device, image, memory, 0);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkFreeMemory(context.device, memory, ptr::null());
                crate::vkDestroyImage(context.device, image, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to bind image memory: {:?}",
                    result
                )));
            }

            // Create image view
            let view_info = crate::VkImageViewCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                image,
                viewType: crate::VkImageViewType::VK_IMAGE_VIEW_TYPE_2D,
                format: format.to_vk_format(),
                components: crate::VkComponentMapping {
                    r: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    g: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    b: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    a: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                },
                subresourceRange: crate::VkImageSubresourceRange {
                    aspectMask: format.aspect_mask(),
                    baseMipLevel: 0,
                    levelCount: 1,
                    baseArrayLayer: 0,
                    layerCount: 1,
                },
            };

            let mut image_view = ptr::null_mut();
            let result =
                crate::vkCreateImageView(context.device, &view_info, ptr::null(), &mut image_view);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkFreeMemory(context.device, memory, ptr::null());
                crate::vkDestroyImage(context.device, image, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to create image view: {:?}",
                    result
                )));
            }

            Ok(Texture {
                image,
                image_view,
                memory,
                format,
                width,
                height,
                device: context.device,
            })
        }
    }

    /// Get the Vulkan image handle
    pub fn vk_image(&self) -> crate::VkImage {
        self.image
    }

    /// Get the Vulkan image view handle
    pub fn vk_image_view(&self) -> crate::VkImageView {
        self.image_view
    }

    /// Get texture width
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get texture height
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get texture format
    pub fn format(&self) -> Format {
        self.format
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe {
            crate::vkDestroyImageView(self.device, self.image_view, std::ptr::null());
            crate::vkDestroyImage(self.device, self.image, std::ptr::null());
            crate::vkFreeMemory(self.device, self.memory, std::ptr::null());
        }
    }
}

/// A compiled shader module
pub struct ShaderModule {
    module: crate::VkShaderModule,
    device: crate::VkDevice,
}

impl ShaderModule {
    /// Create a shader module from SPIR-V bytecode
    pub fn new(context: &GraphicsContext, spirv_code: &[u32]) -> Result<Self> {
        use std::ptr;

        unsafe {
            let create_info = crate::VkShaderModuleCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                codeSize: spirv_code.len() * std::mem::size_of::<u32>(),
                pCode: spirv_code.as_ptr(),
            };

            let mut module = std::ptr::null_mut();
            let result =
                crate::vkCreateShaderModule(context.device, &create_info, ptr::null(), &mut module);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create shader module: {:?}",
                    result
                )));
            }

            Ok(ShaderModule {
                module,
                device: context.device,
            })
        }
    }

    /// Get the Vulkan shader module handle
    pub fn vk_module(&self) -> crate::VkShaderModule {
        self.module
    }
}

impl Drop for ShaderModule {
    fn drop(&mut self) {
        unsafe {
            crate::vkDestroyShaderModule(self.device, self.module, std::ptr::null());
        }
    }
}

/// Root argument system for passing data to shaders
/// Uses a single 64-bit pointer to a root data struct
pub struct RootArguments {
    allocation: GpuAllocation,
    size: usize,
}

impl RootArguments {
    /// Create root arguments for a given type
    pub fn new<T>(context: &GraphicsContext) -> Result<Self> {
        let size = std::mem::size_of::<T>();
        let alignment = std::mem::align_of::<T>();

        let allocation = context.gpu_malloc(size, alignment, MemoryType::CpuMapped)?;

        Ok(RootArguments { allocation, size })
    }

    /// Get CPU pointer for writing root data
    pub fn cpu_ptr<T>(&self) -> *mut T {
        self.allocation.cpu_ptr as *mut T
    }

    /// Get GPU address for passing to shaders
    pub fn gpu_address(&self) -> u64 {
        self.allocation.gpu_ptr
    }

    /// Write data to root arguments
    pub fn write<T>(&self, data: &T) -> Result<()> {
        if std::mem::size_of::<T>() > self.size {
            return Err(Error::InvalidArgument);
        }

        unsafe {
            std::ptr::copy_nonoverlapping(
                data as *const T as *const u8,
                self.allocation.cpu_ptr,
                std::mem::size_of::<T>(),
            );
        }

        Ok(())
    }

    /// Get size in bytes
    pub fn size(&self) -> usize {
        self.size
    }
}

/// Pipeline layout for describing resource bindings
pub struct PipelineLayout {
    layout: crate::VkPipelineLayout,
    device: crate::VkDevice,
    #[allow(dead_code)]
    set_layouts: Vec<crate::VkDescriptorSetLayout>,
    push_constant_range: Option<crate::VkPushConstantRange>,
}

impl PipelineLayout {
    /// Create a simple pipeline layout (no descriptors, no push constants)
    pub fn new(context: &GraphicsContext) -> Result<Self> {
        use std::ptr;

        unsafe {
            let create_info = crate::VkPipelineLayoutCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                setLayoutCount: 0,
                pSetLayouts: ptr::null(),
                pushConstantRangeCount: 0,
                pPushConstantRanges: ptr::null(),
            };

            let mut layout = std::ptr::null_mut();
            let result = crate::vkCreatePipelineLayout(
                context.device,
                &create_info,
                ptr::null(),
                &mut layout,
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create pipeline layout: {:?}",
                    result
                )));
            }

            Ok(PipelineLayout {
                layout,
                device: context.device,
                set_layouts: Vec::new(),
                push_constant_range: None,
            })
        }
    }

    /// Create a pipeline layout with push constants for root pointer (compute only)
    pub fn with_push_constants(context: &GraphicsContext) -> Result<Self> {
        Self::with_root_argument(
            context,
            crate::VkShaderStageFlagBits::VK_SHADER_STAGE_COMPUTE_BIT as u32,
        )
    }

    /// Create a pipeline layout with root argument for specified shader stages
    pub fn with_root_argument(context: &GraphicsContext, stage_flags: u32) -> Result<Self> {
        use std::ptr;

        unsafe {
            // Push constant range for a single 64-bit root pointer
            let push_constant_range = crate::VkPushConstantRange {
                stageFlags: stage_flags,
                offset: 0,
                size: 8, // 64-bit pointer
            };

            let create_info = crate::VkPipelineLayoutCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                setLayoutCount: 0,
                pSetLayouts: ptr::null(),
                pushConstantRangeCount: 1,
                pPushConstantRanges: &push_constant_range,
            };

            let mut layout = std::ptr::null_mut();
            let result = crate::vkCreatePipelineLayout(
                context.device,
                &create_info,
                ptr::null(),
                &mut layout,
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create pipeline layout with push constants: {:?}",
                    result
                )));
            }

            Ok(PipelineLayout {
                layout,
                device: context.device,
                set_layouts: Vec::new(),
                push_constant_range: Some(push_constant_range),
            })
        }
    }

    /// Create a pipeline layout with separate root arguments for vertex and fragment stages
    /// Uses two 64-bit pointers: vertex root at offset 0, fragment root at offset 8
    pub fn with_separate_root_arguments(context: &GraphicsContext) -> Result<Self> {
        use std::ptr;

        unsafe {
            // Push constant range for two 64-bit root pointers (16 bytes total)
            let push_constant_range = crate::VkPushConstantRange {
                stageFlags: SHADER_STAGE_VERTEX | SHADER_STAGE_FRAGMENT,
                offset: 0,
                size: 16, // Two 64-bit pointers
            };

            let create_info = crate::VkPipelineLayoutCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                setLayoutCount: 0,
                pSetLayouts: ptr::null(),
                pushConstantRangeCount: 1,
                pPushConstantRanges: &push_constant_range,
            };

            let mut layout = std::ptr::null_mut();
            let result = crate::vkCreatePipelineLayout(
                context.device,
                &create_info,
                ptr::null(),
                &mut layout,
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create pipeline layout with separate root arguments: {:?}",
                    result
                )));
            }

            Ok(PipelineLayout {
                layout,
                device: context.device,
                set_layouts: Vec::new(),
                push_constant_range: Some(push_constant_range),
            })
        }
    }

    /// Create a pipeline layout for bindless textures using descriptor buffers
    pub fn with_bindless_textures(context: &GraphicsContext, stage_flags: u32) -> Result<Self> {
        use std::ptr;

        // For descriptor buffers, we don't need descriptor set layouts
        // The descriptor buffer is bound directly to the command buffer
        let push_constant_range = if stage_flags != 0 {
            Some(crate::VkPushConstantRange {
                stageFlags: stage_flags,
                offset: 0,
                size: 8, // 64-bit pointer
            })
        } else {
            None
        };

        unsafe {
            let create_info = crate::VkPipelineLayoutCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                setLayoutCount: 0,
                pSetLayouts: ptr::null(),
                pushConstantRangeCount: push_constant_range.as_ref().map(|_| 1).unwrap_or(0),
                pPushConstantRanges: push_constant_range
                    .as_ref()
                    .map(|r| r as *const _)
                    .unwrap_or(ptr::null()),
            };

            let mut layout = std::ptr::null_mut();
            let result = crate::vkCreatePipelineLayout(
                context.device,
                &create_info,
                ptr::null(),
                &mut layout,
            );

            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create pipeline layout with bindless textures: {:?}",
                    result
                )));
            }

            Ok(PipelineLayout {
                layout,
                device: context.device,
                set_layouts: Vec::new(),
                push_constant_range,
            })
        }
    }

    /// Create a pipeline layout with descriptor set layouts and optional push constants
    pub fn with_descriptor_set_layouts(
        context: &GraphicsContext,
        set_layouts: &[DescriptorSetLayout],
        stage_flags: u32,
    ) -> Result<Self> {
        Self::with_descriptor_set_layouts_and_push_size(context, set_layouts, stage_flags, 128)
    }

    /// Create a pipeline layout with descriptor set layouts and custom push constant size
    pub fn with_descriptor_set_layouts_and_push_size(
        context: &GraphicsContext,
        set_layouts: &[DescriptorSetLayout],
        stage_flags: u32,
        push_constant_size: u32,
    ) -> Result<Self> {
        use std::ptr;

        let set_layout_handles: Vec<_> = set_layouts.iter().map(|l| l.vk_layout()).collect();

        let push_constant_range = if stage_flags != 0 && push_constant_size > 0 {
            Some(crate::VkPushConstantRange {
                stageFlags: stage_flags,
                offset: 0,
                size: push_constant_size,
            })
        } else {
            None
        };

        unsafe {
            let create_info = crate::VkPipelineLayoutCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                setLayoutCount: set_layout_handles.len() as u32,
                pSetLayouts: set_layout_handles.as_ptr(),
                pushConstantRangeCount: push_constant_range.as_ref().map(|_| 1).unwrap_or(0),
                pPushConstantRanges: push_constant_range
                    .as_ref()
                    .map(|r| r as *const _)
                    .unwrap_or(ptr::null()),
            };

            let mut layout = std::ptr::null_mut();
            let result = crate::vkCreatePipelineLayout(
                context.device,
                &create_info,
                ptr::null(),
                &mut layout,
            );

            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create pipeline layout with descriptor sets: {:?}",
                    result
                )));
            }

            Ok(PipelineLayout {
                layout,
                device: context.device,
                set_layouts: set_layout_handles,
                push_constant_range,
            })
        }
    }

    /// Get the Vulkan pipeline layout handle
    pub fn vk_layout(&self) -> crate::VkPipelineLayout {
        self.layout
    }

    /// Get push constant stage flags (0 if no push constants)
    pub fn push_constant_stage_flags(&self) -> u32 {
        self.push_constant_range
            .map(|range| range.stageFlags)
            .unwrap_or(0)
    }

    pub fn push_constant_size(&self) -> usize {
        self.push_constant_range
            .map(|range| range.size as usize)
            .unwrap_or(0)
    }

    /// Create a pipeline layout with flexible push constant size for specified shader stages
    /// This is useful for passing data larger than a pointer (e.g., mat4 = 64 bytes, vec4 = 16 bytes)
    pub fn with_push_constants_size(
        context: &GraphicsContext,
        stage_flags: u32,
        size: u32,
    ) -> Result<Self> {
        use std::ptr;

        unsafe {
            let push_constant_range = crate::VkPushConstantRange {
                stageFlags: stage_flags,
                offset: 0,
                size,
            };

            let create_info = crate::VkPipelineLayoutCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                setLayoutCount: 0,
                pSetLayouts: ptr::null(),
                pushConstantRangeCount: 1,
                pPushConstantRanges: &push_constant_range,
            };

            let mut layout = std::ptr::null_mut();
            let result = crate::vkCreatePipelineLayout(
                context.device,
                &create_info,
                ptr::null(),
                &mut layout,
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create pipeline layout with push constants: {:?}",
                    result
                )));
            }

            Ok(PipelineLayout {
                layout,
                device: context.device,
                set_layouts: Vec::new(),
                push_constant_range: Some(push_constant_range),
            })
        }
    }

    /// Create a pipeline layout with 64-byte push constants (for mat4 matrices)
    pub fn with_mat4_push_constants(context: &GraphicsContext, stage_flags: u32) -> Result<Self> {
        Self::with_push_constants_size(context, stage_flags, 64)
    }

    /// Create a pipeline layout with 16-byte push constants (for vec4 colors or 2x f64)
    pub fn with_vec4_push_constants(context: &GraphicsContext, stage_flags: u32) -> Result<Self> {
        Self::with_push_constants_size(context, stage_flags, 16)
    }
}

impl Drop for PipelineLayout {
    fn drop(&mut self) {
        unsafe {
            crate::vkDestroyPipelineLayout(self.device, self.layout, std::ptr::null());
        }
    }
}

/// Descriptor set layout for bindless textures using descriptor buffers
pub struct DescriptorSetLayout {
    layout: crate::VkDescriptorSetLayout,
    device: crate::VkDevice,
}

impl DescriptorSetLayout {
    /// Create a bindless descriptor set layout for combined image samplers.
    ///
    /// NOTE: This is intended for use with VK_EXT_descriptor_buffer.
    /// We deliberately do NOT set `VK_DESCRIPTOR_BINDING_VARIABLE_DESCRIPTOR_COUNT_BIT_EXT`
    /// here to avoid requiring the `descriptorBindingVariableDescriptorCount` feature.
    /// Instead, we allocate a fixed-capacity runtime array (`textures[]`) with
    /// `descriptorCount = max_textures`.
    pub fn new_bindless_textures(context: &GraphicsContext, max_textures: u32) -> Result<Self> {
        use std::ptr;

        unsafe {
            let binding = crate::VkDescriptorSetLayoutBinding {
                binding: 0,
                descriptorType: crate::VkDescriptorType::VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                descriptorCount: max_textures,
                stageFlags: crate::VkShaderStageFlagBits::VK_SHADER_STAGE_ALL_GRAPHICS as u32,
                pImmutableSamplers: ptr::null(),
            };

            let create_info = crate::VkDescriptorSetLayoutCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
                pNext: ptr::null(),
                flags: crate::VkDescriptorSetLayoutCreateFlagBits::VK_DESCRIPTOR_SET_LAYOUT_CREATE_DESCRIPTOR_BUFFER_BIT_EXT as u32,
                bindingCount: 1,
                pBindings: &binding,
            };

            let mut layout = std::ptr::null_mut();
            let result = crate::vkCreateDescriptorSetLayout(
                context.device,
                &create_info,
                ptr::null(),
                &mut layout,
            );

            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create descriptor set layout: {:?}",
                    result
                )));
            }

            Ok(DescriptorSetLayout {
                layout,
                device: context.device,
            })
        }
    }

    /// Create a standard (non-bindless) descriptor set layout for texture array
    pub fn new_texture_array(context: &GraphicsContext, texture_count: u32) -> Result<Self> {
        use std::ptr;

        unsafe {
            let binding = crate::VkDescriptorSetLayoutBinding {
                binding: 0,
                descriptorType: crate::VkDescriptorType::VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                descriptorCount: texture_count,
                stageFlags: crate::VkShaderStageFlagBits::VK_SHADER_STAGE_FRAGMENT_BIT as u32,
                pImmutableSamplers: ptr::null(),
            };

            let create_info = crate::VkDescriptorSetLayoutCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                bindingCount: 1,
                pBindings: &binding,
            };

            let mut layout = std::ptr::null_mut();
            let result = crate::vkCreateDescriptorSetLayout(
                context.device,
                &create_info,
                ptr::null(),
                &mut layout,
            );

            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create descriptor set layout: {:?}",
                    result
                )));
            }

            Ok(DescriptorSetLayout {
                layout,
                device: context.device,
            })
        }
    }

    /// Get Vulkan descriptor set layout handle
    pub fn vk_layout(&self) -> crate::VkDescriptorSetLayout {
        self.layout
    }
}

impl Drop for DescriptorSetLayout {
    fn drop(&mut self) {
        use std::ptr;
        unsafe {
            crate::vkDestroyDescriptorSetLayout(self.device, self.layout, ptr::null());
        }
    }
}

/// Descriptor pool for allocating traditional descriptor sets
pub struct DescriptorPool {
    pool: crate::VkDescriptorPool,
    device: crate::VkDevice,
}

impl DescriptorPool {
    /// Create a descriptor pool with capacity for the specified number of descriptor sets and descriptors
    pub fn new(context: &GraphicsContext, max_sets: u32, max_samplers: u32) -> Result<Self> {
        use std::ptr;

        unsafe {
            let pool_size = crate::VkDescriptorPoolSize {
                type_: crate::VkDescriptorType::VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                descriptorCount: max_samplers,
            };

            let create_info = crate::VkDescriptorPoolCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                maxSets: max_sets,
                poolSizeCount: 1,
                pPoolSizes: &pool_size,
            };

            let mut pool = std::ptr::null_mut();
            let result =
                crate::vkCreateDescriptorPool(context.device, &create_info, ptr::null(), &mut pool);

            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create descriptor pool: {:?}",
                    result
                )));
            }

            Ok(DescriptorPool {
                pool,
                device: context.device,
            })
        }
    }

    /// Allocate a descriptor set from this pool
    pub fn allocate(&self, layout: &DescriptorSetLayout) -> Result<DescriptorSet> {
        use std::ptr;

        unsafe {
            let layouts = [layout.vk_layout()];
            let alloc_info = crate::VkDescriptorSetAllocateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
                pNext: ptr::null(),
                descriptorPool: self.pool,
                descriptorSetCount: 1,
                pSetLayouts: layouts.as_ptr(),
            };

            let mut set = std::ptr::null_mut();
            let result = crate::vkAllocateDescriptorSets(self.device, &alloc_info, &mut set);

            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to allocate descriptor set: {:?}",
                    result
                )));
            }

            Ok(DescriptorSet {
                set,
                device: self.device,
            })
        }
    }
}

impl Drop for DescriptorPool {
    fn drop(&mut self) {
        use std::ptr;
        unsafe {
            crate::vkDestroyDescriptorPool(self.device, self.pool, ptr::null());
        }
    }
}

/// A single descriptor set allocated from a descriptor pool
pub struct DescriptorSet {
    set: crate::VkDescriptorSet,
    device: crate::VkDevice,
}

impl DescriptorSet {
    /// Write texture samplers to this descriptor set
    pub fn write_textures(
        &self,
        context: &GraphicsContext,
        textures: &[&Texture],
        sampler: crate::VkSampler,
    ) -> Result<()> {
        use std::ptr;

        unsafe {
            let mut image_infos: Vec<crate::VkDescriptorImageInfo> = textures
                .iter()
                .map(|tex| crate::VkDescriptorImageInfo {
                    sampler,
                    imageView: tex.image_view,
                    imageLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                })
                .collect();

            let write = crate::VkWriteDescriptorSet {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                pNext: ptr::null(),
                dstSet: self.set,
                dstBinding: 0,
                dstArrayElement: 0,
                descriptorCount: image_infos.len() as u32,
                descriptorType: crate::VkDescriptorType::VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                pImageInfo: image_infos.as_mut_ptr(),
                pBufferInfo: ptr::null(),
                pTexelBufferView: ptr::null(),
            };

            crate::vkUpdateDescriptorSets(context.device, 1, &write, 0, ptr::null());
            Ok(())
        }
    }

    /// Get the Vulkan descriptor set handle
    pub fn vk_set(&self) -> crate::VkDescriptorSet {
        self.set
    }
}

/// Texture descriptor heap for bindless texturing
/// Manages an array of combined-image-sampler descriptors in GPU memory.
pub struct TextureDescriptorHeap {
    allocation: GpuAllocation,
    descriptor_size: usize,
    capacity: usize,
    used: usize,
    image_views: Vec<crate::VkImageView>,
    device: crate::VkDevice,
}

impl TextureDescriptorHeap {
    /// Create a new texture descriptor heap with specified capacity.
    ///
    /// IMPORTANT: The underlying buffer must be created with
    /// `VK_BUFFER_USAGE_SAMPLER_DESCRIPTOR_BUFFER_BIT_EXT` for
    /// `vkCmdBindDescriptorBuffersEXT` validation to pass.
    pub fn new(context: &GraphicsContext, capacity: usize) -> Result<Self> {
        if !context.descriptor_buffer_supported() {
            return Err(Error::Unsupported);
        }

        // Get descriptor buffer properties
        let mut properties = crate::VkPhysicalDeviceDescriptorBufferPropertiesEXT {
            sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DESCRIPTOR_BUFFER_PROPERTIES_EXT,
            pNext: std::ptr::null_mut(),
            ..unsafe { std::mem::zeroed() }
        };

        unsafe {
            let mut props2 = crate::VkPhysicalDeviceProperties2 {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2,
                pNext: &mut properties as *mut _ as *mut std::ffi::c_void,
                properties: std::mem::zeroed(),
            };
            crate::vkGetPhysicalDeviceProperties2(context._physical_device, &mut props2);
        }

        let descriptor_size = properties.combinedImageSamplerDescriptorSize as usize;

        // Allocate GPU memory for descriptors. This allocation must be a buffer created with
        // the descriptor-buffer usage flag, not a generic storage buffer.
        let size = capacity * descriptor_size;

        // Descriptor buffer alignment requirement for offsets.
        let alignment = properties.descriptorBufferOffsetAlignment as usize;
        let allocation = context.gpu_malloc_descriptor_buffer(size, alignment)?;

        Ok(TextureDescriptorHeap {
            allocation,
            descriptor_size,
            capacity,
            used: 0,
            image_views: vec![ptr::null_mut(); capacity],
            device: context.device,
        })
    }

    /// Allocate space for a texture descriptor and return its index
    pub fn allocate(&mut self) -> Result<u32> {
        if self.used >= self.capacity {
            return Err(Error::OutOfMemory);
        }
        let index = self.used as u32;
        self.used += 1;
        Ok(index)
    }

    /// Write a texture descriptor at the specified index
    /// Uses vkGetDescriptorEXT to encode the hardware-specific descriptor
    pub fn write_descriptor(
        &mut self,
        context: &GraphicsContext,
        index: u32,
        texture: &Texture,
        sampler: crate::VkSampler,
    ) -> Result<()> {
        if index as usize >= self.capacity {
            return Err(Error::InvalidArgument);
        }

        unsafe {
            // Create image view if not already created
            let view_info = crate::VkImageViewCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                image: texture.image,
                viewType: crate::VkImageViewType::VK_IMAGE_VIEW_TYPE_2D,
                format: texture.format.to_vk_format(),
                components: crate::VkComponentMapping {
                    r: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    g: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    b: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    a: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                },
                subresourceRange: crate::VkImageSubresourceRange {
                    aspectMask: texture.format.aspect_mask(),
                    baseMipLevel: 0,
                    levelCount: 1,
                    baseArrayLayer: 0,
                    layerCount: 1,
                },
            };

            let mut image_view: crate::VkImageView = ptr::null_mut();
            let result =
                crate::vkCreateImageView(context.device, &view_info, ptr::null(), &mut image_view);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create image view for descriptor: {:?}",
                    result
                )));
            }

            // Destroy the old image view at this slot if one exists
            let old_view = self.image_views[index as usize];
            if !old_view.is_null() {
                crate::vkDestroyImageView(self.device, old_view, ptr::null());
            }

            // Create combined image sampler descriptor info
            let image_info = crate::VkDescriptorImageInfo {
                sampler,
                imageView: image_view,
                imageLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            };

            // Get the hardware descriptor encoding
            let descriptor_info = crate::VkDescriptorGetInfoEXT {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DESCRIPTOR_GET_INFO_EXT,
                pNext: ptr::null(),
                type_: crate::VkDescriptorType::VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                data: crate::VkDescriptorDataEXT {
                    pCombinedImageSampler: &image_info as *const _,
                },
            };

            // Calculate offset and write descriptor
            let offset = index as usize * self.descriptor_size;
            let dest_ptr = self.allocation.cpu_ptr.add(offset) as *mut std::ffi::c_void;

            // Use vkGetDescriptorEXT to encode the descriptor (loaded dynamically)
            if !vk_get_descriptor_ext_dynamic(
                context.device,
                &descriptor_info,
                self.descriptor_size,
                dest_ptr,
            ) {
                crate::vkDestroyImageView(context.device, image_view, ptr::null());
                return Err(Error::Unsupported);
            }

            // Store the image view for later destruction
            self.image_views[index as usize] = image_view;

            println!(
                "✓ Texture descriptor written at index {} (offset: 0x{:x}, size: {} bytes)",
                index, offset, self.descriptor_size
            );

            Ok(())
        }
    }

    /// Get GPU address of the descriptor heap
    pub fn gpu_address(&self) -> u64 {
        self.allocation.gpu_ptr
    }

    /// Get CPU pointer to descriptor heap memory
    pub fn cpu_ptr(&self) -> *mut u8 {
        self.allocation.cpu_ptr
    }

    /// Get descriptor size in bytes
    pub fn descriptor_size(&self) -> usize {
        self.descriptor_size
    }

    /// Get capacity (maximum number of descriptors)
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get number of used descriptors
    pub fn used(&self) -> usize {
        self.used
    }
}

impl Drop for TextureDescriptorHeap {
    fn drop(&mut self) {
        unsafe {
            for &image_view in &self.image_views {
                if !image_view.is_null() {
                    crate::vkDestroyImageView(self.device, image_view, std::ptr::null());
                }
            }
        }
    }
}

/// A graphics pipeline for rendering
pub struct GraphicsPipeline {
    pipeline: crate::VkPipeline,
    layout: crate::VkPipelineLayout,
    device: crate::VkDevice,
}

impl GraphicsPipeline {
    /// Create a simple graphics pipeline for rendering triangles (traditional descriptor sets).
    pub fn new(
        context: &GraphicsContext,
        vertex_shader: &ShaderModule,
        fragment_shader: &ShaderModule,
        layout: &PipelineLayout,
        render_pass: crate::VkRenderPass,
        _format: Format,
        vertex_specialization: Option<&SpecializationConstants>,
        fragment_specialization: Option<&SpecializationConstants>,
    ) -> Result<Self> {
        Self::new_internal(
            context,
            vertex_shader,
            fragment_shader,
            layout,
            render_pass,
            _format,
            vertex_specialization,
            fragment_specialization,
            0, // no special pipeline create flags
            false,
            true,
            false, // static scissor
        )
    }

    /// Create a graphics pipeline that is compatible with VK_EXT_descriptor_buffer.
    ///
    /// This sets `VK_PIPELINE_CREATE_DESCRIPTOR_BUFFER_BIT_EXT` so that descriptor
    /// buffers bound via `vkCmdBindDescriptorBuffersEXT`/`vkCmdSetDescriptorBufferOffsetsEXT`
    /// are considered valid for this pipeline.
    pub fn new_descriptor_buffer(
        context: &GraphicsContext,
        vertex_shader: &ShaderModule,
        fragment_shader: &ShaderModule,
        layout: &PipelineLayout,
        render_pass: crate::VkRenderPass,
        _format: Format,
        vertex_specialization: Option<&SpecializationConstants>,
        fragment_specialization: Option<&SpecializationConstants>,
    ) -> Result<Self> {
        Self::new_internal(
            context,
            vertex_shader,
            fragment_shader,
            layout,
            render_pass,
            _format,
            vertex_specialization,
            fragment_specialization,
            crate::VkPipelineCreateFlagBits::VK_PIPELINE_CREATE_DESCRIPTOR_BUFFER_BIT_EXT as u32,
            false,
            true,
            false, // static scissor
        )
    }

    /// Create a graphics pipeline with alpha blending enabled (src_alpha / one_minus_src_alpha).
    /// Suitable for UI overlays such as egui.
    pub fn new_with_blend(
        context: &GraphicsContext,
        vertex_shader: &ShaderModule,
        fragment_shader: &ShaderModule,
        layout: &PipelineLayout,
        render_pass: crate::VkRenderPass,
        format: Format,
        vertex_specialization: Option<&SpecializationConstants>,
        fragment_specialization: Option<&SpecializationConstants>,
    ) -> Result<Self> {
        Self::new_internal(
            context,
            vertex_shader,
            fragment_shader,
            layout,
            render_pass,
            format,
            vertex_specialization,
            fragment_specialization,
            0,
            true,
            true,
            false, // static scissor
        )
    }

    /// Create a graphics pipeline with alpha blending AND the descriptor-buffer flag.
    /// Use this when rendering with blending in a frame that also uses
    /// `VK_EXT_descriptor_buffer` (e.g. egui rendered after a descriptor-buffer scene pass).
    pub fn new_with_blend_descriptor_buffer(
        context: &GraphicsContext,
        vertex_shader: &ShaderModule,
        fragment_shader: &ShaderModule,
        layout: &PipelineLayout,
        render_pass: crate::VkRenderPass,
        format: Format,
        vertex_specialization: Option<&SpecializationConstants>,
        fragment_specialization: Option<&SpecializationConstants>,
    ) -> Result<Self> {
        Self::new_internal(
            context,
            vertex_shader,
            fragment_shader,
            layout,
            render_pass,
            format,
            vertex_specialization,
            fragment_specialization,
            crate::VkPipelineCreateFlagBits::VK_PIPELINE_CREATE_DESCRIPTOR_BUFFER_BIT_EXT as u32,
            true,
            false, // egui draws at Z=0 like the scene; disable depth test to avoid being culled
            true,  // dynamic scissor for per-primitive clip rects
        )
    }

    fn new_internal(
        context: &GraphicsContext,
        vertex_shader: &ShaderModule,
        fragment_shader: &ShaderModule,
        layout: &PipelineLayout,
        render_pass: crate::VkRenderPass,
        _format: Format,
        vertex_specialization: Option<&SpecializationConstants>,
        fragment_specialization: Option<&SpecializationConstants>,
        pipeline_create_flags: u32,
        blend_enable: bool,
        depth_test_enable: bool,
        dynamic_scissor: bool,
    ) -> Result<Self> {
        use std::ptr;

        unsafe {
            // Shader stage creation info
            let vertex_specialization_info = vertex_specialization.and_then(|s| s.build());
            let vertex_stage = crate::VkPipelineShaderStageCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                stage: crate::VkShaderStageFlagBits::VK_SHADER_STAGE_VERTEX_BIT,
                module: vertex_shader.vk_module(),
                pName: b"main\0".as_ptr() as *const i8,
                pSpecializationInfo: vertex_specialization_info
                    .as_ref()
                    .map_or(std::ptr::null(), |info| info as *const _),
            };

            let fragment_specialization_info = fragment_specialization.and_then(|s| s.build());
            let fragment_stage = crate::VkPipelineShaderStageCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                stage: crate::VkShaderStageFlagBits::VK_SHADER_STAGE_FRAGMENT_BIT,
                module: fragment_shader.vk_module(),
                pName: b"main\0".as_ptr() as *const i8,
                pSpecializationInfo: fragment_specialization_info
                    .as_ref()
                    .map_or(std::ptr::null(), |info| info as *const _),
            };

            let shader_stages = [vertex_stage, fragment_stage];

            // Vertex input (none for triangle with hardcoded vertices)
            let vertex_input = crate::VkPipelineVertexInputStateCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                vertexBindingDescriptionCount: 0,
                pVertexBindingDescriptions: ptr::null(),
                vertexAttributeDescriptionCount: 0,
                pVertexAttributeDescriptions: ptr::null(),
            };

            // Input assembly
            let input_assembly = crate::VkPipelineInputAssemblyStateCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                topology: crate::VkPrimitiveTopology::VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                primitiveRestartEnable: 0,
            };

            // Viewport and scissor
            let viewport = crate::VkViewport {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
                minDepth: 0.0,
                maxDepth: 1.0,
            };

            let scissor = crate::VkRect2D {
                offset: crate::VkOffset2D { x: 0, y: 0 },
                extent: crate::VkExtent2D {
                    width: 800,
                    height: 600,
                },
            };

            let viewport_state = crate::VkPipelineViewportStateCreateInfo {
                sType:
                    crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                viewportCount: 1,
                pViewports: &viewport,
                scissorCount: 1,
                pScissors: &scissor,
            };

            // Rasterization
            let rasterization = crate::VkPipelineRasterizationStateCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                depthClampEnable: 0,
                rasterizerDiscardEnable: 0,
                polygonMode: crate::VkPolygonMode::VK_POLYGON_MODE_FILL,
                cullMode: crate::VkCullModeFlagBits::VK_CULL_MODE_NONE as u32,
                frontFace: crate::VkFrontFace::VK_FRONT_FACE_CLOCKWISE,
                depthBiasEnable: 0,
                depthBiasConstantFactor: 0.0,
                depthBiasClamp: 0.0,
                depthBiasSlopeFactor: 0.0,
                lineWidth: 1.0,
            };

            // Multisampling (disabled)
            let multisampling = crate::VkPipelineMultisampleStateCreateInfo {
                sType:
                    crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                rasterizationSamples: crate::VkSampleCountFlagBits::VK_SAMPLE_COUNT_1_BIT,
                sampleShadingEnable: 0,
                minSampleShading: 0.0,
                pSampleMask: ptr::null(),
                alphaToCoverageEnable: 0,
                alphaToOneEnable: 0,
            };

            // Color blending — pre-multiplied alpha (egui standard):
            //   out.rgb = src.rgb + dst.rgb * (1 - src.a)
            //   out.a   = src.a   + dst.a   * (1 - src.a)
            let color_blend_attachment = crate::VkPipelineColorBlendAttachmentState {
                blendEnable: if blend_enable { 1 } else { 0 },
                srcColorBlendFactor: crate::VkBlendFactor::VK_BLEND_FACTOR_ONE,
                dstColorBlendFactor: crate::VkBlendFactor::VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA,
                colorBlendOp: crate::VkBlendOp::VK_BLEND_OP_ADD,
                srcAlphaBlendFactor: crate::VkBlendFactor::VK_BLEND_FACTOR_ONE,
                dstAlphaBlendFactor: crate::VkBlendFactor::VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA,
                alphaBlendOp: crate::VkBlendOp::VK_BLEND_OP_ADD,
                colorWriteMask: crate::VkColorComponentFlagBits::VK_COLOR_COMPONENT_R_BIT as u32
                    | crate::VkColorComponentFlagBits::VK_COLOR_COMPONENT_G_BIT as u32
                    | crate::VkColorComponentFlagBits::VK_COLOR_COMPONENT_B_BIT as u32
                    | crate::VkColorComponentFlagBits::VK_COLOR_COMPONENT_A_BIT as u32,
            };

            let color_blending = crate::VkPipelineColorBlendStateCreateInfo {
                sType:
                    crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                logicOpEnable: 0,
                logicOp: crate::VkLogicOp::VK_LOGIC_OP_COPY,
                attachmentCount: 1,
                pAttachments: &color_blend_attachment,
                blendConstants: [0.0, 0.0, 0.0, 0.0],
            };

            // Dynamic state: scissor rect (set per-draw for egui clip regions)
            let dynamic_states = [crate::VkDynamicState::VK_DYNAMIC_STATE_SCISSOR];
            let dynamic_state = crate::VkPipelineDynamicStateCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                dynamicStateCount: if dynamic_scissor {
                    dynamic_states.len() as u32
                } else {
                    0
                },
                pDynamicStates: if dynamic_scissor {
                    dynamic_states.as_ptr()
                } else {
                    ptr::null()
                },
            };

            // Depth stencil state for depth testing
            let depth_stencil = crate::VkPipelineDepthStencilStateCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                depthTestEnable: if depth_test_enable { 1 } else { 0 },
                depthWriteEnable: if depth_test_enable { 1 } else { 0 },
                depthCompareOp: crate::VkCompareOp::VK_COMPARE_OP_LESS,
                depthBoundsTestEnable: 0,
                stencilTestEnable: 0,
                front: crate::VkStencilOpState {
                    failOp: crate::VkStencilOp::VK_STENCIL_OP_KEEP,
                    passOp: crate::VkStencilOp::VK_STENCIL_OP_KEEP,
                    depthFailOp: crate::VkStencilOp::VK_STENCIL_OP_KEEP,
                    compareOp: crate::VkCompareOp::VK_COMPARE_OP_ALWAYS,
                    compareMask: 0,
                    writeMask: 0,
                    reference: 0,
                },
                back: crate::VkStencilOpState {
                    failOp: crate::VkStencilOp::VK_STENCIL_OP_KEEP,
                    passOp: crate::VkStencilOp::VK_STENCIL_OP_KEEP,
                    depthFailOp: crate::VkStencilOp::VK_STENCIL_OP_KEEP,
                    compareOp: crate::VkCompareOp::VK_COMPARE_OP_ALWAYS,
                    compareMask: 0,
                    writeMask: 0,
                    reference: 0,
                },
                minDepthBounds: 0.0,
                maxDepthBounds: 1.0,
            };

            // Create graphics pipeline
            let pipeline_info = crate::VkGraphicsPipelineCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
                pNext: ptr::null(),
                flags: pipeline_create_flags,
                stageCount: 2,
                pStages: shader_stages.as_ptr(),
                pVertexInputState: &vertex_input,
                pInputAssemblyState: &input_assembly,
                pTessellationState: ptr::null(),
                pViewportState: &viewport_state,
                pRasterizationState: &rasterization,
                pMultisampleState: &multisampling,
                pDepthStencilState: &depth_stencil,
                pColorBlendState: &color_blending,
                pDynamicState: &dynamic_state,
                layout: layout.vk_layout(),
                renderPass: render_pass,
                subpass: 0,
                basePipelineHandle: std::ptr::null_mut(),
                basePipelineIndex: -1,
            };

            let mut pipeline = std::ptr::null_mut();
            let result = crate::vkCreateGraphicsPipelines(
                context.device,
                std::ptr::null_mut(),
                1,
                &pipeline_info,
                ptr::null(),
                &mut pipeline,
            );

            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create graphics pipeline: {:?}",
                    result
                )));
            }

            Ok(GraphicsPipeline {
                pipeline,
                layout: layout.vk_layout(),
                device: context.device,
            })
        }
    }

    /// Get the Vulkan pipeline handle
    pub fn vk_pipeline(&self) -> crate::VkPipeline {
        self.pipeline
    }

    /// Get the pipeline layout
    pub fn vk_layout(&self) -> crate::VkPipelineLayout {
        self.layout
    }
}

impl Drop for GraphicsPipeline {
    fn drop(&mut self) {
        unsafe {
            crate::vkDestroyPipeline(self.device, self.pipeline, std::ptr::null());
        }
    }
}

/// A compute pipeline for compute shaders
pub struct ComputePipeline {
    pipeline: crate::VkPipeline,
    layout: crate::VkPipelineLayout,
    device: crate::VkDevice,
}

impl ComputePipeline {
    /// Create a compute pipeline from a shader module
    pub fn new(
        context: &GraphicsContext,
        shader: &ShaderModule,
        layout: &PipelineLayout,
        specialization: Option<&SpecializationConstants>,
    ) -> Result<Self> {
        use std::ptr;

        unsafe {
            let specialization_info = specialization.and_then(|s| s.build());
            let stage_info = crate::VkPipelineShaderStageCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                stage: crate::VkShaderStageFlagBits::VK_SHADER_STAGE_COMPUTE_BIT,
                module: shader.vk_module(),
                pName: b"main\0".as_ptr() as *const i8,
                pSpecializationInfo: specialization_info
                    .as_ref()
                    .map_or(std::ptr::null(), |info| info as *const _),
            };

            let pipeline_info = crate::VkComputePipelineCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                stage: stage_info,
                layout: layout.vk_layout(),
                basePipelineHandle: std::ptr::null_mut(),
                basePipelineIndex: -1,
            };

            let mut pipeline = std::ptr::null_mut();
            let result = crate::vkCreateComputePipelines(
                context.device,
                std::ptr::null_mut(),
                1,
                &pipeline_info,
                ptr::null(),
                &mut pipeline,
            );

            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create compute pipeline: {:?}",
                    result
                )));
            }

            Ok(ComputePipeline {
                pipeline,
                layout: layout.vk_layout(),
                device: context.device,
            })
        }
    }

    /// Get the Vulkan pipeline handle
    pub fn vk_pipeline(&self) -> crate::VkPipeline {
        self.pipeline
    }

    /// Get the pipeline layout
    pub fn vk_layout(&self) -> crate::VkPipelineLayout {
        self.layout
    }
}

impl Drop for ComputePipeline {
    fn drop(&mut self) {
        unsafe {
            crate::vkDestroyPipeline(self.device, self.pipeline, std::ptr::null());
        }
    }
}

/// Command buffer for recording rendering commands
pub struct CommandBuffer {
    buffer: crate::VkCommandBuffer,
    _device: crate::VkDevice,
    _command_pool: crate::VkCommandPool,
}

impl CommandBuffer {
    /// Allocate a new command buffer
    pub fn allocate(context: &GraphicsContext) -> Result<Self> {
        use std::ptr;

        unsafe {
            let alloc_info = crate::VkCommandBufferAllocateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
                pNext: ptr::null(),
                commandPool: context._command_pool,
                level: crate::VkCommandBufferLevel::VK_COMMAND_BUFFER_LEVEL_PRIMARY,
                commandBufferCount: 1,
            };

            let mut buffer = std::ptr::null_mut();
            let result = crate::vkAllocateCommandBuffers(context.device, &alloc_info, &mut buffer);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to allocate command buffer: {:?}",
                    result
                )));
            }

            Ok(CommandBuffer {
                buffer,
                _device: context.device,
                _command_pool: context._command_pool,
            })
        }
    }

    /// Begin recording commands
    pub fn begin(&self) -> Result<()> {
        use std::ptr;

        unsafe {
            let begin_info = crate::VkCommandBufferBeginInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
                pNext: ptr::null(),
                flags:
                    crate::VkCommandBufferUsageFlagBits::VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT
                        as u32,
                pInheritanceInfo: ptr::null(),
            };

            let result = crate::vkBeginCommandBuffer(self.buffer, &begin_info);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to begin command buffer: {:?}",
                    result
                )));
            }

            Ok(())
        }
    }

    /// End recording commands
    pub fn end(&self) -> Result<()> {
        unsafe {
            let result = crate::vkEndCommandBuffer(self.buffer);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to end command buffer: {:?}",
                    result
                )));
            }

            Ok(())
        }
    }

    /// Reset the command buffer to initial state (must be called between uses)
    pub fn reset(&self) -> Result<()> {
        unsafe {
            let result = crate::vkResetCommandBuffer(self.buffer, 0);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to reset command buffer: {:?}",
                    result
                )));
            }
            Ok(())
        }
    }

    /// Copy data between GPU buffers
    pub fn copy_buffer(&self, src: &GpuAllocation, dst: &GpuAllocation, size: usize) -> Result<()> {
        self.copy_vk_buffer(src.buffer(), dst.buffer(), size, 0, 0)
    }

    /// Copy data between Vulkan buffers.
    pub fn copy_vk_buffer(
        &self,
        src: crate::VkBuffer,
        dst: crate::VkBuffer,
        size: usize,
        src_offset: u64,
        dst_offset: u64,
    ) -> Result<()> {
        if size == 0 {
            return Err(Error::InvalidArgument);
        }
        unsafe {
            let copy_region = crate::VkBufferCopy {
                srcOffset: src_offset,
                dstOffset: dst_offset,
                size: size as u64,
            };
            crate::vkCmdCopyBuffer(self.buffer, src, dst, 1, &copy_region);
        }
        Ok(())
    }

    /// Bind a vertex buffer at a specific binding slot.
    pub fn bind_vertex_buffer(&self, binding: u32, buffer: &Buffer, offset: u64) {
        unsafe {
            let buffers = [buffer.vk_buffer()];
            let offsets = [offset];
            crate::vkCmdBindVertexBuffers(
                self.buffer,
                binding,
                1,
                buffers.as_ptr(),
                offsets.as_ptr(),
            );
        }
    }

    /// Bind an index buffer.
    pub fn bind_index_buffer(&self, buffer: &Buffer, offset: u64, index_type: IndexType) {
        unsafe {
            crate::vkCmdBindIndexBuffer(
                self.buffer,
                buffer.vk_buffer(),
                offset,
                index_type.to_vk(),
            );
        }
    }

    /// Set the dynamic scissor rect (requires VK_DYNAMIC_STATE_SCISSOR in the pipeline).
    pub fn set_scissor(&self, x: i32, y: i32, width: u32, height: u32) {
        let scissor = crate::VkRect2D {
            offset: crate::VkOffset2D { x, y },
            extent: crate::VkExtent2D { width, height },
        };
        unsafe {
            crate::vkCmdSetScissor(self.buffer, 0, 1, &scissor);
        }
    }

    /// Draw indexed primitives.
    pub fn draw_indexed(
        &self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) {
        unsafe {
            crate::vkCmdDrawIndexed(
                self.buffer,
                index_count,
                instance_count,
                first_index,
                vertex_offset,
                first_instance,
            );
        }
    }

    /// Insert a pipeline barrier between stages with optional hazard flags
    pub fn barrier(&self, src_stage: u32, dst_stage: u32, hazards: HazardFlags) -> Result<()> {
        // For now, hazard flags are ignored - they would be used for additional
        // cache invalidations (draw arguments, descriptors, depth stencil)
        // In a full implementation, these would translate to additional
        // Vulkan pipeline stage flags or memory barriers
        let _ = hazards; // Mark as used

        unsafe {
            crate::vkCmdPipelineBarrier(
                self.buffer,
                src_stage,
                dst_stage,
                0,
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
            );
        }
        Ok(())
    }

    /// Transition texture layout to TRANSFER_DST_OPTIMAL for copying data
    pub fn transition_to_transfer_dst(&self, texture: &Texture) {
        unsafe {
            let barrier = crate::VkImageMemoryBarrier {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
                pNext: std::ptr::null(),
                srcAccessMask: 0,
                dstAccessMask: crate::VkAccessFlagBits::VK_ACCESS_TRANSFER_WRITE_BIT as u32,
                oldLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_UNDEFINED,
                newLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                srcQueueFamilyIndex: crate::VK_QUEUE_FAMILY_IGNORED as u32,
                dstQueueFamilyIndex: crate::VK_QUEUE_FAMILY_IGNORED as u32,
                image: texture.vk_image(),
                subresourceRange: crate::VkImageSubresourceRange {
                    aspectMask: texture.format().aspect_mask(),
                    baseMipLevel: 0,
                    levelCount: 1,
                    baseArrayLayer: 0,
                    layerCount: 1,
                },
            };
            crate::vkCmdPipelineBarrier(
                self.buffer,
                crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT as u32,
                crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_TRANSFER_BIT as u32,
                0,
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                1,
                &barrier,
            );
        }
    }

    /// Transition texture layout from TRANSFER_DST_OPTIMAL to SHADER_READ_ONLY_OPTIMAL
    pub fn transition_to_shader_read(&self, texture: &Texture) {
        unsafe {
            let barrier = crate::VkImageMemoryBarrier {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
                pNext: std::ptr::null(),
                srcAccessMask: crate::VkAccessFlagBits::VK_ACCESS_TRANSFER_WRITE_BIT as u32,
                dstAccessMask: crate::VkAccessFlagBits::VK_ACCESS_SHADER_READ_BIT as u32,
                oldLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                newLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                srcQueueFamilyIndex: crate::VK_QUEUE_FAMILY_IGNORED as u32,
                dstQueueFamilyIndex: crate::VK_QUEUE_FAMILY_IGNORED as u32,
                image: texture.vk_image(),
                subresourceRange: crate::VkImageSubresourceRange {
                    aspectMask: texture.format().aspect_mask(),
                    baseMipLevel: 0,
                    levelCount: 1,
                    baseArrayLayer: 0,
                    layerCount: 1,
                },
            };
            crate::vkCmdPipelineBarrier(
                self.buffer,
                crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_TRANSFER_BIT as u32,
                crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT as u32,
                0,
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                1,
                &barrier,
            );
        }
    }

    /// Copy buffer data to texture with automatic layout transitions
    /// This handles the optimal upload path with DCC compression support
    pub fn copy_to_texture(
        &self,
        src_data: &[u8],
        dst_texture: &Texture,
        width: u32,
        height: u32,
        format: Format,
    ) -> Result<()> {
        // Calculate required buffer size
        let pixel_size = match format {
            Format::Rgba8Unorm | Format::Bgra8Unorm => 4,
            Format::Rgba32Float => 16,
            Format::Depth32Float => 4,
        };

        let required_size = (width as usize) * (height as usize) * pixel_size;
        if src_data.len() < required_size {
            return Err(Error::InvalidArgument);
        }

        // Transition texture to TRANSFER_DST_OPTIMAL
        self.transition_to_transfer_dst(dst_texture);

        // Create staging buffer
        // Note: In a real implementation, we would use a staging buffer pool
        // For simplicity, we create a new allocation each time
        println!("Texture upload with DCC compression not yet implemented - using simple copy");

        Ok(())
    }

    /// Copy buffer data to texture (texture must be in TRANSFER_DST_OPTIMAL layout)
    pub fn copy_buffer_to_texture(
        &self,
        src_buffer: &GpuAllocation,
        dst_texture: &Texture,
        width: u32,
        height: u32,
    ) {
        unsafe {
            let region = crate::VkBufferImageCopy {
                bufferOffset: 0,
                bufferRowLength: 0,
                bufferImageHeight: 0,
                imageSubresource: crate::VkImageSubresourceLayers {
                    aspectMask: dst_texture.format().aspect_mask(),
                    mipLevel: 0,
                    baseArrayLayer: 0,
                    layerCount: 1,
                },
                imageOffset: crate::VkOffset3D { x: 0, y: 0, z: 0 },
                imageExtent: crate::VkExtent3D {
                    width,
                    height,
                    depth: 1,
                },
            };
            crate::vkCmdCopyBufferToImage(
                self.buffer,
                src_buffer.buffer(),
                dst_texture.vk_image(),
                crate::VkImageLayout::VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                1,
                &region,
            );
        }
    }

    /// Begin a render pass
    pub fn begin_render_pass(
        &self,
        render_pass: crate::VkRenderPass,
        framebuffer: crate::VkFramebuffer,
        width: u32,
        height: u32,
        clear_color: [f32; 4],
    ) {
        unsafe {
            // Prepare clear values for both color and depth attachments
            let clear_values = [
                crate::VkClearValue {
                    color: crate::VkClearColorValue {
                        float32: [
                            clear_color[0],
                            clear_color[1],
                            clear_color[2],
                            clear_color[3],
                        ],
                    },
                },
                crate::VkClearValue {
                    depthStencil: crate::VkClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                },
            ];

            let render_pass_begin = crate::VkRenderPassBeginInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
                pNext: std::ptr::null(),
                renderPass: render_pass,
                framebuffer,
                renderArea: crate::VkRect2D {
                    offset: crate::VkOffset2D { x: 0, y: 0 },
                    extent: crate::VkExtent2D { width, height },
                },
                clearValueCount: 2,
                pClearValues: clear_values.as_ptr(),
            };

            crate::vkCmdBeginRenderPass(
                self.buffer,
                &render_pass_begin,
                crate::VkSubpassContents::VK_SUBPASS_CONTENTS_INLINE,
            );
        }
    }

    /// End a render pass
    pub fn end_render_pass(&self) {
        unsafe {
            crate::vkCmdEndRenderPass(self.buffer);
        }
    }

    /// Bind a graphics pipeline
    pub fn bind_pipeline(&self, pipeline: &GraphicsPipeline) {
        unsafe {
            crate::vkCmdBindPipeline(
                self.buffer,
                crate::VkPipelineBindPoint::VK_PIPELINE_BIND_POINT_GRAPHICS,
                pipeline.vk_pipeline(),
            );
        }
    }

    /// Bind descriptor sets for graphics pipeline
    pub fn bind_descriptor_sets(
        &self,
        layout: &PipelineLayout,
        first_set: u32,
        sets: &[&DescriptorSet],
    ) {
        unsafe {
            let vk_sets: Vec<crate::VkDescriptorSet> = sets.iter().map(|s| s.vk_set()).collect();
            crate::vkCmdBindDescriptorSets(
                self.buffer,
                crate::VkPipelineBindPoint::VK_PIPELINE_BIND_POINT_GRAPHICS,
                layout.vk_layout(),
                first_set,
                vk_sets.len() as u32,
                vk_sets.as_ptr(),
                0,
                std::ptr::null(),
            );
        }
    }

    /// Bind a compute pipeline
    pub fn bind_compute_pipeline(&self, pipeline: &ComputePipeline) {
        unsafe {
            crate::vkCmdBindPipeline(
                self.buffer,
                crate::VkPipelineBindPoint::VK_PIPELINE_BIND_POINT_COMPUTE,
                pipeline.vk_pipeline(),
            );
        }
    }

    /// Set push constants (for root pointer)
    pub fn push_constants(&self, layout: &PipelineLayout, data: &[u8]) {
        let stage_flags = layout.push_constant_stage_flags();
        if stage_flags == 0 {
            return; // No push constants in this layout
        }

        unsafe {
            crate::vkCmdPushConstants(
                self.buffer,
                layout.vk_layout(),
                stage_flags,
                0,
                data.len() as u32,
                data.as_ptr() as *const _,
            );
        }
    }

    /// Push separate root pointers for vertex and fragment shaders
    /// Layout must be created with with_separate_root_arguments (16-byte push constants)
    pub fn push_separate_root_pointers(
        &self,
        layout: &PipelineLayout,
        vertex_ptr: u64,
        fragment_ptr: u64,
    ) {
        let stage_flags = layout.push_constant_stage_flags();
        if stage_flags == 0 {
            return; // No push constants in this layout
        }

        // Pack both pointers into 16 bytes
        let mut data = [0u8; 16];
        data[0..8].copy_from_slice(&vertex_ptr.to_ne_bytes());
        data[8..16].copy_from_slice(&fragment_ptr.to_ne_bytes());

        unsafe {
            crate::vkCmdPushConstants(
                self.buffer,
                layout.vk_layout(),
                stage_flags,
                0,
                16,
                data.as_ptr() as *const _,
            );
        }
    }

    /// Bind a descriptor heap for graphics or compute pipeline
    /// Note: This is a placeholder implementation. Descriptor buffer extension
    /// (VK_EXT_descriptor_buffer) is not universally supported. Use root arguments
    /// and standard descriptor sets instead.
    /// Bind a descriptor buffer to the command buffer
    /// This enables bindless resource access via VK_EXT_descriptor_buffer
    pub fn bind_descriptor_buffer(
        &self,
        heap: &DescriptorHeap,
        _bind_point: crate::VkPipelineBindPoint,
    ) {
        unsafe {
            let binding_info = crate::VkDescriptorBufferBindingInfoEXT {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DESCRIPTOR_BUFFER_BINDING_INFO_EXT,
                pNext: ptr::null(),
                address: heap.device_address(),
                usage:
                    crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_RESOURCE_DESCRIPTOR_BUFFER_BIT_EXT
                        as u32,
            };

            let _ = vk_cmd_bind_descriptor_buffers_ext_dynamic(
                self._device,
                self.buffer,
                1,
                &binding_info,
            );
        }
    }

    /// Set the descriptor buffer offset for a specific set
    /// Call this after bind_descriptor_buffer to select which descriptors to use
    pub fn set_descriptor_buffer_offset(
        &self,
        layout: &PipelineLayout,
        set_index: u32,
        offset: u64,
        bind_point: crate::VkPipelineBindPoint,
    ) {
        unsafe {
            let buffer_index = 0u32; // We bind one descriptor buffer at a time
            let _ = vk_cmd_set_descriptor_buffer_offsets_ext_dynamic(
                self._device,
                self.buffer,
                bind_point,
                layout.vk_layout(),
                set_index,
                1,
                &buffer_index,
                &offset,
            );
        }
    }

    pub fn bind_descriptor_heap(
        &self,
        _heap: &DescriptorHeap,
        _layout: &PipelineLayout,
        _set_index: u32,
        _bind_point: crate::VkPipelineBindPoint,
    ) {
        // Deprecated in favor of bind_descriptor_buffer
        // This method is kept for backward compatibility
    }

    /// Dispatch compute with root pointer
    pub fn dispatch(
        &self,
        pipeline: &ComputePipeline,
        layout: &PipelineLayout,
        root_ptr: u64,
        group_count: [u32; 3],
    ) {
        self.bind_compute_pipeline(pipeline);
        // Pass 64-bit pointer as two 32-bit integers
        let data = [
            root_ptr as u32,         // low 32 bits
            (root_ptr >> 32) as u32, // high 32 bits
        ];
        let data_bytes = unsafe {
            std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(&data))
        };
        self.push_constants(layout, data_bytes);
        unsafe {
            crate::vkCmdDispatch(self.buffer, group_count[0], group_count[1], group_count[2]);
        }
    }

    /// Draw triangles
    pub fn draw(
        &self,
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    ) {
        unsafe {
            crate::vkCmdDraw(
                self.buffer,
                vertex_count,
                instance_count,
                first_vertex,
                first_instance,
            );
        }
    }

    /// Bind texture descriptor heap for bindless texturing (Graphics pipeline)
    /// This enables VK_EXT_descriptor_buffer for texture sampling
    pub fn bind_texture_heap_graphics(
        &self,
        heap: &TextureDescriptorHeap,
        layout: &PipelineLayout,
        set_index: u32,
    ) {
        unsafe {
            let binding_info = crate::VkDescriptorBufferBindingInfoEXT {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DESCRIPTOR_BUFFER_BINDING_INFO_EXT,
                pNext: ptr::null(),
                address: heap.gpu_address(),
                usage:
                    crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_SAMPLER_DESCRIPTOR_BUFFER_BIT_EXT
                        as u32,
            };

            let _ = vk_cmd_bind_descriptor_buffers_ext_dynamic(
                self._device,
                self.buffer,
                1,
                &binding_info,
            );

            // Set the offset for this descriptor set
            let buffer_index = 0u32;
            let offset = 0u64;
            let _ = vk_cmd_set_descriptor_buffer_offsets_ext_dynamic(
                self._device,
                self.buffer,
                crate::VkPipelineBindPoint::VK_PIPELINE_BIND_POINT_GRAPHICS,
                layout.vk_layout(),
                set_index,
                1,
                &buffer_index,
                &offset,
            );
        }
    }

    /// Bind texture descriptor heap for bindless texturing (Compute pipeline)
    pub fn bind_texture_heap_compute(
        &self,
        heap: &TextureDescriptorHeap,
        layout: &PipelineLayout,
        set_index: u32,
    ) {
        unsafe {
            let binding_info = crate::VkDescriptorBufferBindingInfoEXT {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DESCRIPTOR_BUFFER_BINDING_INFO_EXT,
                pNext: ptr::null(),
                address: heap.gpu_address(),
                usage:
                    crate::VkBufferUsageFlagBits::VK_BUFFER_USAGE_SAMPLER_DESCRIPTOR_BUFFER_BIT_EXT
                        as u32,
            };

            let _ = vk_cmd_bind_descriptor_buffers_ext_dynamic(
                self._device,
                self.buffer,
                1,
                &binding_info,
            );

            // Set the offset for this descriptor set
            let buffer_index = 0u32;
            let offset = 0u64;
            let _ = vk_cmd_set_descriptor_buffer_offsets_ext_dynamic(
                self._device,
                self.buffer,
                crate::VkPipelineBindPoint::VK_PIPELINE_BIND_POINT_COMPUTE,
                layout.vk_layout(),
                set_index,
                1,
                &buffer_index,
                &offset,
            );
        }
    }

    /// Bind texture descriptor heap for bindless texturing
    /// Sets the descriptor buffer for combined image samplers
    /// Note: This is a conceptual implementation. In a real implementation,
    /// we would use vkCmdBindDescriptorBuffersEXT and vkCmdSetDescriptorBufferOffsetsEXT.
    pub fn bind_texture_heap(
        &self,
        heap: &TextureDescriptorHeap,
        _layout: &PipelineLayout,
        set_index: u32,
    ) {
        println!(
            "   [Conceptual] Binding texture heap at set {} with GPU address: 0x{:x}",
            set_index,
            heap.gpu_address()
        );
        // In a real implementation, we would:
        // 1. Create VkDescriptorBufferBindingInfoEXT
        // 2. Call vkCmdBindDescriptorBuffersEXT
        // 3. Call vkCmdSetDescriptorBufferOffsetsEXT
    }

    /// Set root arguments for compute shader
    /// Passes 64-bit pointer as two 32-bit integers (lo, hi)
    pub fn set_compute_root_arguments(&self, layout: &PipelineLayout, root_args: &RootArguments) {
        if layout.push_constant_size() >= 8 {
            let addr = root_args.gpu_address();
            let data = [
                addr as u32,         // low 32 bits
                (addr >> 32) as u32, // high 32 bits
            ];
            let data_bytes = unsafe {
                std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(&data))
            };
            self.push_constants(layout, data_bytes);
        }
    }

    /// Set root arguments for graphics pipeline
    /// Passes 64-bit pointer as two 32-bit integers (lo, hi)
    pub fn set_graphics_root_arguments(&self, layout: &PipelineLayout, root_args: &RootArguments) {
        if layout.push_constant_size() >= 8 {
            let addr = root_args.gpu_address();
            let data = [
                addr as u32,         // low 32 bits
                (addr >> 32) as u32, // high 32 bits
            ];
            let data_bytes = unsafe {
                std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(&data))
            };
            self.push_constants(layout, data_bytes);
        }
    }

    /// Get the Vulkan command buffer handle
    pub fn vk_buffer(&self) -> crate::VkCommandBuffer {
        self.buffer
    }
}

/// Descriptor heap for bindless textures
pub struct DescriptorHeap {
    buffer: GpuAllocation,
    descriptor_size: usize,
    #[allow(dead_code)]
    descriptor_alignment: usize,
    count: usize,
    capacity: usize,
    device: crate::VkDevice,
    image_views: Vec<crate::VkImageView>,
}

impl DescriptorHeap {
    /// Create a new descriptor heap with given capacity
    pub fn new(context: &GraphicsContext, capacity: usize) -> Result<Self> {
        use std::ptr;

        if !context.descriptor_buffer_supported() {
            return Err(Error::Unsupported);
        }

        unsafe {
            // Get descriptor buffer properties
            let mut properties: crate::VkPhysicalDeviceDescriptorBufferPropertiesEXT =
                std::mem::zeroed();
            properties.sType = crate::VkStructureType::VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DESCRIPTOR_BUFFER_PROPERTIES_EXT;
            properties.pNext = ptr::null_mut();

            let mut properties2: crate::VkPhysicalDeviceProperties2 = std::mem::zeroed();
            properties2.sType =
                crate::VkStructureType::VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2;
            properties2.pNext = &mut properties as *mut _ as *mut std::ffi::c_void;

            crate::vkGetPhysicalDeviceProperties2(context._physical_device, &mut properties2);

            let descriptor_size = properties.combinedImageSamplerDescriptorSize as usize;
            // Use the required alignment for descriptor buffer offsets
            let descriptor_alignment = properties.descriptorBufferOffsetAlignment as usize;

            // Create buffer for descriptor data
            // We need host-visible memory for CPU updates
            let buffer_size = descriptor_size * capacity;
            let buffer =
                context.gpu_malloc(buffer_size, descriptor_alignment, MemoryType::CpuMapped)?;

            Ok(DescriptorHeap {
                buffer,
                descriptor_size,
                descriptor_alignment,
                count: 0,
                capacity,
                device: context.device,
                image_views: Vec::new(),
            })
        }
    }

    /// Get the device address of the descriptor heap buffer
    pub fn device_address(&self) -> u64 {
        self.buffer.gpu_ptr
    }

    /// Add a texture to the heap and return its index
    pub fn add_texture(&mut self, texture: &Texture, sampler: crate::VkSampler) -> Result<u32> {
        if self.count >= self.capacity {
            return Err(Error::OutOfMemory);
        }

        unsafe {
            // Create image view for the texture
            let view_info = crate::VkImageViewCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                image: texture.image,
                viewType: crate::VkImageViewType::VK_IMAGE_VIEW_TYPE_2D,
                format: texture.format.to_vk_format(),
                components: crate::VkComponentMapping {
                    r: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    g: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    b: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    a: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                },
                subresourceRange: crate::VkImageSubresourceRange {
                    aspectMask: crate::VkImageAspectFlagBits::VK_IMAGE_ASPECT_COLOR_BIT as u32,
                    baseMipLevel: 0,
                    levelCount: 1,
                    baseArrayLayer: 0,
                    layerCount: 1,
                },
            };

            let mut image_view: crate::VkImageView = ptr::null_mut();
            let result =
                crate::vkCreateImageView(self.device, &view_info, ptr::null(), &mut image_view);
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create image view: {:?}",
                    result
                )));
            }

            // Store image view for cleanup
            self.image_views.push(image_view);

            // Create descriptor image info
            let image_info = crate::VkDescriptorImageInfo {
                sampler,
                imageView: image_view,
                imageLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            };

            // Write descriptor to buffer
            let descriptor_info = crate::VkDescriptorGetInfoEXT {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DESCRIPTOR_GET_INFO_EXT,
                pNext: ptr::null(),
                type_: crate::VkDescriptorType::VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                data: crate::VkDescriptorDataEXT {
                    pCombinedImageSampler: &image_info as *const _,
                },
            };

            let offset = self.count * self.descriptor_size;
            let dest_ptr = self.buffer.cpu_ptr.add(offset) as *mut std::ffi::c_void;

            if !vk_get_descriptor_ext_dynamic(
                self.device,
                &descriptor_info,
                self.descriptor_size,
                dest_ptr,
            ) {
                crate::vkDestroyImageView(self.device, image_view, ptr::null());
                return Err(Error::Unsupported);
            }

            // Store image view for cleanup
            // TODO: Store image view for later destruction
            // For now, we leak it (in real implementation, store it)

            let index = self.count as u32;
            self.count += 1;
            Ok(index)
        }
    }

    /// Get GPU address of the descriptor heap
    pub fn gpu_address(&self) -> u64 {
        self.buffer.gpu_ptr
    }

    /// Get descriptor size (for shader indexing)
    pub fn descriptor_size(&self) -> usize {
        self.descriptor_size
    }

    /// Get current count of descriptors in the heap
    pub fn count(&self) -> usize {
        self.count
    }
}

/// Swapchain for presentation
pub struct Swapchain {
    swapchain: crate::VkSwapchainKHR,
    #[allow(dead_code)]
    images: Vec<crate::VkImage>,
    image_views: Vec<crate::VkImageView>,
    depth_image: crate::VkImage,
    depth_image_view: crate::VkImageView,
    depth_memory: crate::VkDeviceMemory,
    #[allow(dead_code)]
    depth_format: crate::VkFormat,
    render_pass: crate::VkRenderPass,
    framebuffers: Vec<crate::VkFramebuffer>,
    format: crate::VkFormat,
    extent: crate::VkExtent2D,
    device: crate::VkDevice,
    #[allow(dead_code)]
    graphics_queue: crate::VkQueue,
    present_queue: crate::VkQueue,
    // Per-swapchain-image semaphores signaled when rendering finishes for that image.
    // Using one semaphore per swapchain image prevents reusing a signal semaphore
    // for a different image while it may still be in use by the presentation operation.
    image_render_finished_semaphores: Vec<crate::VkSemaphore>,
    // Whether VK_KHR_swapchain_maintenance1 is supported by the device. When true,
    // we can present using a VkFence via the present pNext chain and avoid per-image semaphores.
    support_swapchain_maintenance1: bool,

    // Double buffering: 2 frames in flight
    frame_data: Vec<FrameData>,
    current_frame_index: usize,
    current_image_index: u32,
}

impl Swapchain {
    /// Create a new swapchain for the given window size
    pub fn new(
        context: &GraphicsContext,
        surface: crate::VkSurfaceKHR,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        use std::ptr;

        unsafe {
            // Get surface capabilities
            let mut capabilities =
                std::mem::MaybeUninit::<crate::VkSurfaceCapabilitiesKHR>::zeroed();
            let result = crate::vkGetPhysicalDeviceSurfaceCapabilitiesKHR(
                context._physical_device,
                surface,
                capabilities.as_mut_ptr(),
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to get surface capabilities: {:?}",
                    result
                )));
            }
            let capabilities = capabilities.assume_init();

            // Choose swapchain format
            let mut format_count = 0;
            let result = crate::vkGetPhysicalDeviceSurfaceFormatsKHR(
                context._physical_device,
                surface,
                &mut format_count,
                ptr::null_mut(),
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to get surface format count: {:?}",
                    result
                )));
            }

            let mut formats = Vec::with_capacity(format_count as usize);
            let result = crate::vkGetPhysicalDeviceSurfaceFormatsKHR(
                context._physical_device,
                surface,
                &mut format_count,
                formats.as_mut_ptr(),
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to get surface formats: {:?}",
                    result
                )));
            }
            formats.set_len(format_count as usize);

            // Prefer B8G8R8A8_UNORM format if available
            let format = formats
                .iter()
                .find(|f| f.format == crate::VkFormat::VK_FORMAT_B8G8R8A8_UNORM)
                .map(|f| f.format)
                .unwrap_or_else(|| {
                    if formats.is_empty() {
                        crate::VkFormat::VK_FORMAT_B8G8R8A8_UNORM
                    } else {
                        formats[0].format
                    }
                });

            // Choose present mode (FIFO is always available)
            let mut present_mode_count = 0;
            let result = crate::vkGetPhysicalDeviceSurfacePresentModesKHR(
                context._physical_device,
                surface,
                &mut present_mode_count,
                ptr::null_mut(),
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to get present mode count: {:?}",
                    result
                )));
            }

            let mut present_modes = Vec::with_capacity(present_mode_count as usize);
            let result = crate::vkGetPhysicalDeviceSurfacePresentModesKHR(
                context._physical_device,
                surface,
                &mut present_mode_count,
                present_modes.as_mut_ptr(),
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to get present modes: {:?}",
                    result
                )));
            }
            present_modes.set_len(present_mode_count as usize);

            // Prefer MAILBOX (triple buffering) if available, otherwise FIFO
            let present_mode = present_modes
                .iter()
                .find(|&&mode| mode == crate::VkPresentModeKHR::VK_PRESENT_MODE_MAILBOX_KHR)
                .copied()
                .unwrap_or(crate::VkPresentModeKHR::VK_PRESENT_MODE_FIFO_KHR);

            // Determine swapchain extent
            let extent = if capabilities.currentExtent.width != u32::MAX {
                capabilities.currentExtent
            } else {
                crate::VkExtent2D {
                    width: width.clamp(
                        capabilities.minImageExtent.width,
                        capabilities.maxImageExtent.width,
                    ),
                    height: height.clamp(
                        capabilities.minImageExtent.height,
                        capabilities.maxImageExtent.height,
                    ),
                }
            };

            // Determine image count
            let mut image_count = capabilities.minImageCount + 1;
            if capabilities.maxImageCount > 0 && image_count > capabilities.maxImageCount {
                image_count = capabilities.maxImageCount;
            }

            // Create swapchain
            let swapchain_info = crate::VkSwapchainCreateInfoKHR {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR,
                pNext: ptr::null(),
                flags: 0,
                surface,
                minImageCount: image_count,
                imageFormat: format,
                imageColorSpace: crate::VkColorSpaceKHR::VK_COLOR_SPACE_SRGB_NONLINEAR_KHR,
                imageExtent: extent,
                imageArrayLayers: 1,
                imageUsage: crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT as u32,
                imageSharingMode: crate::VkSharingMode::VK_SHARING_MODE_EXCLUSIVE,
                queueFamilyIndexCount: 0,
                pQueueFamilyIndices: ptr::null(),
                preTransform: capabilities.currentTransform,
                compositeAlpha:
                    crate::VkCompositeAlphaFlagBitsKHR::VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR,
                presentMode: present_mode,
                clipped: 1,
                oldSwapchain: ptr::null_mut(),
            };

            let mut swapchain: crate::VkSwapchainKHR = ptr::null_mut();
            let result = crate::vkCreateSwapchainKHR(
                context.device,
                &swapchain_info,
                ptr::null(),
                &mut swapchain,
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(Error::Vulkan(format!(
                    "Failed to create swapchain: {:?}",
                    result
                )));
            }

            // Get swapchain images
            let mut image_count_actual = 0;
            let result = crate::vkGetSwapchainImagesKHR(
                context.device,
                swapchain,
                &mut image_count_actual,
                ptr::null_mut(),
            );
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroySwapchainKHR(context.device, swapchain, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to get swapchain image count: {:?}",
                    result
                )));
            }

            let mut images = Vec::with_capacity(image_count_actual as usize);
            let result = crate::vkGetSwapchainImagesKHR(
                context.device,
                swapchain,
                &mut image_count_actual,
                images.as_mut_ptr(),
            );
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroySwapchainKHR(context.device, swapchain, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to get swapchain images: {:?}",
                    result
                )));
            }
            images.set_len(image_count_actual as usize);

            // Create image views
            let mut image_views = Vec::with_capacity(images.len());
            for &image in &images {
                let view_info = crate::VkImageViewCreateInfo {
                    sType: crate::VkStructureType::VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
                    pNext: ptr::null(),
                    flags: 0,
                    image,
                    viewType: crate::VkImageViewType::VK_IMAGE_VIEW_TYPE_2D,
                    format,
                    components: crate::VkComponentMapping {
                        r: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                        g: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                        b: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                        a: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    },
                    subresourceRange: crate::VkImageSubresourceRange {
                        aspectMask: crate::VkImageAspectFlagBits::VK_IMAGE_ASPECT_COLOR_BIT as u32,
                        baseMipLevel: 0,
                        levelCount: 1,
                        baseArrayLayer: 0,
                        layerCount: 1,
                    },
                };

                let mut image_view: crate::VkImageView = ptr::null_mut();
                let result = crate::vkCreateImageView(
                    context.device,
                    &view_info,
                    ptr::null(),
                    &mut image_view,
                );
                if result != crate::VkResult::VK_SUCCESS {
                    // Clean up already created image views
                    for &view in &image_views {
                        crate::vkDestroyImageView(context.device, view, ptr::null());
                    }
                    crate::vkDestroySwapchainKHR(context.device, swapchain, ptr::null());
                    return Err(Error::Vulkan(format!(
                        "Failed to create image view: {:?}",
                        result
                    )));
                }
                image_views.push(image_view);
            }

            // Create depth buffer
            let depth_format = crate::VkFormat::VK_FORMAT_D32_SFLOAT;

            let depth_image_info = crate::VkImageCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                imageType: crate::VkImageType::VK_IMAGE_TYPE_2D,
                format: depth_format,
                extent: crate::VkExtent3D {
                    width: extent.width,
                    height: extent.height,
                    depth: 1,
                },
                mipLevels: 1,
                arrayLayers: 1,
                samples: crate::VkSampleCountFlagBits::VK_SAMPLE_COUNT_1_BIT,
                tiling: crate::VkImageTiling::VK_IMAGE_TILING_OPTIMAL,
                usage: crate::VkImageUsageFlagBits::VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT
                    as u32,
                sharingMode: crate::VkSharingMode::VK_SHARING_MODE_EXCLUSIVE,
                queueFamilyIndexCount: 0,
                pQueueFamilyIndices: ptr::null(),
                initialLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_UNDEFINED,
            };

            let mut depth_image: crate::VkImage = ptr::null_mut();
            let result = crate::vkCreateImage(
                context.device,
                &depth_image_info,
                ptr::null(),
                &mut depth_image,
            );
            if result != crate::VkResult::VK_SUCCESS {
                // Clean up image views
                for &view in &image_views {
                    crate::vkDestroyImageView(context.device, view, ptr::null());
                }
                crate::vkDestroySwapchainKHR(context.device, swapchain, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to create depth image: {:?}",
                    result
                )));
            }

            // Get depth image memory requirements
            let mut depth_requirements: crate::VkMemoryRequirements = std::mem::zeroed();
            crate::vkGetImageMemoryRequirements(
                context.device,
                depth_image,
                &mut depth_requirements,
            );

            // Find GPU-local memory type for depth buffer
            let property_flags =
                crate::VkMemoryPropertyFlagBits::VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT as u32;
            let mut depth_memory_type_index = !0u32;
            for i in 0..context.memory_properties.memoryTypeCount {
                let properties = context.memory_properties.memoryTypes[i as usize].propertyFlags;
                if (properties & property_flags) == property_flags {
                    depth_memory_type_index = i;
                    break;
                }
            }

            if depth_memory_type_index == !0u32 {
                crate::vkDestroyImage(context.device, depth_image, ptr::null());
                for &view in &image_views {
                    crate::vkDestroyImageView(context.device, view, ptr::null());
                }
                crate::vkDestroySwapchainKHR(context.device, swapchain, ptr::null());
                return Err(Error::Unsupported);
            }

            // Allocate depth image memory
            let depth_alloc_info = crate::VkMemoryAllocateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                pNext: ptr::null(),
                allocationSize: depth_requirements.size,
                memoryTypeIndex: depth_memory_type_index,
            };

            let mut depth_memory: crate::VkDeviceMemory = ptr::null_mut();
            let result = crate::vkAllocateMemory(
                context.device,
                &depth_alloc_info,
                ptr::null(),
                &mut depth_memory,
            );
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyImage(context.device, depth_image, ptr::null());
                for &view in &image_views {
                    crate::vkDestroyImageView(context.device, view, ptr::null());
                }
                crate::vkDestroySwapchainKHR(context.device, swapchain, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to allocate depth image memory: {:?}",
                    result
                )));
            }

            // Bind depth image memory
            let result = crate::vkBindImageMemory(context.device, depth_image, depth_memory, 0);
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkFreeMemory(context.device, depth_memory, ptr::null());
                crate::vkDestroyImage(context.device, depth_image, ptr::null());
                for &view in &image_views {
                    crate::vkDestroyImageView(context.device, view, ptr::null());
                }
                crate::vkDestroySwapchainKHR(context.device, swapchain, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to bind depth image memory: {:?}",
                    result
                )));
            }

            // Create depth image view
            let depth_view_info = crate::VkImageViewCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                image: depth_image,
                viewType: crate::VkImageViewType::VK_IMAGE_VIEW_TYPE_2D,
                format: depth_format,
                components: crate::VkComponentMapping {
                    r: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    g: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    b: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                    a: crate::VkComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY,
                },
                subresourceRange: crate::VkImageSubresourceRange {
                    aspectMask: crate::VkImageAspectFlagBits::VK_IMAGE_ASPECT_DEPTH_BIT as u32,
                    baseMipLevel: 0,
                    levelCount: 1,
                    baseArrayLayer: 0,
                    layerCount: 1,
                },
            };

            let mut depth_image_view: crate::VkImageView = ptr::null_mut();
            let result = crate::vkCreateImageView(
                context.device,
                &depth_view_info,
                ptr::null(),
                &mut depth_image_view,
            );
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkFreeMemory(context.device, depth_memory, ptr::null());
                crate::vkDestroyImage(context.device, depth_image, ptr::null());
                for &view in &image_views {
                    crate::vkDestroyImageView(context.device, view, ptr::null());
                }
                crate::vkDestroySwapchainKHR(context.device, swapchain, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to create depth image view: {:?}",
                    result
                )));
            }
            // Create render pass with color and depth attachments
            let color_attachment = crate::VkAttachmentDescription {
                flags: 0,
                format,
                samples: crate::VkSampleCountFlagBits::VK_SAMPLE_COUNT_1_BIT,
                loadOp: crate::VkAttachmentLoadOp::VK_ATTACHMENT_LOAD_OP_CLEAR,
                storeOp: crate::VkAttachmentStoreOp::VK_ATTACHMENT_STORE_OP_STORE,
                stencilLoadOp: crate::VkAttachmentLoadOp::VK_ATTACHMENT_LOAD_OP_DONT_CARE,
                stencilStoreOp: crate::VkAttachmentStoreOp::VK_ATTACHMENT_STORE_OP_DONT_CARE,
                initialLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_UNDEFINED,
                finalLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
            };

            let depth_attachment = crate::VkAttachmentDescription {
                flags: 0,
                format: depth_format,
                samples: crate::VkSampleCountFlagBits::VK_SAMPLE_COUNT_1_BIT,
                loadOp: crate::VkAttachmentLoadOp::VK_ATTACHMENT_LOAD_OP_CLEAR,
                storeOp: crate::VkAttachmentStoreOp::VK_ATTACHMENT_STORE_OP_DONT_CARE,
                stencilLoadOp: crate::VkAttachmentLoadOp::VK_ATTACHMENT_LOAD_OP_DONT_CARE,
                stencilStoreOp: crate::VkAttachmentStoreOp::VK_ATTACHMENT_STORE_OP_DONT_CARE,
                initialLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_UNDEFINED,
                finalLayout: crate::VkImageLayout::VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
            };

            let color_attachment_ref = crate::VkAttachmentReference {
                attachment: 0,
                layout: crate::VkImageLayout::VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
            };

            let depth_attachment_ref = crate::VkAttachmentReference {
                attachment: 1,
                layout: crate::VkImageLayout::VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
            };

            let subpass = crate::VkSubpassDescription {
                flags: 0,
                pipelineBindPoint: crate::VkPipelineBindPoint::VK_PIPELINE_BIND_POINT_GRAPHICS,
                inputAttachmentCount: 0,
                pInputAttachments: ptr::null(),
                colorAttachmentCount: 1,
                pColorAttachments: &color_attachment_ref,
                pResolveAttachments: ptr::null(),
                pDepthStencilAttachment: &depth_attachment_ref,
                preserveAttachmentCount: 0,
                pPreserveAttachments: ptr::null(),
            };

            let subpass_dependency = crate::VkSubpassDependency {
                srcSubpass: crate::VK_SUBPASS_EXTERNAL as u32,
                dstSubpass: 0,
                srcStageMask:
                    (crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                        as u32
                        | crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT
                            as u32),
                dstStageMask:
                    (crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                        as u32
                        | crate::VkPipelineStageFlagBits::VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT
                            as u32),
                srcAccessMask: 0,
                dstAccessMask: (crate::VkAccessFlagBits::VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT
                    as u32
                    | crate::VkAccessFlagBits::VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT as u32),
                dependencyFlags: 0,
            };

            let attachments = [color_attachment, depth_attachment];
            let render_pass_info = crate::VkRenderPassCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,
                attachmentCount: 2,
                pAttachments: attachments.as_ptr(),
                subpassCount: 1,
                pSubpasses: &subpass,
                dependencyCount: 1,
                pDependencies: &subpass_dependency,
            };

            let mut render_pass: crate::VkRenderPass = ptr::null_mut();
            let result = crate::vkCreateRenderPass(
                context.device,
                &render_pass_info,
                ptr::null(),
                &mut render_pass,
            );
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyImageView(context.device, depth_image_view, ptr::null());
                crate::vkFreeMemory(context.device, depth_memory, ptr::null());
                crate::vkDestroyImage(context.device, depth_image, ptr::null());
                for &view in &image_views {
                    crate::vkDestroyImageView(context.device, view, ptr::null());
                }
                crate::vkDestroySwapchainKHR(context.device, swapchain, ptr::null());
                return Err(Error::Vulkan(format!(
                    "Failed to create render pass: {:?}",
                    result
                )));
            }

            // Create framebuffers with both color and depth attachments
            let mut framebuffers = Vec::with_capacity(image_views.len());
            for &image_view in &image_views {
                let attachments = [image_view, depth_image_view];
                let framebuffer_info = crate::VkFramebufferCreateInfo {
                    sType: crate::VkStructureType::VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
                    pNext: ptr::null(),
                    flags: 0,
                    renderPass: render_pass,
                    attachmentCount: 2,
                    pAttachments: attachments.as_ptr(),
                    width: extent.width,
                    height: extent.height,
                    layers: 1,
                };

                let mut framebuffer: crate::VkFramebuffer = ptr::null_mut();
                let result = crate::vkCreateFramebuffer(
                    context.device,
                    &framebuffer_info,
                    ptr::null(),
                    &mut framebuffer,
                );
                if result != crate::VkResult::VK_SUCCESS {
                    // Clean up already created framebuffers
                    for &fb in &framebuffers {
                        crate::vkDestroyFramebuffer(context.device, fb, ptr::null());
                    }
                    crate::vkDestroyRenderPass(context.device, render_pass, ptr::null());
                    crate::vkDestroyImageView(context.device, depth_image_view, ptr::null());
                    crate::vkFreeMemory(context.device, depth_memory, ptr::null());
                    crate::vkDestroyImage(context.device, depth_image, ptr::null());
                    for &view in &image_views {
                        crate::vkDestroyImageView(context.device, view, ptr::null());
                    }
                    crate::vkDestroySwapchainKHR(context.device, swapchain, ptr::null());
                    return Err(Error::Vulkan(format!(
                        "Failed to create framebuffer: {:?}",
                        result
                    )));
                }
                framebuffers.push(framebuffer);
            }

            // Detect VK_KHR_swapchain_maintenance1 support on the device.
            // If supported we can use a present-fence path and avoid allocating per-image semaphores.
            let mut ext_count: u32 = 0;
            let mut support_swapchain_maintenance1 = false;
            unsafe {
                let mut result = crate::vkEnumerateDeviceExtensionProperties(
                    context._physical_device,
                    std::ptr::null(),
                    &mut ext_count,
                    std::ptr::null_mut(),
                );
                if result == crate::VkResult::VK_SUCCESS && ext_count > 0 {
                    let mut exts: Vec<crate::VkExtensionProperties> =
                        Vec::with_capacity(ext_count as usize);
                    result = crate::vkEnumerateDeviceExtensionProperties(
                        context._physical_device,
                        std::ptr::null(),
                        &mut ext_count,
                        exts.as_mut_ptr(),
                    );
                    if result == crate::VkResult::VK_SUCCESS {
                        exts.set_len(ext_count as usize);
                        for ext in &exts {
                            // extensionName is a C string buffer; convert it to Rust &str safely.
                            let name_ptr = ext.extensionName.as_ptr() as *const i8;
                            if let Ok(name) = std::ffi::CStr::from_ptr(name_ptr).to_str() {
                                if name == "VK_KHR_swapchain_maintenance1" {
                                    support_swapchain_maintenance1 = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            // Create per-image render-finished semaphores only if the maintenance1 extension
            // is not available. When maintenance1 is available we will use present-fence.
            let mut image_render_finished_semaphores: Vec<crate::VkSemaphore> =
                Vec::with_capacity(images.len());
            if !support_swapchain_maintenance1 {
                for _ in 0..images.len() {
                    // Use the context helper to create semaphores; propagate error if creation fails
                    let sem = context.create_semaphore()?;
                    image_render_finished_semaphores.push(sem);
                }
            }

            // Create double-buffering frame data (2 frames in flight)
            let frame_data = vec![FrameData::create(context)?, FrameData::create(context)?];

            Ok(Swapchain {
                swapchain,
                images,
                image_views,
                depth_image,
                depth_image_view,
                depth_memory,
                depth_format,
                render_pass,
                framebuffers,
                format,
                extent,
                device: context.device,
                graphics_queue: context._graphics_queue,
                present_queue: context._present_queue,
                // Whether the maintenance1 present-fence path is available on this device
                support_swapchain_maintenance1,

                image_render_finished_semaphores,
                frame_data,
                current_frame_index: 0,
                current_image_index: 0,
            })
        }
    }

    /// Get the render pass for this swapchain
    pub fn render_pass(&self) -> crate::VkRenderPass {
        self.render_pass
    }

    /// Get the swapchain format
    pub fn format(&self) -> crate::VkFormat {
        self.format
    }

    /// Get the swapchain extent (width, height)
    pub fn extent(&self) -> crate::VkExtent2D {
        self.extent
    }

    /// Acquire the next image from the swapchain
    pub fn acquire_next_image(&self, semaphore: crate::VkSemaphore) -> Result<u32> {
        unsafe {
            let mut image_index = 0;
            let result = crate::vkAcquireNextImageKHR(
                self.device,
                self.swapchain,
                u64::MAX,
                semaphore,
                std::ptr::null_mut(),
                &mut image_index,
            );
            match result {
                crate::VkResult::VK_SUCCESS | crate::VkResult::VK_SUBOPTIMAL_KHR => Ok(image_index),
                crate::VkResult::VK_ERROR_OUT_OF_DATE_KHR => {
                    // Caller should recreate the swapchain
                    Err(Error::Vulkan("Swapchain out of date".to_string()))
                }
                _ => Err(Error::Vulkan(format!(
                    "Failed to acquire next image: {:?}",
                    result
                ))),
            }
        }
    }

    /// Present an image to the swapchain
    pub fn present(&self, image_index: u32, wait_semaphore: crate::VkSemaphore) -> Result<()> {
        unsafe {
            let swapchains = [self.swapchain];
            let image_indices = [image_index];
            let wait_semaphores = [wait_semaphore];

            let present_info = crate::VkPresentInfoKHR {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,
                pNext: ptr::null(),
                waitSemaphoreCount: 1,
                pWaitSemaphores: wait_semaphores.as_ptr(),
                swapchainCount: 1,
                pSwapchains: swapchains.as_ptr(),
                pImageIndices: image_indices.as_ptr(),
                pResults: ptr::null_mut(),
            };

            let result = crate::vkQueuePresentKHR(self.present_queue, &present_info);
            match result {
                crate::VkResult::VK_SUCCESS | crate::VkResult::VK_SUBOPTIMAL_KHR => Ok(()),
                crate::VkResult::VK_ERROR_OUT_OF_DATE_KHR => {
                    // Caller should recreate the swapchain
                    Err(Error::Vulkan("Swapchain out of date".to_string()))
                }
                _ => Err(Error::Vulkan(format!("Failed to present: {:?}", result))),
            }
        }
    }

    /// Present using a VkFence via VK_KHR_swapchain_maintenance1
    /// Requires that the device advertised support for the extension and that the
    /// provided fence is the same fence used for the submission for this frame
    /// (the application must ensure the fence is correctly reset prior to submit).
    pub fn present_with_fence(&self, image_index: u32, fence: crate::VkFence) -> Result<()> {
        use std::ptr;
        unsafe {
            let swapchains = [self.swapchain];
            let image_indices = [image_index];

            // Prepare the SwapchainPresentFenceInfoKHR with the fence to associate with this present.
            let mut swapchain_fence_info = SwapchainPresentFenceInfoKHR {
                sType: VK_STRUCTURE_TYPE_SWAPCHAIN_PRESENT_FENCE_INFO_KHR,
                pNext: ptr::null(),
                swapchainCount: 1,
                pFences: &fence as *const _,
            };

            let present_info = crate::VkPresentInfoKHR {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,
                pNext: &mut swapchain_fence_info as *mut _ as *const std::ffi::c_void,
                waitSemaphoreCount: 0,
                pWaitSemaphores: ptr::null(),
                swapchainCount: 1,
                pSwapchains: swapchains.as_ptr(),
                pImageIndices: image_indices.as_ptr(),
                pResults: ptr::null_mut(),
            };

            let result = crate::vkQueuePresentKHR(self.present_queue, &present_info);
            match result {
                crate::VkResult::VK_SUCCESS | crate::VkResult::VK_SUBOPTIMAL_KHR => Ok(()),
                crate::VkResult::VK_ERROR_OUT_OF_DATE_KHR => {
                    // Caller should recreate the swapchain
                    Err(Error::Vulkan("Swapchain out of date".to_string()))
                }
                _ => Err(Error::Vulkan(format!(
                    "Failed to present with fence: {:?}",
                    result
                ))),
            }
        }
    }

    /// Get framebuffer for given image index
    pub fn framebuffer(&self, image_index: u32) -> crate::VkFramebuffer {
        self.framebuffers[image_index as usize]
    }

    /// Get the number of images in the swapchain
    pub fn image_count(&self) -> u32 {
        self.images.len() as u32
    }

    /// Get the index of the currently acquired swapchain image
    pub fn current_image_index(&self) -> u32 {
        self.current_image_index
    }

    /// Get the command buffer for the current frame
    pub fn current_command_buffer(&self) -> &CommandBuffer {
        &self.frame_data[self.current_frame_index].command_buffer
    }

    /// Get the number of frames in flight (i.e. how many per-frame slots are available).
    /// This corresponds to the length of the `frame_data` vector and should be used
    /// for allocating per-frame transient resources (e.g. root arguments, per-frame UBOs).
    pub fn frames_in_flight(&self) -> usize {
        self.frame_data.len()
    }

    /// Get the current frame index (0 .. frames_in_flight-1).
    /// Use this to index per-frame resources safely — these slots are synchronized
    /// using the per-frame `Fence` in `FrameData`.
    pub fn current_frame_index(&self) -> usize {
        self.current_frame_index
    }

    /// Begin a new frame for rendering with automatic frame synchronization
    /// This method:
    /// - Waits for the previous GPU work on this frame to complete
    /// - Acquires the next swapchain image
    /// - Resets the command buffer for reuse
    /// - Returns Ok if successful
    pub fn begin_frame(&mut self) -> Result<()> {
        let current_frame = &self.frame_data[self.current_frame_index];

        // Wait for this frame's GPU work to complete before reusing it
        current_frame.wait()?;

        // Acquire next image from swapchain
        self.current_image_index =
            self.acquire_next_image(current_frame.image_available_semaphore)?;

        // Reset command buffer for reuse
        current_frame.command_buffer.reset()?;

        Ok(())
    }

    /// End the current frame and submit rendering to the GPU
    /// This method:
    /// - Submits the command buffer
    /// - Presents the rendered image to the screen
    /// - Advances to the next frame in the double-buffering rotation
    pub fn end_frame(&mut self, context: &GraphicsContext) -> Result<()> {
        let current_frame = &self.frame_data[self.current_frame_index];
        let image_index = self.current_image_index;

        // Determine whether we should use maintenance1; debug toggle can force-disable it.
        let use_maintenance1 = self.support_swapchain_maintenance1;
        if use_maintenance1 {
            // Submit using per-frame fence and no signal semaphore; present will use the fence via pNext.
            current_frame.submit(
                context,
                &[current_frame.image_available_semaphore],
                &[], // no signal semaphores when using present-fence
            )?;
            // Present using present-fence (per-frame fence)
            self.present_with_fence(image_index, current_frame.fence.raw())?;
        } else {
            // Use the per-swapchain-image render-finished semaphore for this acquired image.
            // This avoids signaling/reusing the same semaphore for different images while a
            // presentation operation may still reference it.
            let image_semaphore = self.image_render_finished_semaphores[image_index as usize];

            // Submit command buffer with semaphore synchronization (wait image-available, signal image-specific finished semaphore)
            current_frame.submit(
                context,
                &[current_frame.image_available_semaphore],
                &[image_semaphore],
            )?;

            // Present the image to the screen, waiting on the image-specific render-finished semaphore
            self.present(image_index, image_semaphore)?;
        }

        // Per-frame fences ensure that resources for the next frame are not reused
        // until GPU work for this frame has completed.
        self.current_frame_index = 1 - self.current_frame_index;

        Ok(())
    }
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        unsafe {
            for &framebuffer in &self.framebuffers {
                crate::vkDestroyFramebuffer(self.device, framebuffer, std::ptr::null());
            }
            crate::vkDestroyRenderPass(self.device, self.render_pass, std::ptr::null());
            for &image_view in &self.image_views {
                crate::vkDestroyImageView(self.device, image_view, ptr::null());
            }
            crate::vkDestroyImageView(self.device, self.depth_image_view, ptr::null());
            crate::vkFreeMemory(self.device, self.depth_memory, ptr::null());
            crate::vkDestroyImage(self.device, self.depth_image, ptr::null());

            // Destroy per-image render-finished semaphores allocated for this swapchain
            for &sem in &self.image_render_finished_semaphores {
                crate::vkDestroySemaphore(self.device, sem, std::ptr::null());
            }

            crate::vkDestroySwapchainKHR(self.device, self.swapchain, std::ptr::null());
        }
    }
}

impl Drop for DescriptorHeap {
    fn drop(&mut self) {
        unsafe {
            // Destroy image views
            for &image_view in &self.image_views {
                crate::vkDestroyImageView(self.device, image_view, std::ptr::null());
            }
            // Buffer is freed by GpuAllocation's Drop
        }
    }
}

impl Drop for CommandBuffer {
    fn drop(&mut self) {
        // Command buffers are freed when the pool is destroyed
    }
}

/// Data for a single frame in flight (supports multiple frames being rendered simultaneously)
pub struct FrameData {
    pub command_buffer: CommandBuffer,
    pub fence: Fence,
    pub image_available_semaphore: crate::VkSemaphore,
    pub render_finished_semaphore: crate::VkSemaphore,
    device: crate::VkDevice,
}

impl FrameData {
    /// Create a new frame data for double buffering (2 frames in flight)
    pub fn create(context: &GraphicsContext) -> Result<Self> {
        Ok(FrameData {
            command_buffer: CommandBuffer::allocate(context)?,
            fence: Fence::create(context)?,
            image_available_semaphore: context.create_semaphore()?,
            render_finished_semaphore: context.create_semaphore()?,
            device: context.device,
        })
    }

    /// Wait for this frame's GPU work to complete
    pub fn wait(&self) -> Result<()> {
        self.fence.wait_forever()
    }

    /// Reset the fence for the next frame
    pub fn reset_fence(&self, context: &GraphicsContext) -> Result<()> {
        self.fence.reset(context)
    }

    /// Submit this frame's command buffer with semaphore synchronization
    /// Uses the per-frame fence so the CPU is not blocked here and resources can
    /// be reused only after the fence is signaled on the next frame.
    pub fn submit(
        &self,
        context: &GraphicsContext,
        wait_semaphores: &[crate::VkSemaphore],
        signal_semaphores: &[crate::VkSemaphore],
    ) -> Result<()> {
        // Reset the per-frame fence before submitting (it was waited on at the start of the frame)
        self.reset_fence(context)?;

        // Submit using the existing per-frame fence (do not create a new fence)
        context.submit_with_fence(
            &self.command_buffer,
            wait_semaphores,
            signal_semaphores,
            self.fence.raw(),
        )?;

        // Do not wait here; allow GPU/CPU overlap. The fence will be waited on in begin_frame().
        Ok(())
    }
}

impl Drop for FrameData {
    fn drop(&mut self) {
        unsafe {
            crate::vkDestroySemaphore(
                self.device,
                self.image_available_semaphore,
                std::ptr::null(),
            );
            crate::vkDestroySemaphore(
                self.device,
                self.render_finished_semaphore,
                std::ptr::null(),
            );
        }
    }
}
