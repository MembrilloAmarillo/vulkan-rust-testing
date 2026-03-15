use rust_and_vulkan::automation::AutomationFileLoader;
use rust_and_vulkan::ecss_automation::{ExecutionEvent, ExecutionStats};
use rust_and_vulkan::{EguiManager, EguiRenderer};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};

use rust_and_vulkan::simple::{
    Buffer, BufferUsage, CommandBuffer, DescriptorPool, DescriptorSet, DescriptorSetLayout, Format,
    GraphicsContext, GraphicsPipeline, GraphicsPipelineConfig, MemoryType, PipelineLayout,
    ShaderModule, Swapchain, TextureDescriptorHeap, TextureUsage,
};

use glm as gl;
use std::fmt::Write as _;
use std::net::UdpSocket;

fn send_commander_udp(target: &str, command_line: &str) -> Result<String, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind failed: {}", e))?;
    socket
        .set_read_timeout(Some(std::time::Duration::from_millis(600)))
        .map_err(|e| format!("set_read_timeout failed: {}", e))?;

    socket
        .send_to(command_line.as_bytes(), target)
        .map_err(|e| format!("send_to failed: {}", e))?;

    let mut buf = [0u8; 512];
    match socket.recv_from(&mut buf) {
        Ok((n, _)) => {
            let reply = String::from_utf8_lossy(&buf[..n]).to_string();
            Ok(reply)
        }
        Err(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
        {
            Ok("sent (no immediate reply)".to_string())
        }
        Err(e) => Err(format!("recv_from failed: {}", e)),
    }
}

fn render_virtualized_code_view(
    ui: &mut egui::Ui,
    content: &str,
    line_starts: &[usize],
    highlight_command_name: Option<&str>,
) {
    let total_lines = line_starts.len().saturating_sub(1);
    if total_lines == 0 {
        ui.label("(empty file)");
        return;
    }

    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::both().auto_shrink([false; 2]).show_rows(
        ui,
        row_height,
        total_lines,
        |ui, row_range| {
            for row in row_range {
                let start = line_starts[row];
                let end = line_starts[row + 1];
                let mut line = &content[start..end];
                if line.ends_with('\n') {
                    line = &line[..line.len() - 1];
                }

                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::GRAY, format!("{:>7} ", row + 1));
                    let highlight_line = highlight_command_name
                        .map(|name| !name.is_empty() && line.contains(name))
                        .unwrap_or(false);
                    if highlight_line {
                        ui.label(
                            egui::RichText::new(line)
                                .monospace()
                                .background_color(egui::Color32::from_rgb(255, 245, 170)),
                        );
                    } else {
                        ui.monospace(line);
                    }
                });
            }
        },
    );
}

