//! Simple triangle example using SDL3 and Vulkan bindings.
//! This demonstrates the minimal setup to create a window and clear the screen.

use rust_and_vulkan::{SdlContext, SdlWindow, VulkanInstance};

fn main() -> Result<(), String> {
    // Initialize SDL3
    let sdl = SdlContext::init()?;

    // Create a window with Vulkan support
    let window = SdlWindow::new("Rust + SDL3 + Vulkan - Triangle", 800, 600)?;

    // Create Vulkan instance
    let _instance = VulkanInstance::create(&sdl, &window)?;

    println!("SDL3, window, and Vulkan instance created successfully!");
    println!("Window should be visible. Press Ctrl+C or close window to exit.");

    // Simple event loop
    let mut quit = false;
    while !quit {
        unsafe {
            let mut event = std::mem::zeroed();
            while rust_and_vulkan::SDL_PollEvent(&mut event) {
                if event.type_ == rust_and_vulkan::SDL_EventType::SDL_EVENT_QUIT as u32 {
                    quit = true;
                }
            }
        }

        // In a real application, you would:
        // 1. Acquire swapchain image
        // 2. Record command buffer
        // 3. Submit commands
        // 4. Present image

        std::thread::sleep(std::time::Duration::from_millis(16)); // ~60 FPS
    }

    println!("Shutting down...");
    Ok(())
}
