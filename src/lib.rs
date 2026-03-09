#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use libc::c_char;

mod raw {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use raw::*;

// Validation layers
const ENABLE_VALIDATION_LAYERS: bool = cfg!(debug_assertions);
const VK_EXT_DEBUG_UTILS_NAME: &str = "VK_EXT_debug_utils";
const VK_LAYER_KHRONOS_VALIDATION_NAME: &str = "VK_LAYER_KHRONOS_validation";
const VK_KHR_SWAPCHAIN_NAME: &str = "VK_KHR_swapchain";
const VK_KHR_SYNCHRONIZATION2_NAME: &str = "VK_KHR_synchronization2";
const VK_EXT_DESCRIPTOR_BUFFER_NAME: &str = "VK_EXT_descriptor_buffer";
const RAV_DISABLE_OPTIONAL_EXTENSIONS_ENV: &str = "RAV_DISABLE_OPTIONAL_EXTENSIONS";

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceCapabilities {
    pub descriptor_buffer_supported: bool,
    pub descriptor_buffer_capture_replay: bool,
    pub descriptor_buffer_image_layout_ignored: bool,
    pub descriptor_indexing_supported: bool,
}

fn env_var_is_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

unsafe fn enumerate_instance_extension_names() -> Result<Vec<String>, String> {
    let mut extension_count = 0;
    let result = crate::vkEnumerateInstanceExtensionProperties(
        std::ptr::null(),
        &mut extension_count,
        std::ptr::null_mut(),
    );
    if result != crate::VkResult::VK_SUCCESS {
        return Err(format!(
            "Failed to enumerate instance extensions: {:?}",
            result
        ));
    }

    let mut extension_properties = Vec::with_capacity(extension_count as usize);
    if extension_count > 0 {
        let enumerate_result = crate::vkEnumerateInstanceExtensionProperties(
            std::ptr::null(),
            &mut extension_count,
            extension_properties.as_mut_ptr(),
        );
        if enumerate_result != crate::VkResult::VK_SUCCESS {
            return Err(format!(
                "Failed to enumerate instance extensions: {:?}",
                enumerate_result
            ));
        }
        extension_properties.set_len(extension_count as usize);
    }

    Ok(extension_properties
        .iter()
        .map(|ext| {
            std::ffi::CStr::from_ptr(ext.extensionName.as_ptr() as *const c_char)
                .to_string_lossy()
                .into_owned()
        })
        .collect())
}

unsafe fn enumerate_instance_layer_names() -> Result<Vec<String>, String> {
    let mut layer_count = 0;
    let result = crate::vkEnumerateInstanceLayerProperties(&mut layer_count, std::ptr::null_mut());
    if result != crate::VkResult::VK_SUCCESS {
        return Err(format!("Failed to enumerate instance layers: {:?}", result));
    }

    let mut layer_properties = Vec::with_capacity(layer_count as usize);
    if layer_count > 0 {
        let enumerate_result = crate::vkEnumerateInstanceLayerProperties(
            &mut layer_count,
            layer_properties.as_mut_ptr(),
        );
        if enumerate_result != crate::VkResult::VK_SUCCESS {
            return Err(format!(
                "Failed to enumerate instance layers: {:?}",
                enumerate_result
            ));
        }
        layer_properties.set_len(layer_count as usize);
    }

    Ok(layer_properties
        .iter()
        .map(|layer| {
            std::ffi::CStr::from_ptr(layer.layerName.as_ptr() as *const c_char)
                .to_string_lossy()
                .into_owned()
        })
        .collect())
}

unsafe fn enumerate_device_extension_names(
    physical_device: crate::VkPhysicalDevice,
) -> Result<Vec<String>, String> {
    let mut extension_count = 0;
    let result = crate::vkEnumerateDeviceExtensionProperties(
        physical_device,
        std::ptr::null(),
        &mut extension_count,
        std::ptr::null_mut(),
    );
    if result != crate::VkResult::VK_SUCCESS {
        return Err(format!(
            "Failed to enumerate device extensions: {:?}",
            result
        ));
    }

    let mut extension_properties = Vec::with_capacity(extension_count as usize);
    if extension_count > 0 {
        let enumerate_result = crate::vkEnumerateDeviceExtensionProperties(
            physical_device,
            std::ptr::null(),
            &mut extension_count,
            extension_properties.as_mut_ptr(),
        );
        if enumerate_result != crate::VkResult::VK_SUCCESS {
            return Err(format!(
                "Failed to enumerate device extensions: {:?}",
                enumerate_result
            ));
        }
        extension_properties.set_len(extension_count as usize);
    }

    Ok(extension_properties
        .iter()
        .map(|ext| {
            std::ffi::CStr::from_ptr(ext.extensionName.as_ptr() as *const c_char)
                .to_string_lossy()
                .into_owned()
        })
        .collect())
}

unsafe extern "C" fn debug_callback(
    message_severity: crate::VkDebugUtilsMessageSeverityFlagBitsEXT,
    message_type: crate::VkDebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const crate::VkDebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut std::ffi::c_void,
) -> crate::VkBool32 {
    use crate::VkDebugUtilsMessageSeverityFlagBitsEXT as Severity;
    let severity = match message_severity {
        Severity::VK_DEBUG_UTILS_MESSAGE_SEVERITY_VERBOSE_BIT_EXT => "VERBOSE",
        Severity::VK_DEBUG_UTILS_MESSAGE_SEVERITY_INFO_BIT_EXT => "INFO",
        Severity::VK_DEBUG_UTILS_MESSAGE_SEVERITY_WARNING_BIT_EXT => "WARNING",
        Severity::VK_DEBUG_UTILS_MESSAGE_SEVERITY_ERROR_BIT_EXT => "ERROR",
        _ => "UNKNOWN",
    };
    let mut type_str = String::new();
    if message_type
        & crate::VkDebugUtilsMessageTypeFlagBitsEXT::VK_DEBUG_UTILS_MESSAGE_TYPE_GENERAL_BIT_EXT
            as u32
        != 0
    {
        type_str.push_str("GENERAL|");
    }
    if message_type
        & crate::VkDebugUtilsMessageTypeFlagBitsEXT::VK_DEBUG_UTILS_MESSAGE_TYPE_VALIDATION_BIT_EXT
            as u32
        != 0
    {
        type_str.push_str("VALIDATION|");
    }
    if message_type
        & crate::VkDebugUtilsMessageTypeFlagBitsEXT::VK_DEBUG_UTILS_MESSAGE_TYPE_PERFORMANCE_BIT_EXT
            as u32
        != 0
    {
        type_str.push_str("PERFORMANCE|");
    }
    if type_str.is_empty() {
        type_str = format!("UNKNOWN({})", message_type);
    } else {
        type_str.pop(); // remove trailing '|'
    }
    let callback_data = &*p_callback_data;
    let message = std::ffi::CStr::from_ptr(callback_data.pMessage);
    eprintln!("[{} {}] {}", severity, type_str, message.to_string_lossy());
    crate::VK_FALSE
}

// SDL window flags (not generated by bindgen)
pub const SDL_WINDOW_VULKAN: u64 = 0x0000000010000000;
pub const SDL_WINDOW_RESIZABLE: u64 = 0x0000000000000020;

// Vulkan version helper
pub fn VK_MAKE_VERSION(major: u32, minor: u32, patch: u32) -> u32 {
    (major << 22) | (minor << 12) | patch
}

// Simple API module
pub mod simple;

// egui integration
pub mod egui_manager;
pub mod egui_renderer;

// ECSS UDP Command Library
pub mod ecss_udp;

// ECSS Command Automation Framework
pub mod ecss_automation;

// Automation file loader
pub mod automation;

pub use egui_manager::EguiManager;
pub use egui_renderer::EguiRenderer;

pub struct SdlContext {
    _private: (),
}

impl SdlContext {
    pub fn init() -> Result<Self, String> {
        unsafe {
            if !SDL_Init(SDL_INIT_VIDEO) {
                return Err(format!("SDL_Init failed: {}", get_sdl_error()));
            }
        }
        Ok(SdlContext { _private: () })
    }
}

impl Drop for SdlContext {
    fn drop(&mut self) {
        unsafe {
            SDL_Quit();
        }
    }
}

pub struct VulkanInstance {
    pub instance: crate::VkInstance,
    debug_messenger: Option<crate::VkDebugUtilsMessengerEXT>,
}

impl VulkanInstance {
    pub fn create(_sdl: &SdlContext, _window: &SdlWindow) -> Result<Self, String> {
        unsafe {
            let mut count = 0;
            let extensions_ptr = SDL_Vulkan_GetInstanceExtensions(&mut count);
            if extensions_ptr.is_null() || count == 0 {
                return Err(format!(
                    "SDL_Vulkan_GetInstanceExtensions failed: {}",
                    get_sdl_error()
                ));
            }
            let sdl_extensions = std::slice::from_raw_parts(extensions_ptr, count as usize);
            let available_instance_extensions = enumerate_instance_extension_names()?;

            for &ext_ptr in sdl_extensions {
                let ext_name = std::ffi::CStr::from_ptr(ext_ptr)
                    .to_string_lossy()
                    .into_owned();
                if !available_instance_extensions
                    .iter()
                    .any(|name| name == &ext_name)
                {
                    return Err(format!(
                        "Required SDL instance extension '{}' is not available on this system",
                        ext_name
                    ));
                }
            }

            let mut enabled_extensions: Vec<*const i8> = sdl_extensions.to_vec();
            eprintln!("ENABLE_VALIDATION_LAYERS = {}", ENABLE_VALIDATION_LAYERS);
            eprintln!("debug_assertions = {}", cfg!(debug_assertions));

            let available_layers = enumerate_instance_layer_names()?;
            let validation_layer_available = available_layers
                .iter()
                .any(|name| name == VK_LAYER_KHRONOS_VALIDATION_NAME);
            let validation_enabled = ENABLE_VALIDATION_LAYERS && validation_layer_available;
            if ENABLE_VALIDATION_LAYERS && !validation_layer_available {
                eprintln!(
                    "Validation layer '{}' not found; continuing without validation layers",
                    VK_LAYER_KHRONOS_VALIDATION_NAME
                );
            }

            let debug_utils_available = available_instance_extensions
                .iter()
                .any(|name| name == VK_EXT_DEBUG_UTILS_NAME);

            // Add debug utils extension if validation layers enabled
            if validation_enabled && debug_utils_available {
                eprintln!("Adding {} extension", VK_EXT_DEBUG_UTILS_NAME);
                enabled_extensions.push(b"VK_EXT_debug_utils\0".as_ptr() as *const i8);
                eprintln!("Debug utils extension added");
            } else if validation_enabled {
                eprintln!(
                    "{} unavailable; validation will run without debug messenger",
                    VK_EXT_DEBUG_UTILS_NAME
                );
            }

            // Validation layers
            let enabled_layers = if validation_enabled {
                eprintln!("Enabling Vulkan validation layers");
                vec![b"VK_LAYER_KHRONOS_validation\0".as_ptr() as *const i8]
            } else {
                vec![]
            };

            let app_info = VkApplicationInfo {
                sType: VkStructureType::VK_STRUCTURE_TYPE_APPLICATION_INFO,
                pNext: std::ptr::null(),
                pApplicationName: b"Rust Vulkan App\0".as_ptr() as *const c_char,
                applicationVersion: VK_MAKE_VERSION(1, 0, 0),
                pEngineName: b"No Engine\0".as_ptr() as *const c_char,
                engineVersion: VK_MAKE_VERSION(1, 0, 0),
                apiVersion: VK_MAKE_VERSION(1, 2, 0),
            };

            let create_info = VkInstanceCreateInfo {
                sType: VkStructureType::VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
                pNext: std::ptr::null(),
                flags: 0,
                pApplicationInfo: &app_info,
                enabledLayerCount: enabled_layers.len() as u32,
                ppEnabledLayerNames: if enabled_layers.is_empty() {
                    std::ptr::null()
                } else {
                    enabled_layers.as_ptr()
                },
                enabledExtensionCount: enabled_extensions.len() as u32,
                ppEnabledExtensionNames: if enabled_extensions.is_empty() {
                    std::ptr::null()
                } else {
                    enabled_extensions.as_ptr()
                },
            };

            let mut instance = std::ptr::null_mut();
            let result = vkCreateInstance(&create_info, std::ptr::null(), &mut instance);
            if result != VkResult::VK_SUCCESS {
                return Err(format!("vkCreateInstance failed: {:?}", result));
            }
            let debug_messenger = if validation_enabled && debug_utils_available {
                Self::setup_debug_messenger(instance)
            } else {
                None
            };
            Ok(VulkanInstance {
                instance,
                debug_messenger,
            })
        }
    }
}

impl VulkanInstance {
    fn setup_debug_messenger(
        instance: crate::VkInstance,
    ) -> Option<crate::VkDebugUtilsMessengerEXT> {
        unsafe {
            // Load function pointer
            let create_fn_ptr = crate::vkGetInstanceProcAddr(
                instance,
                b"vkCreateDebugUtilsMessengerEXT\0".as_ptr() as *const i8,
            );
            if create_fn_ptr.is_none() {
                eprintln!("Failed to get vkCreateDebugUtilsMessengerEXT function pointer");
                return None;
            }
            let create_fn: crate::PFN_vkCreateDebugUtilsMessengerEXT =
                std::mem::transmute(create_fn_ptr);
            let create_fn = create_fn.expect("vkCreateDebugUtilsMessengerEXT is null");

            let create_info = crate::VkDebugUtilsMessengerCreateInfoEXT {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DEBUG_UTILS_MESSENGER_CREATE_INFO_EXT,
                pNext: std::ptr::null(),
                flags: 0,
                messageSeverity: crate::VkDebugUtilsMessageSeverityFlagBitsEXT::VK_DEBUG_UTILS_MESSAGE_SEVERITY_VERBOSE_BIT_EXT as u32
                    | crate::VkDebugUtilsMessageSeverityFlagBitsEXT::VK_DEBUG_UTILS_MESSAGE_SEVERITY_INFO_BIT_EXT as u32
                    | crate::VkDebugUtilsMessageSeverityFlagBitsEXT::VK_DEBUG_UTILS_MESSAGE_SEVERITY_WARNING_BIT_EXT as u32
                    | crate::VkDebugUtilsMessageSeverityFlagBitsEXT::VK_DEBUG_UTILS_MESSAGE_SEVERITY_ERROR_BIT_EXT as u32,
                messageType: crate::VkDebugUtilsMessageTypeFlagBitsEXT::VK_DEBUG_UTILS_MESSAGE_TYPE_GENERAL_BIT_EXT as u32
                    | crate::VkDebugUtilsMessageTypeFlagBitsEXT::VK_DEBUG_UTILS_MESSAGE_TYPE_VALIDATION_BIT_EXT as u32
                    | crate::VkDebugUtilsMessageTypeFlagBitsEXT::VK_DEBUG_UTILS_MESSAGE_TYPE_PERFORMANCE_BIT_EXT as u32,
                pfnUserCallback: Some(debug_callback),
                pUserData: std::ptr::null_mut(),
            };
            let mut messenger = std::ptr::null_mut();
            let result = create_fn(instance, &create_info, std::ptr::null(), &mut messenger);
            if result == crate::VkResult::VK_SUCCESS {
                Some(messenger)
            } else {
                eprintln!("Failed to create debug messenger: {:?}", result);
                None
            }
        }
    }
}

impl Drop for VulkanInstance {
    fn drop(&mut self) {
        unsafe {
            if let Some(messenger) = self.debug_messenger {
                let destroy_fn_ptr = crate::vkGetInstanceProcAddr(
                    self.instance,
                    b"vkDestroyDebugUtilsMessengerEXT\0".as_ptr() as *const i8,
                );
                if destroy_fn_ptr.is_none() {
                    eprintln!("Failed to get vkDestroyDebugUtilsMessengerEXT function pointer");
                } else {
                    let destroy_fn: crate::PFN_vkDestroyDebugUtilsMessengerEXT =
                        std::mem::transmute(destroy_fn_ptr);
                    let destroy_fn = destroy_fn.expect("vkDestroyDebugUtilsMessengerEXT is null");
                    destroy_fn(self.instance, messenger, std::ptr::null());
                }
            }
            vkDestroyInstance(self.instance, std::ptr::null());
        }
    }
}

pub struct VulkanSurface {
    pub surface: crate::VkSurfaceKHR,
    pub instance: crate::VkInstance,
}

impl VulkanSurface {
    pub fn create(window: &SdlWindow, instance: &VulkanInstance) -> Result<Self, String> {
        unsafe {
            let mut surface = std::ptr::null_mut();
            let success = crate::SDL_Vulkan_CreateSurface(
                window.window,
                instance.instance,
                std::ptr::null(),
                &mut surface,
            );
            if !success {
                return Err(format!(
                    "SDL_Vulkan_CreateSurface failed: {}",
                    get_sdl_error()
                ));
            }
            Ok(VulkanSurface {
                surface,
                instance: instance.instance,
            })
        }
    }
}

impl Drop for VulkanSurface {
    fn drop(&mut self) {
        unsafe {
            crate::SDL_Vulkan_DestroySurface(self.instance, self.surface, std::ptr::null());
        }
    }
}

pub struct VulkanDevice {
    pub surface: Option<VulkanSurface>,
    pub physical_device: crate::VkPhysicalDevice,
    pub device: crate::VkDevice,
    pub graphics_queue: crate::VkQueue,
    pub present_queue: crate::VkQueue,
    pub command_pool: crate::VkCommandPool,
    pub instance: VulkanInstance,
    pub capabilities: DeviceCapabilities,
    pub descriptor_buffer_supported: bool,
}

impl VulkanDevice {
    pub fn create(
        instance: VulkanInstance,
        surface: Option<VulkanSurface>,
    ) -> Result<Self, String> {
        eprintln!("VulkanDevice::create enter");
        unsafe {
            // Enumerate physical devices
            let mut device_count = 0;
            let result = crate::vkEnumeratePhysicalDevices(
                instance.instance,
                &mut device_count,
                std::ptr::null_mut(),
            );
            if result != crate::VkResult::VK_SUCCESS || device_count == 0 {
                return Err(format!(
                    "Failed to enumerate physical devices: {:?}",
                    result
                ));
            }
            eprintln!("Found {} physical devices", device_count);

            let mut physical_devices = Vec::with_capacity(device_count as usize);
            let result = crate::vkEnumeratePhysicalDevices(
                instance.instance,
                &mut device_count,
                physical_devices.as_mut_ptr(),
            );
            if result != crate::VkResult::VK_SUCCESS {
                return Err(format!("Failed to get physical devices: {:?}", result));
            }
            physical_devices.set_len(device_count as usize);

            // Select a suitable physical device.
            // Prefer discrete GPUs (e.g. RTX) over integrated adapters (e.g. Intel UHD),
            // but only among devices that can provide graphics + present queues.
            // Optional override: set VULKAN_DEVICE_NAME to a case-insensitive substring.
            let forced_device_name = std::env::var("VULKAN_DEVICE_NAME")
                .ok()
                .map(|v| v.to_ascii_lowercase());

            let mut selected_device: Option<crate::VkPhysicalDevice> = None;
            let mut selected_score: i32 = i32::MIN;
            let mut selected_name = String::new();

            for candidate in physical_devices {
                let mut props: crate::VkPhysicalDeviceProperties = std::mem::zeroed();
                crate::vkGetPhysicalDeviceProperties(candidate, &mut props);
                let name = std::ffi::CStr::from_ptr(props.deviceName.as_ptr() as *const c_char)
                    .to_string_lossy()
                    .into_owned();

                let mut queue_family_count = 0;
                crate::vkGetPhysicalDeviceQueueFamilyProperties(
                    candidate,
                    &mut queue_family_count,
                    std::ptr::null_mut(),
                );
                if queue_family_count == 0 {
                    continue;
                }
                let mut queue_families = Vec::with_capacity(queue_family_count as usize);
                crate::vkGetPhysicalDeviceQueueFamilyProperties(
                    candidate,
                    &mut queue_family_count,
                    queue_families.as_mut_ptr(),
                );
                queue_families.set_len(queue_family_count as usize);

                let has_graphics_queue = queue_families.iter().any(|props| {
                    props.queueFlags & (crate::VkQueueFlagBits::VK_QUEUE_GRAPHICS_BIT as u32) != 0
                });
                if !has_graphics_queue {
                    continue;
                }

                let has_present_queue = if let Some(ref surf) = surface {
                    let mut supported = false;
                    for i in 0..queue_families.len() {
                        let mut present_support = 0u32;
                        let result = crate::vkGetPhysicalDeviceSurfaceSupportKHR(
                            candidate,
                            i as u32,
                            surf.surface,
                            &mut present_support,
                        );
                        if result == crate::VkResult::VK_SUCCESS && present_support != 0 {
                            supported = true;
                            break;
                        }
                    }
                    supported
                } else {
                    true
                };
                if !has_present_queue {
                    continue;
                }

                let mut score = match props.deviceType {
                    crate::VkPhysicalDeviceType::VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU => 1000,
                    crate::VkPhysicalDeviceType::VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU => 600,
                    crate::VkPhysicalDeviceType::VK_PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU => 300,
                    crate::VkPhysicalDeviceType::VK_PHYSICAL_DEVICE_TYPE_CPU => 100,
                    _ => 50,
                };

                if let Some(ref forced) = forced_device_name {
                    if name.to_ascii_lowercase().contains(forced) {
                        score += 10_000;
                    } else {
                        score -= 2_000;
                    }
                }

                eprintln!("Physical device candidate: '{name}', score={score}");

                if score > selected_score {
                    selected_score = score;
                    selected_name = name;
                    selected_device = Some(candidate);
                }
            }

            let physical_device =
                selected_device.ok_or("No suitable Vulkan physical device found".to_string())?;
            eprintln!(
                "Selected physical device: '{}' (score={})",
                selected_name, selected_score
            );

            // Get queue family properties
            let mut queue_family_count = 0;
            crate::vkGetPhysicalDeviceQueueFamilyProperties(
                physical_device,
                &mut queue_family_count,
                std::ptr::null_mut(),
            );
            let mut queue_families = Vec::with_capacity(queue_family_count as usize);
            crate::vkGetPhysicalDeviceQueueFamilyProperties(
                physical_device,
                &mut queue_family_count,
                queue_families.as_mut_ptr(),
            );
            queue_families.set_len(queue_family_count as usize);

            // Find graphics queue family index
            let graphics_queue_family_index = queue_families
                .iter()
                .position(|props| {
                    props.queueFlags & (crate::VkQueueFlagBits::VK_QUEUE_GRAPHICS_BIT as u32) != 0
                })
                .ok_or("No graphics queue family found".to_string())?
                as u32;
            eprintln!(
                "Graphics queue family index: {}",
                graphics_queue_family_index
            );

            // Find present queue family index (if surface exists)
            let present_queue_family_index = if let Some(ref surf) = surface {
                let mut present_support = 0u32;
                let mut index = 0;
                for (i, _) in queue_families.iter().enumerate() {
                    let result = crate::vkGetPhysicalDeviceSurfaceSupportKHR(
                        physical_device,
                        i as u32,
                        surf.surface,
                        &mut present_support,
                    );
                    if result == crate::VkResult::VK_SUCCESS && present_support != 0 {
                        index = i as u32;
                        break;
                    }
                }
                if present_support == 0 {
                    return Err("No present queue family found".to_string());
                }
                index
            } else {
                graphics_queue_family_index // same as graphics if no surface
            };
            eprintln!("Present queue family index: {}", present_queue_family_index);

            let available_device_extensions = enumerate_device_extension_names(physical_device)?;
            let has_device_extension = |name: &str| {
                available_device_extensions
                    .iter()
                    .any(|extension_name| extension_name == name)
            };

            if surface.is_some() && !has_device_extension(VK_KHR_SWAPCHAIN_NAME) {
                return Err(format!(
                    "Selected physical device '{}' is missing required device extension '{}' for presentation",
                    selected_name, VK_KHR_SWAPCHAIN_NAME
                ));
            }

            let optional_extensions_disabled =
                env_var_is_truthy(RAV_DISABLE_OPTIONAL_EXTENSIONS_ENV);
            if optional_extensions_disabled {
                eprintln!(
                    "{}=1 detected, optional device extensions/features will be disabled",
                    RAV_DISABLE_OPTIONAL_EXTENSIONS_ENV
                );
            }

            let descriptor_buffer_extension_available =
                has_device_extension(VK_EXT_DESCRIPTOR_BUFFER_NAME);
            let synchronization2_extension_available =
                has_device_extension(VK_KHR_SYNCHRONIZATION2_NAME);

            let mut descriptor_buffer_features_query =
                crate::VkPhysicalDeviceDescriptorBufferFeaturesEXT {
                    sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DESCRIPTOR_BUFFER_FEATURES_EXT,
                    pNext: std::ptr::null_mut(),
                    descriptorBuffer: 0,
                    descriptorBufferCaptureReplay: 0,
                    descriptorBufferImageLayoutIgnored: 0,
                    descriptorBufferPushDescriptors: 0,
                };
            let mut vulkan12_features_query: crate::VkPhysicalDeviceVulkan12Features =
                std::mem::zeroed();
            vulkan12_features_query.sType =
                crate::VkStructureType::VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES;
            vulkan12_features_query.pNext = std::ptr::null_mut();

            descriptor_buffer_features_query.pNext =
                &mut vulkan12_features_query as *mut _ as *mut libc::c_void;

            let mut features2 = crate::VkPhysicalDeviceFeatures2 {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2,
                pNext: &mut descriptor_buffer_features_query as *mut _ as *mut libc::c_void,
                features: std::mem::zeroed(),
            };
            crate::vkGetPhysicalDeviceFeatures2(physical_device, &mut features2);

            if vulkan12_features_query.bufferDeviceAddress == 0 {
                return Err(format!(
                    "Selected physical device '{}' does not support required Vulkan 1.2 feature bufferDeviceAddress",
                    selected_name
                ));
            }
            if vulkan12_features_query.scalarBlockLayout == 0 {
                return Err(format!(
                    "Selected physical device '{}' does not support required Vulkan 1.2 feature scalarBlockLayout",
                    selected_name
                ));
            }

            let descriptor_indexing_supported = vulkan12_features_query.descriptorIndexing != 0
                && vulkan12_features_query.runtimeDescriptorArray != 0
                && vulkan12_features_query.shaderSampledImageArrayNonUniformIndexing != 0;

            let descriptor_buffer_supported = !optional_extensions_disabled
                && descriptor_buffer_extension_available
                && synchronization2_extension_available
                && descriptor_indexing_supported
                && descriptor_buffer_features_query.descriptorBuffer != 0;
            let descriptor_buffer_capture_replay = descriptor_buffer_supported
                && descriptor_buffer_features_query.descriptorBufferCaptureReplay != 0;
            let descriptor_buffer_image_layout_ignored = descriptor_buffer_supported
                && descriptor_buffer_features_query.descriptorBufferImageLayoutIgnored != 0;
            let capabilities = DeviceCapabilities {
                descriptor_buffer_supported,
                descriptor_buffer_capture_replay,
                descriptor_buffer_image_layout_ignored,
                descriptor_indexing_supported,
            };

            // Create logical device
            eprintln!("Creating logical device...");
            let queue_priorities = [1.0f32];
            let mut queue_create_infos = Vec::new();

            let graphics_queue_info = crate::VkDeviceQueueCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
                pNext: std::ptr::null(),
                flags: 0,
                queueFamilyIndex: graphics_queue_family_index,
                queueCount: 1,
                pQueuePriorities: queue_priorities.as_ptr(),
            };
            queue_create_infos.push(graphics_queue_info);

            if graphics_queue_family_index != present_queue_family_index {
                let present_queue_info = crate::VkDeviceQueueCreateInfo {
                    sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
                    pNext: std::ptr::null_mut(),
                    flags: 0,
                    queueFamilyIndex: present_queue_family_index,
                    queueCount: 1,
                    pQueuePriorities: queue_priorities.as_ptr(),
                };
                queue_create_infos.push(present_queue_info);
            }

            // Required and optional device extensions
            let mut enabled_extensions = Vec::new();
            if surface.is_some() {
                enabled_extensions.push(b"VK_KHR_swapchain\0".as_ptr() as *const i8);
            }
            if descriptor_buffer_supported {
                enabled_extensions.push(b"VK_EXT_descriptor_buffer\0".as_ptr() as *const i8);
                enabled_extensions.push(b"VK_KHR_synchronization2\0".as_ptr() as *const i8);
            }

            eprintln!("Enabled device extensions:");
            for ext in &enabled_extensions {
                eprintln!("  {:?}", std::ffi::CStr::from_ptr(*ext));
            }
            eprintln!(
                "Capabilities: descriptor_indexing_supported={}, descriptor_buffer_supported={}",
                capabilities.descriptor_indexing_supported,
                capabilities.descriptor_buffer_supported
            );

            // Core feature toggles
            let mut enabled_features: crate::VkPhysicalDeviceFeatures = std::mem::zeroed();
            enabled_features.shaderInt64 = if features2.features.shaderInt64 != 0 {
                crate::VK_TRUE
            } else {
                0
            };

            // Vulkan 1.2 feature chain
            let mut vulkan12_features: crate::VkPhysicalDeviceVulkan12Features = std::mem::zeroed();
            vulkan12_features.sType =
                crate::VkStructureType::VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES;
            vulkan12_features.pNext = std::ptr::null_mut();

            // Required by this abstraction.
            vulkan12_features.bufferDeviceAddress = crate::VK_TRUE;
            vulkan12_features.scalarBlockLayout = crate::VK_TRUE;

            // Required for bindless-style `sampler2D textures[];` runtime arrays and non-uniform indexing.
            // These satisfy validation errors like:
            // - Capability RuntimeDescriptorArray -> VkPhysicalDeviceVulkan12Features::runtimeDescriptorArray
            // - Capability SampledImageArrayNonUniformIndexing -> VkPhysicalDeviceVulkan12Features::shaderSampledImageArrayNonUniformIndexing
            vulkan12_features.descriptorIndexing = if capabilities.descriptor_indexing_supported {
                crate::VK_TRUE
            } else {
                0
            };
            vulkan12_features.runtimeDescriptorArray = if capabilities.descriptor_indexing_supported
            {
                crate::VK_TRUE
            } else {
                0
            };
            vulkan12_features.shaderSampledImageArrayNonUniformIndexing =
                if capabilities.descriptor_indexing_supported {
                    crate::VK_TRUE
                } else {
                    0
                };

            let mut descriptor_buffer_features_enable =
                crate::VkPhysicalDeviceDescriptorBufferFeaturesEXT {
                    sType: crate::VkStructureType::VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DESCRIPTOR_BUFFER_FEATURES_EXT,
                    pNext: &mut vulkan12_features as *mut _ as *mut libc::c_void,
                    descriptorBuffer: if capabilities.descriptor_buffer_supported { crate::VK_TRUE } else { 0 },
                    descriptorBufferCaptureReplay: if capabilities.descriptor_buffer_capture_replay { crate::VK_TRUE } else { 0 },
                    descriptorBufferImageLayoutIgnored: if capabilities.descriptor_buffer_image_layout_ignored { crate::VK_TRUE } else { 0 },
                    descriptorBufferPushDescriptors: 0,
                };
            let feature_chain = if capabilities.descriptor_buffer_supported {
                &mut descriptor_buffer_features_enable as *mut _ as *mut libc::c_void
            } else {
                &mut vulkan12_features as *mut _ as *mut libc::c_void
            };

            let device_create_info = crate::VkDeviceCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
                pNext: feature_chain,
                flags: 0,
                queueCreateInfoCount: queue_create_infos.len() as u32,
                pQueueCreateInfos: queue_create_infos.as_ptr(),
                enabledLayerCount: 0,
                ppEnabledLayerNames: std::ptr::null(),
                enabledExtensionCount: enabled_extensions.len() as u32,
                ppEnabledExtensionNames: if enabled_extensions.is_empty() {
                    std::ptr::null()
                } else {
                    enabled_extensions.as_ptr()
                },
                pEnabledFeatures: &enabled_features,
            };