enum AutomationThreadMessage {
    Progress(ExecutionEvent),
    Finished(Result<ExecutionStats, String>),
}

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

    // SDL3 requires explicit text-input activation for reliable TEXT_INPUT events.
    // Without this, egui text boxes may not receive typed characters.
    unsafe {
        let ok = rust_and_vulkan::SDL_StartTextInput(window.window);
        if !ok {
            eprintln!("Warning: SDL_StartTextInput failed");
        }
    }

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

    // Create swapchain using actual drawable pixel size (important for HiDPI).
    let surface_khr = device.surface.as_ref().expect("surface expected").surface;
    let mut drawable_width = 0i32;
    let mut drawable_height = 0i32;
    unsafe {
        rust_and_vulkan::SDL_GetWindowSizeInPixels(
            window.window,
            &mut drawable_width,
            &mut drawable_height,
        );
    }
    if drawable_width <= 0 || drawable_height <= 0 {
        drawable_width = 800;
        drawable_height = 600;
    }
    let mut swapchain = Swapchain::new(
        &context,
        surface_khr,
        drawable_width as u32,
        drawable_height as u32,
    )
    .map_err(|e| e.to_string())?;

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

    // MVP buffer strategy (optimal for discrete + compatible with old fallback GPUs):
    // - `mpv_buffer`: device-local GPU buffer used by shaders via buffer device address.
    // - `mpv_upload_buffer`: CPU-mapped transfer source updated each frame.
    // Each frame we copy upload -> device-local before drawing.
    let (mpv_buffer, mpv_upload_buffer) = {
        let mpv = MPV_PushConstants {
            mpv: num_traits::one(), // identity initially
        };
        let size = std::mem::size_of::<MPV_PushConstants>();

        let gpu_buf = Buffer::new(
            &context,
            size,
            BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            MemoryType::GpuOnly,
        )
        .map_err(|e| format!("Failed to create GPU MPV buffer: {}", e))?;

        let upload_buf = Buffer::new(
            &context,
            size,
            BufferUsage::TRANSFER_SRC,
            MemoryType::CpuMapped,
        )
        .map_err(|e| format!("Failed to create MPV upload buffer: {}", e))?;

        let bytes = unsafe {
            std::slice::from_raw_parts(&mpv as *const MPV_PushConstants as *const u8, size)
        };
        upload_buf
            .write(bytes)
            .map_err(|e| format!("Failed to initialize MPV upload buffer: {}", e))?;

        (gpu_buf, upload_buf)
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

    // Simple frame-rate estimator for UI display.
    let mut last_frame_time = std::time::Instant::now();
    let mut refresh_label = String::with_capacity(64); // Pre-allocate to prevent reallocation
    refresh_label.push_str("Current refresh: --.- Hz");
    let mut last_refresh_label_update = std::time::Instant::now();
    let refresh_label_update_interval = std::time::Duration::from_millis(250);

    // Initialize automation file loader
    let mut automation_loader = AutomationFileLoader::default();
    automation_loader.refresh_files();
    let mut filter_last_edit_at: Option<std::time::Instant> = None;
    let filter_debounce = std::time::Duration::from_millis(250);

    // Automation execution state
    let mut automation_executing = false;
    let mut automation_stats: Option<ExecutionStats> = None;
    let mut automation_error: Option<String> = None;
    let mut automation_current_command: Option<String> = None;
    let mut automation_progress_label = String::new();
    let mut automation_log: Vec<String> = Vec::new();
    // Channel through which a background execution thread reports its result
    let mut automation_rx: Option<std::sync::mpsc::Receiver<AutomationThreadMessage>> = None;

    // Commander UDP UI state
    let mut commander_show_window = false;
    let mut commander_target_host = "127.0.0.1".to_string();
    let mut commander_target_port: u16 = 8092;
    let mut ftp_list_path = "/".to_string();
    let mut ftp_download_remote = "/remote/file.bin".to_string();
    let mut ftp_download_local = "./downloaded_file.bin".to_string();
    let mut delete_file_path = "/remote/file.bin".to_string();
    let mut delete_all_prefix = "/remote/folder".to_string();
    let mut commander_status = String::new();
    let mut commander_log: Vec<String> = Vec::new();

    // Event + render loop
    let mut quit = false;
    let mut window_resized = false;
    while !quit {
        let now = std::time::Instant::now();
        let frame_dt = now.duration_since(last_frame_time).as_secs_f32();
        last_frame_time = now;

        if now.duration_since(last_refresh_label_update) >= refresh_label_update_interval {
            refresh_label.clear();
            let hz = if frame_dt > 0.0 { 1.0 / frame_dt } else { 0.0 };
            let _ = write!(refresh_label, "Current refresh: {:.4} Hz", hz);
            last_refresh_label_update = now;
        }

        automation_loader.poll_file_load();

        // Poll background automation thread for results
        if automation_executing {
            if let Some(rx) = &automation_rx {
                let mut finished_result: Option<Result<ExecutionStats, String>> = None;
                while let Ok(msg) = rx.try_recv() {
                    match msg {
                        AutomationThreadMessage::Progress(event) => match event {
                            ExecutionEvent::CommandStarted {
                                index,
                                total,
                                name,
                                description,
                            } => {
                                automation_current_command = Some(name.clone());
                                automation_progress_label = if description.trim().is_empty() {
                                    format!("Running {}/{}: {}", index, total, name)
                                } else {
                                    format!(
                                        "Running {}/{}: {} — {}",
                                        index, total, name, description
                                    )
                                };
                                automation_log
                                    .push(format!("▶ {}/{} START {}", index, total, name));
                            }
                            ExecutionEvent::CommandSucceeded {
                                index,
                                total,
                                name,
                                elapsed_ms,
                            } => {
                                automation_log.push(format!(
                                    "✓ {}/{} OK {} ({} ms)",
                                    index, total, name, elapsed_ms
                                ));
                            }
                            ExecutionEvent::CommandFailed {
                                index,
                                total,
                                name,
                                error,
                            } => {
                                automation_log.push(format!(
                                    "✗ {}/{} FAIL {} — {}",
                                    index, total, name, error
                                ));
                            }
                        },
                        AutomationThreadMessage::Finished(result) => {
                            finished_result = Some(result);
                        }
                    }
                }

                if let Some(result) = finished_result {
                    automation_executing = false;
                    automation_rx = None;
                    automation_current_command = None;
                    match result {
                        Ok(stats) => automation_stats = Some(stats),
                        Err(e) => automation_error = Some(e),
                    }
                }

                if automation_log.len() > 200 {
                    let remove_count = automation_log.len() - 200;
                    automation_log.drain(0..remove_count);
                }
            }
        }

        if let Some(last_edit) = filter_last_edit_at {
            if now.duration_since(last_edit) >= filter_debounce {
                automation_loader.refresh_files();
                filter_last_edit_at = None;
            }
        }

        let mut input_pixels_per_point =
            unsafe { rust_and_vulkan::SDL_GetWindowDisplayScale(window.window) };
        if !input_pixels_per_point.is_finite() || input_pixels_per_point <= 0.0 {
            input_pixels_per_point = 1.0;
        }

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
                egui_manager.handle_event(&event, input_pixels_per_point);
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

        // Begin egui frame in framebuffer pixel space.
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

            if commander_show_window {
                egui::Window::new("Commander UDP")
                    .open(&mut commander_show_window)
                    .default_width(520.0)
                    .default_height(460.0)
                    .show(&egui_manager.ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Host:");
                            ui.text_edit_singleline(&mut commander_target_host);
                            ui.label("Port:");
                            ui.add(
                                egui::DragValue::new(&mut commander_target_port).range(1..=65535),
                            );
                        });

                        let target =
                            format!("{}:{}", commander_target_host.trim(), commander_target_port);

                        ui.separator();
                        ui.heading("Actions");

                        if ui.button("HPC_SEND").clicked() {
                            let cmd = "HPC_SEND".to_string();
                            match send_commander_udp(&target, &cmd) {
                                Ok(reply) => {
                                    commander_status = format!("{} -> {}", cmd, reply);
                                    commander_log.push(commander_status.clone());
                                }
                                Err(e) => {
                                    commander_status = format!("{} -> ERROR {}", cmd, e);
                                    commander_log.push(commander_status.clone());
                                }
                            }
                        }

                        ui.separator();
                        ui.label("FTP_LIST <path>");
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut ftp_list_path);
                            if ui.button("Send FTP_LIST").clicked() {
                                let cmd = format!("FTP_LIST {}", ftp_list_path.trim());
                                match send_commander_udp(&target, &cmd) {
                                    Ok(reply) => {
                                        commander_status = format!("{} -> {}", cmd, reply);
                                        commander_log.push(commander_status.clone());
                                    }
                                    Err(e) => {
                                        commander_status = format!("{} -> ERROR {}", cmd, e);
                                        commander_log.push(commander_status.clone());
                                    }
                                }
                            }
                        });

                        ui.separator();
                        ui.label("FTP_DOWNLOAD <remote> <local>");
                        ui.text_edit_singleline(&mut ftp_download_remote);
                        ui.text_edit_singleline(&mut ftp_download_local);
                        if ui.button("Send FTP_DOWNLOAD").clicked() {
                            let cmd = format!(
                                "FTP_DOWNLOAD {} {}",
                                ftp_download_remote.trim(),
                                ftp_download_local.trim()
                            );
                            match send_commander_udp(&target, &cmd) {
                                Ok(reply) => {
                                    commander_status = format!("{} -> {}", cmd, reply);
                                    commander_log.push(commander_status.clone());
                                }
                                Err(e) => {
                                    commander_status = format!("{} -> ERROR {}", cmd, e);
                                    commander_log.push(commander_status.clone());
                                }
                            }
                        }

                        ui.separator();
                        ui.label("DELETE_FILE <path>");
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut delete_file_path);
                            if ui.button("Send DELETE_FILE").clicked() {
                                let cmd = format!("DELETE_FILE {}", delete_file_path.trim());
                                match send_commander_udp(&target, &cmd) {
                                    Ok(reply) => {
                                        commander_status = format!("{} -> {}", cmd, reply);
                                        commander_log.push(commander_status.clone());
                                    }
                                    Err(e) => {
                                        commander_status = format!("{} -> ERROR {}", cmd, e);
                                        commander_log.push(commander_status.clone());
                                    }
                                }
                            }
                        });

                        ui.separator();
                        ui.label("DELETE_ALL <prefix>");
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut delete_all_prefix);
                            if ui.button("Send DELETE_ALL").clicked() {
                                let cmd = format!("DELETE_ALL {}", delete_all_prefix.trim());
                                match send_commander_udp(&target, &cmd) {
                                    Ok(reply) => {
                                        commander_status = format!("{} -> {}", cmd, reply);
                                        commander_log.push(commander_status.clone());
                                    }
                                    Err(e) => {
                                        commander_status = format!("{} -> ERROR {}", cmd, e);
                                        commander_log.push(commander_status.clone());
                                    }
                                }
                            }
                        });

                        if commander_log.len() > 200 {
                            let remove_count = commander_log.len() - 200;
                            commander_log.drain(0..remove_count);
                        }

                        ui.separator();
                        if !commander_status.is_empty() {
                            ui.label(format!("Status: {}", commander_status));
                        }

                        egui::CollapsingHeader::new("UDP Log")
                            .default_open(true)
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(120.0)
                                    .show(ui, |ui| {
                                        for line in commander_log.iter().rev().take(20) {
                                            ui.monospace(line);
                                        }
                                    });
                            });
                    });
            }

            // Handle automation UI interactions first (without borrowing automation_loader in closures)
            let mut should_navigate_up = false;
            let mut should_refresh = false;
            let mut file_to_load: Option<std::path::PathBuf> = None;

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
                            let filter_edit =
                                ui.text_edit_singleline(&mut automation_loader.file_filter);
                            if filter_edit.changed() {
                                filter_last_edit_at = Some(now);
                            }
                            if filter_edit.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                should_refresh = true;
                                filter_last_edit_at = None;
                            }
                        });

                        ui.separator();

                        // File list
                        ui.label(format!("Files ({}):", automation_loader.files.len()));

                        for file in &automation_loader.files {
                            let selected =
                                automation_loader.selected_file.as_ref() == Some(&file.path);

                            if ui
                                .selectable_label(selected, file.display_label.as_str())
                                .clicked()
                            {
                                file_to_load = Some(file.path.clone());
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
                let can_execute = file_name
                    .as_ref()
                    .map(|n| n.ends_with(".toml") || n.ends_with(".json"))
                    .unwrap_or(false);

                let mut window_open = true;
                egui::Window::new("Code Display")
                    .open(&mut window_open)
                    .default_width(700.0)
                    .default_height(600.0)
                    .vscroll(false)
                    .show(&egui_manager.ctx, |ui| {
                        ui.vertical(|ui| {
                            if let Some(name) = &file_name {
                                ui.heading(format!("📄 {}", name));
                            }
                            ui.separator();

                            // Keep code view stable during resize by allocating a fixed portion
                            // of the remaining space and keeping controls below it.
                            let code_height = (ui.available_height() * 0.60).max(120.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), code_height),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    if automation_loader.is_loading_file {
                                        ui.label("Loading file…");
                                    } else if let (Some(content), Some(line_starts)) = (
                                        automation_loader.file_content.as_deref(),
                                        automation_loader.file_line_starts.as_deref(),
                                    ) {
                                        render_virtualized_code_view(
                                            ui,
                                            content,
                                            line_starts,
                                            automation_current_command.as_deref(),
                                        );
                                    } else {
                                        ui.label("No file loaded");
                                    }
                                },
                            );

                            ui.separator();

                            ui.horizontal(|ui| {
                                if can_execute {
                                    let running = automation_executing;
                                    let btn_text = if running {
                                        "⏳ Running…"
                                    } else {
                                        "▶ Run Automation"
                                    };
                                    if ui
                                        .add_enabled(!running, egui::Button::new(btn_text))
                                        .clicked()
                                    {
                                        if let (Some(name), Some(content)) = (&file_name, &file_content)
                                        {
                                            let parse_result = if name.ends_with(".toml") {
                                                rust_and_vulkan::ecss_automation::AutomationEngine::from_toml_str(content)
                                            } else {
                                                rust_and_vulkan::ecss_automation::AutomationEngine::from_json_str(content)
                                            };
                                            match parse_result {
                                                Ok(engine) => {
                                                    automation_executing = true;
                                                    automation_stats = None;
                                                    automation_error = None;
                                                    automation_current_command = None;
                                                    automation_progress_label.clear();
                                                    automation_log.clear();
                                                    let (tx, rx) = std::sync::mpsc::channel();
                                                    automation_rx = Some(rx);
                                                    std::thread::spawn(move || {
                                                        let rt = tokio::runtime::Runtime::new()
                                                            .expect("tokio runtime");
                                                        let result = rt
                                                            .block_on(async {
                                                                engine
                                                                    .execute_with_progress(|event| {
                                                                        let _ = tx.send(
                                                                            AutomationThreadMessage::Progress(
                                                                                event,
                                                                            ),
                                                                        );
                                                                    })
                                                                    .await
                                                            })
                                                            .map_err(|e| e.to_string());
                                                        let _ = tx.send(
                                                            AutomationThreadMessage::Finished(result),
                                                        );
                                                    });
                                                }
                                                Err(e) => {
                                                    automation_error =
                                                        Some(format!("Parse error: {}", e));
                                                }
                                            }
                                        }
                                    }
                                } else if file_name.is_some() {
                                    ui.label("⚠ Only .toml / .json files can be executed");
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Close").clicked() {
                                            automation_loader.show_code_display = false;
                                        }
                                    },
                                );
                            });

                            if let Some(stats) = &automation_stats {
                                let color = if stats.failed == 0 {
                                    egui::Color32::from_rgb(0, 160, 0)
                                } else {
                                    egui::Color32::from_rgb(200, 140, 0)
                                };
                                ui.colored_label(
                                    color,
                                    format!(
                                        "✓ Done: {}/{} commands, {} ms, {:.1}% success",
                                        stats.successful,
                                        stats.successful + stats.failed,
                                        stats.elapsed_ms,
                                        stats.success_rate()
                                    ),
                                );
                            }
                            if automation_executing {
                                if automation_progress_label.is_empty() {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(50, 120, 200),
                                        "Running automation…",
                                    );
                                } else {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(50, 120, 200),
                                        automation_progress_label.as_str(),
                                    );
                                }
                            }
                            if let Some(error) = &automation_error {
                                ui.colored_label(egui::Color32::RED, format!("❌ {}", error));
                            }

                            egui::CollapsingHeader::new("Execution Log")
                                .default_open(automation_executing)
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .max_height(130.0)
                                        .auto_shrink([false; 2])
                                        .show(ui, |ui| {
                                            if automation_log.is_empty() {
                                                ui.label("No entries yet");
                                            } else {
                                                for entry in &automation_log {
                                                    ui.monospace(entry);
                                                }
                                            }
                                        });
                                });
                        });
                    });

                automation_loader.show_code_display = window_open;
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

        // Update MVP matrix each frame: rotate around Z.
        let t = start.elapsed().as_secs_f32();
        let ident: gl::Mat4 = num_traits::one();
        let rot = gl::ext::rotate(&ident, t, gl::vec3(0.0, 0.0, 1.0));
        let proj: gl::Mat4 = num_traits::one();
        let mvp = proj * rot;

        // Update CPU upload buffer, then copy to device-local MPV buffer.
        let mpv_data = MPV_PushConstants { mpv: mvp };
        let mpv_bytes = unsafe {
            std::slice::from_raw_parts(
                (&mpv_data as *const MPV_PushConstants) as *const u8,
                std::mem::size_of::<MPV_PushConstants>(),
            )
        };
        mpv_upload_buffer
            .write(mpv_bytes)
            .map_err(|e| e.to_string())?;

        let single_command = context
            .begin_single_time_commands()
            .map_err(|e| e.to_string())?;

        single_command
            .copy_vk_buffer(
                mpv_upload_buffer.vk_buffer(),
                mpv_buffer.vk_buffer(),
                std::mem::size_of::<MPV_PushConstants>(),
                0,
                0,
            )
            .map_err(|e| e.to_string())?;
        context.end_single_time_commands(single_command).unwrap();

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

    unsafe {
        rust_and_vulkan::SDL_StopTextInput(window.window);
    }

    Ok(())
}
