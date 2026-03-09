use rust_and_vulkan::automation::AutomationFileLoader;
use rust_and_vulkan::{EguiManager, EguiRenderer};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};

/// Helper function to render code in egui with line numbers
fn render_code_editor(ui: &mut egui::Ui, code: &str) {
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            for (line_num, line) in code.lines().enumerate() {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::GRAY, format!("{:4} | ", line_num + 1));
                    ui.monospace(line);
                });
            }
        });
}

use rust_and_vulkan::simple::{
    Buffer, CommandBuffer, DescriptorPool, DescriptorSet, DescriptorSetLayout, Format,
    GraphicsContext, GraphicsPipeline, GraphicsPipelineConfig, PipelineLayout, ShaderModule,
    Swapchain, TextureDescriptorHeap, TextureUsage,
};

use glm as gl;
use std::fmt::Write as _;

#[repr(C)]
#[derive(Clone, Copy)]
struct PushConstants {
    // Device address of vertex buffer (used by the vertex shader)
    vertex_ptr: u64,
    // Device address of MPV buffer (mat4) (used by the vertex shader)
    mpv_ptr: u64,

    // Bindless texture selection (used by the fragment shader).
    // The shader expects a 16-byte push-constant block (std430),
    // so we pad to 16 bytes.
    texture_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MPV_PushConstants {
    // Column-major mat4 as expected by GLSL.
    mpv: gl::Mat4,
}

fn load_spirv_u32(path: &str) -> Result<Vec<u32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    if bytes.len() % 4 != 0 {
        return Err(format!("SPIR-V file not u32-aligned: {path}"));
    }
    let mut words = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        words.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(words)
}

// Vertex data used for device-address vertex pulling.
#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    normal: [f32; 3],
    uv: [f32; 2],
}