            eprintln!("Calling vkCreateDevice...");
            let mut device = std::ptr::null_mut();
            let result = crate::vkCreateDevice(
                physical_device,
                &device_create_info,
                std::ptr::null(),
                &mut device,
            );
            if result != crate::VkResult::VK_SUCCESS {
                eprintln!("vkCreateDevice failed: {:?}", result);
                return Err(format!("Failed to create logical device: {:?}", result));
            }
            eprintln!("Logical device created successfully");

            // Get queues
            let mut graphics_queue = std::ptr::null_mut();
            crate::vkGetDeviceQueue(device, graphics_queue_family_index, 0, &mut graphics_queue);
            let mut present_queue = std::ptr::null_mut();
            crate::vkGetDeviceQueue(device, present_queue_family_index, 0, &mut present_queue);

            // Create command pool
            let command_pool_info = crate::VkCommandPoolCreateInfo {
                sType: crate::VkStructureType::VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
                pNext: std::ptr::null(),
                flags: crate::VkCommandPoolCreateFlagBits::VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT as u32,
                queueFamilyIndex: graphics_queue_family_index,
            };

            let mut command_pool = std::ptr::null_mut();
            let result = crate::vkCreateCommandPool(
                device,
                &command_pool_info,
                std::ptr::null(),
                &mut command_pool,
            );
            if result != crate::VkResult::VK_SUCCESS {
                crate::vkDestroyDevice(device, std::ptr::null());
                return Err(format!("Failed to create command pool: {:?}", result));
            }

            Ok(VulkanDevice {
                surface,
                physical_device,
                device,
                graphics_queue,
                present_queue,
                command_pool,
                instance,
                capabilities,
                descriptor_buffer_supported: capabilities.descriptor_buffer_supported,
            })
        }
    }

    pub fn graphics_context(&self) -> simple::Result<simple::GraphicsContext> {
        simple::GraphicsContext::new(
            self.instance.instance,
            self.physical_device,
            self.device,
            self.graphics_queue,
            self.present_queue,
            self.command_pool,
            self.descriptor_buffer_supported,
        )
    }
}

impl Drop for VulkanDevice {
    fn drop(&mut self) {
        unsafe {
            crate::vkDestroyCommandPool(self.device, self.command_pool, std::ptr::null());
            crate::vkDestroyDevice(self.device, std::ptr::null());
        }
    }
}

pub struct SdlWindow {
    pub window: *mut SDL_Window,
}

impl SdlWindow {
    pub fn new(title: &str, width: i32, height: i32) -> Result<Self, String> {
        unsafe {
            let window = SDL_CreateWindow(
                title.as_ptr() as *const c_char,
                width,
                height,
                SDL_WINDOW_VULKAN | SDL_WINDOW_RESIZABLE,
            );
            if window.is_null() {
                return Err(format!("SDL_CreateWindow failed: {}", get_sdl_error()));
            }
            Ok(SdlWindow { window })
        }
    }
}

impl Drop for SdlWindow {
    fn drop(&mut self) {
        unsafe {
            SDL_DestroyWindow(self.window);
        }
    }
}

fn get_sdl_error() -> String {
    unsafe {
        let error = SDL_GetError();
        std::ffi::CStr::from_ptr(error)
            .to_string_lossy()
            .into_owned()
    }
}