fn main() -> Result<(), String> {
    let sdl = SdlContext::init()?;
    // Deliver mouse-button-down events even when the click is what gives the
    // window focus (i.e. don't swallow the first click as a focus-gain event).
    unsafe {
        rust_and_vulkan::SDL_SetHint(
            rust_and_vulkan::SDL_HINT_MOUSE_FOCUS_CLICKTHROUGH.as_ptr() as *const i8,
            b"1\0".as_ptr() as *const i8,
        );
    }
    let window = SdlWindow::new("Rotating Square (bindless textures + fallback)", 800, 600)?;

    // Instance + Surface + Device
    let instance = VulkanInstance::create(&sdl, &window)?;
    let surface = VulkanSurface::create(&window, &instance)?;
    let device = VulkanDevice::create(instance, Some(surface))?;

    // Bridge into simple GraphicsContext
    let context = GraphicsContext::new(
        device.instance.instance,
        device.physical_device,
        device.device,
        device.graphics_queue,
        device.present_queue,
        device.command_pool,
        device.descriptor_buffer_supported,
    )
    .map_err(|e| e.to_string())?;

    // Create swapchain
    let surface_khr = device.surface.as_ref().expect("surface expected").surface;
    let mut swapchain =
        Swapchain::new(&context, surface_khr, 800, 600).map_err(|e| e.to_string())?;

    // Shaders
    let vert_spv = load_spirv_u32("shaders/simple_square.vert.spv")?;
    let frag_spv = load_spirv_u32("shaders/simple_square.frag.spv")?;

    let vs = ShaderModule::new(&context, &vert_spv).map_err(|e| e.to_string())?;
    let fs = ShaderModule::new(&context, &frag_spv).map_err(|e| e.to_string())?;

    let use_bindless_descriptor_buffer = context.descriptor_buffer_supported();
    if !use_bindless_descriptor_buffer {
        eprintln!(
            "Descriptor buffer extension unavailable; falling back to traditional descriptor sets"
        );
    }

    // Geometry (vertex pulling via buffer device address)
    let square_vertices = vec![
        Vertex {
            pos: [-0.5, -0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        },
        Vertex {
            pos: [0.5, -0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 0.0],
        },
        Vertex {
            pos: [0.5, 0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 1.0],
        },
        Vertex {
            pos: [0.5, 0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 1.0],
        },
        Vertex {
            pos: [-0.5, 0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 1.0],
        },
        Vertex {
            pos: [-0.5, -0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        },
    ];
    let vertex_buffer = Buffer::vertex_buffer(&context, &square_vertices)
        .map_err(|e| format!("Failed to create square vertex buffer: {}", e))?;

    // Create a buffer for MVP matrix (will be updated each frame)
    let mpv_buffer = {
        let mpv = MPV_PushConstants {
            mpv: num_traits::one(), // identity initially
        };
        Buffer::from_device_address(&context, &[mpv])
            .map_err(|e| format!("Failed to create MPV buffer: {}", e))?
    };

    // Create a tiny 2x2 RGBA texture (used for both bindless and fallback paths).
    let tex_pixels: Vec<u8> = vec![
        255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255,
    ];
    let texture = context
        .upload_texture(
            &tex_pixels,
            2,
            2,
            Format::Rgba8Unorm,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
        )
        .map_err(|e| e.to_string())?;

    let sampler = context
        .create_default_sampler()
        .map_err(|e| e.to_string())?;

    // Bindless heap path (if supported)
    let mut bindless_heap: Option<TextureDescriptorHeap> = None;
    let mut bindless_texture_index: u32 = 0;

    if context.descriptor_buffer_supported() {
        let mut heap = TextureDescriptorHeap::new(&context, 64).map_err(|e| e.to_string())?;
        let idx = heap.allocate().map_err(|e| e.to_string())?;
        heap.write_descriptor(&context, idx, &texture, sampler)
            .map_err(|e| e.to_string())?;
        bindless_texture_index = idx;
        bindless_heap = Some(heap);
    }

    // Fallback descriptor-set path (always available in this demo)
    let set_layout =
        DescriptorSetLayout::new_texture_array(&context, 1).map_err(|e| e.to_string())?;
    let pool = DescriptorPool::new(&context, 1, 1).map_err(|e| e.to_string())?;
    let fallback_set: DescriptorSet = pool.allocate(&set_layout).map_err(|e| e.to_string())?;
    fallback_set
        .write_textures(&context, &[&texture], sampler)
        .map_err(|e| e.to_string())?;

    let layout = if use_bindless_descriptor_buffer {
        let bindless_set_layout =
            DescriptorSetLayout::new_bindless_textures(&context, 64).map_err(|e| e.to_string())?;
        PipelineLayout::with_descriptor_set_layouts_and_push_size(
            &context,
            &[bindless_set_layout],
            rust_and_vulkan::simple::SHADER_STAGE_VERTEX
                | rust_and_vulkan::simple::SHADER_STAGE_FRAGMENT,
            size_of::<PushConstants>() as u32,
        )
        .map_err(|e| e.to_string())?
    } else {
        PipelineLayout::with_descriptor_set_layouts_and_push_size(
            &context,
            &[set_layout],
            rust_and_vulkan::simple::SHADER_STAGE_VERTEX
                | rust_and_vulkan::simple::SHADER_STAGE_FRAGMENT,
            size_of::<PushConstants>() as u32,
        )
        .map_err(|e| e.to_string())?
    };

    let pipeline = if use_bindless_descriptor_buffer {
        GraphicsPipeline::builder(&context, &vs, &fs, &layout, swapchain.render_pass())
            .with_descriptor_buffer()
            .build()
    } else {
        GraphicsPipeline::builder(&context, &vs, &fs, &layout, swapchain.render_pass())
            .with_config(GraphicsPipelineConfig::standard_opaque())
            .build()
    }
    .map_err(|e| e.to_string())?;

    let start = std::time::Instant::now();

    // Initialize egui
    let mut egui_manager = EguiManager::new();
    let mut egui_renderer =
        EguiRenderer::new(&context, swapchain.render_pass()).map_err(|e| e.to_string())?;

    // Simple smoothed frame-rate estimator for UI display.
    let mut last_frame_time = std::time::Instant::now();
    let mut refresh_label = String::with_capacity(64); // Pre-allocate to prevent reallocation
    refresh_label.push_str("Current refresh: --.- Hz");

    // Initialize automation file loader
    let mut automation_loader = AutomationFileLoader::default();

    // Event + render loop
    let mut quit = false;
    let mut window_resized = false;
    while !quit {
        let now = std::time::Instant::now();
        let frame_dt = now.duration_since(last_frame_time).as_secs_f32();
        last_frame_time = now;

        refresh_label.clear();
        let _ = write!(refresh_label, "Current refresh: {:.4} Hz", 1.0 / frame_dt);

        // Poll events
        unsafe {
            let mut event = std::mem::zeroed();
            while rust_and_vulkan::SDL_PollEvent(&mut event) {
                if event.type_ == rust_and_vulkan::SDL_EventType::SDL_EVENT_QUIT as u32 {
                    quit = true;
                } else if event.type_
                    == rust_and_vulkan::SDL_EventType::SDL_EVENT_WINDOW_RESIZED as u32
                    || event.type_
                        == rust_and_vulkan::SDL_EventType::SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED
                            as u32
                {
                    window_resized = true;
                }
                // Feed events to egui
                egui_manager.handle_event(&event);
            }
        }

        // Handle window resize by recreating swapchain
        if window_resized {
            // Get new window size
            let mut width = 0i32;
            let mut height = 0i32;
            unsafe {
                rust_and_vulkan::SDL_GetWindowSizeInPixels(window.window, &mut width, &mut height);
            }

            if width > 0 && height > 0 {
                match swapchain.recreate(&context, surface_khr, width as u32, height as u32) {
                    Ok(_) => println!("Swapchain recreated: {}x{}", width, height),
                    Err(e) => eprintln!("Failed to recreate swapchain: {e:?}"),
                }
            }
            window_resized = false;
        }

        // Begin frame (acquire image, sync, reset cmd)
        if let Err(e) = swapchain.begin_frame() {
            eprintln!("begin_frame failed: {e:?}");
            // On OUT_OF_DATE error, try to recreate the swapchain
            if matches!(e, rust_and_vulkan::simple::Error::Vulkan(ref msg) if msg.contains("out of date"))
            {
                let mut width = 0i32;
                let mut height = 0i32;
                unsafe {
                    rust_and_vulkan::SDL_GetWindowSizeInPixels(
                        window.window,
                        &mut width,
                        &mut height,
                    );
                }

                if width > 0 && height > 0 {
                    if let Err(e) =
                        swapchain.recreate(&context, surface_khr, width as u32, height as u32)
                    {
                        eprintln!("Failed to recreate swapchain: {e:?}");
                    }
                }
            }
            continue;
        }

        let _frame_index = swapchain.current_frame_index();
        let image_index = swapchain.current_image_index();
        let extent = swapchain.extent();
        let cmd: &CommandBuffer = swapchain.current_command_buffer();

        // Begin egui frame
        egui_manager.begin_frame(extent.width as f32, extent.height as f32);

        // Build UI panels
        {
            egui::Window::new("Options")
                .vscroll(true)
                .show(&egui_manager.ctx, |ui| {
                    ui.label("Select an option:");
                    if ui.button("Option 1").clicked() {
                        egui_manager.selected_option = "Option 1";
                    }
                    if ui.button("Option 2").clicked() {
                        egui_manager.selected_option = "Option 2";
                    }
                    if ui.button("Option 3").clicked() {
                        egui_manager.selected_option = "Option 3";
                    }
                });

            egui::Window::new("Data Display")
                .vscroll(true)
                .show(&egui_manager.ctx, |ui| {
                    ui.label("Selected:");
                    ui.label(egui_manager.selected_option);
                    ui.separator();
                    ui.label("Status:");
                    ui.label(egui_manager.data_display);
                    ui.separator();
                    ui.label(egui::RichText::new(refresh_label.as_str()));
                });

            // Handle automation UI interactions first (without borrowing automation_loader in closures)
            let mut should_navigate_up = false;
            let mut should_refresh = false;
            let mut file_to_load: Option<std::path::PathBuf> = None;
            let mut should_close_code_display = false;

            // Automation File Browser Window
            if automation_loader.show_browser {
                egui::Window::new("Automation File Loader")
                    .open(&mut automation_loader.show_browser)
                    .default_width(500.0)
                    .default_height(400.0)
                    .vscroll(true)
                    .show(&egui_manager.ctx, |ui| {
                        ui.heading("File Browser");

                        // Current directory display
                        ui.horizontal(|ui| {
                            ui.label("Location:");
                            let mut dir_str =
                                automation_loader.current_dir.to_string_lossy().to_string();
                            ui.text_edit_singleline(&mut dir_str);
                        });

                        // Navigation buttons
                        ui.horizontal(|ui| {
                            if ui.button("↑ Parent Directory").clicked() {
                                should_navigate_up = true;
                            }
                            if ui.button("🔄 Refresh").clicked() {
                                should_refresh = true;
                            }
                        });

                        // File filter
                        ui.horizontal(|ui| {
                            ui.label("Filter:");
                            let old_filter = automation_loader.file_filter.clone();
                            ui.text_edit_singleline(&mut automation_loader.file_filter);
                            if automation_loader.file_filter != old_filter {
                                should_refresh = true;
                            }
                        });

                        ui.separator();

                        // File list
                        ui.label(format!("Files ({}):", automation_loader.files.len()));

                        for file in automation_loader.files.clone() {
                            let icon = if file.is_dir { "📁" } else { "📄" };
                            let size_str = if file.is_dir {
                                String::new()
                            } else {
                                format!(" ({})", AutomationFileLoader::format_size(file.size))
                            };

                            let label = format!("{} {}{}", icon, file.name, size_str);

                            if ui
                                .selectable_label(
                                    automation_loader.selected_file.as_ref() == Some(&file.path),
                                    label,
                                )
                                .clicked()
                            {
                                if file.is_dir {
                                    file_to_load = Some(file.path.clone());
                                } else {
                                    file_to_load = Some(file.path.clone());
                                }
                            }
                        }

                        // Error message display
                        if let Some(error) = &automation_loader.error_message {
                            ui.separator();
                            ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
                        }
                    });
            }

            // Process deferred actions
            if should_navigate_up {
                automation_loader.navigate_up();
            }
            if should_refresh {
                automation_loader.refresh_files();
            }
            if let Some(path) = file_to_load {
                if path.is_dir() {
                    automation_loader.navigate_to(&path);
                } else {
                    automation_loader.load_file(&path);
                }
            }

            // Code Display Window
            if automation_loader.show_code_display {
                // Pre-capture values to avoid borrow conflicts
                let file_name = automation_loader.current_file_name();
                let file_content = automation_loader.file_content.clone();

                egui::Window::new("Code Display")
                    .open(&mut automation_loader.show_code_display)
                    .default_width(600.0)
                    .default_height(500.0)
                    .vscroll(false)
                    .show(&egui_manager.ctx, |ui| {
                        if let Some(name) = &file_name {
                            ui.heading(format!("📄 {}", name));
                        }

                        ui.separator();

                        if let Some(content) = &file_content {
                            render_code_editor(ui, content.as_str());
                        } else {
                            ui.label("No file loaded");
                        }

                        if ui.button("Close").clicked() {
                            should_close_code_display = true;
                        }
                    });

                if should_close_code_display {
                    automation_loader.show_code_display = false;
                }
            }
        }

        // End egui frame and get tessellated primitives + texture updates
        let (clipped_primitives, textures_delta) = egui_manager.end_frame();

        // Upload any new/changed textures (e.g. font atlas on first frame)
        egui_renderer
            .update_textures(&context, &textures_delta)
            .map_err(|e| e.to_string())?;

        // Prepare egui renderer with tessellated output
        egui_renderer
            .prepare(
                &context,
                clipped_primitives,
                extent.width as f32,
                extent.height as f32,
            )
            .map_err(|e| e.to_string())?;

        // Record commands
        cmd.begin().map_err(|e| e.to_string())?;

        cmd.begin_render_pass(
            swapchain.render_pass(),
            swapchain.framebuffer(image_index),
            extent.width,
            extent.height,
            [0.95, 0.95, 0.95, 1.0], // Light gray background to match light theme
        );

        cmd.bind_pipeline(&pipeline);

        if use_bindless_descriptor_buffer {
            let Some(ref heap) = bindless_heap else {
                return Err(
                    "bindless mode selected but descriptor heap not initialized".to_string()
                );
            };
            cmd.bind_texture_heap(
                heap,
                &layout,
                0,
                rust_and_vulkan::VkPipelineBindPoint::VK_PIPELINE_BIND_POINT_GRAPHICS,
            );
        } else {
            cmd.bind_descriptor_sets(&layout, 0, &[&fallback_set]);
        }

        // Update MVP matrix each frame: rotate around Z.
        let t = start.elapsed().as_secs_f32();
        let ident: gl::Mat4 = num_traits::one();
        let rot = gl::ext::rotate(&ident, t, gl::vec3(0.0, 0.0, 1.0));
        let proj: gl::Mat4 = num_traits::one();
        let mvp = proj * rot;

        // Update the MPV buffer with the new matrix
        let mpv_data = MPV_PushConstants { mpv: mvp };
        let mpv_bytes = unsafe {
            std::slice::from_raw_parts(
                (&mpv_data as *const MPV_PushConstants) as *const u8,
                std::mem::size_of::<MPV_PushConstants>(),
            )
        };
        mpv_buffer.write(mpv_bytes).map_err(|e| e.to_string())?;

        // Push the device addresses + texture index for the fragment shader
        let pc = PushConstants {
            vertex_ptr: vertex_buffer.device_address(),
            mpv_ptr: mpv_buffer.device_address(),
            texture_index: bindless_texture_index,
        };

        let pc_bytes = unsafe {
            std::slice::from_raw_parts(
                (&pc as *const PushConstants) as *const u8,
                std::mem::size_of::<PushConstants>(),
            )
        };
        cmd.push_constants(&layout, pc_bytes);

        // Draw 2 triangles (6 vertices)
        cmd.draw(6, 1, 0, 0);

        // Render egui UI on top of scene
        egui_renderer
            .render(cmd, extent.width as f32, extent.height as f32)
            .map_err(|e| e.to_string())?;

        cmd.end_render_pass();
        cmd.end().map_err(|e| e.to_string())?;

        // Submit + present
        if let Err(e) = swapchain.end_frame(&context) {
            eprintln!("end_frame failed: {e:?}");
        }
    }
    // Ensure device idle before drop order tears things down.
    context.wait_idle().map_err(|e| e.to_string())?;

    // Vulkan requires all child objects to be destroyed before destroying the device.
    context.destroy_sampler(sampler);

    Ok(())
}
