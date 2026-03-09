//! egui context manager with SDL3 input handling.
//!
//! Handles:
//! - egui::Context lifecycle
//! - SDL3 event → egui input conversion
//! - UI state (panels, selections, etc.)

pub struct EguiManager {
    pub ctx: egui::Context,
    pointer_pos: egui::Pos2,
    raw_input: egui::RawInput,
    start_time: std::time::Instant,
    // UI state (using static string references to avoid allocations)
    pub selected_option: &'static str,
    pub data_display: &'static str,
}

impl EguiManager {
    pub fn new() -> Self {
        let ctx = egui::Context::default();

        ctx.set_visuals(egui::Visuals::light());

        // Configure egui for better performance
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::Vec2::new(8.0, 6.0);
        ctx.set_style(style);

        EguiManager {
            ctx,
            pointer_pos: egui::Pos2::ZERO,
            raw_input: egui::RawInput::default(),
            start_time: std::time::Instant::now(),
            selected_option: "None",
            data_display: "No data selected",
        }
    }

    /// Process SDL3 event and feed to egui
    pub fn handle_event(&mut self, event: &crate::SDL_Event) {
        unsafe {
            match event.type_ {
                t if t == crate::SDL_EventType::SDL_EVENT_MOUSE_MOTION as u32 => {
                    let motion = &event.motion;
                    self.pointer_pos = egui::Pos2::new(motion.x, motion.y);
                    self.raw_input
                        .events
                        .push(egui::Event::PointerMoved(self.pointer_pos));
                }
                t if t == crate::SDL_EventType::SDL_EVENT_MOUSE_BUTTON_DOWN as u32 => {
                    let btn = &event.button;
                    self.pointer_pos = egui::Pos2::new(btn.x, btn.y);
                    self.raw_input.events.push(egui::Event::PointerButton {
                        pos: self.pointer_pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    });
                }
                t if t == crate::SDL_EventType::SDL_EVENT_MOUSE_BUTTON_UP as u32 => {
                    let btn = &event.button;
                    self.pointer_pos = egui::Pos2::new(btn.x, btn.y);
                    self.raw_input.events.push(egui::Event::PointerButton {
                        pos: self.pointer_pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    });
                }
                t if t == crate::SDL_EventType::SDL_EVENT_MOUSE_WHEEL as u32 => {
                    let wheel = &event.wheel;
                    let delta = egui::Vec2::new(wheel.x, wheel.y) * 50.0;
                    self.raw_input.events.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta,
                        modifiers: egui::Modifiers::default(),
                    });
                }
                t if t == crate::SDL_EventType::SDL_EVENT_KEY_DOWN as u32 => {
                    let key = &event.key;
                    if let Some(egui_key) = sdl_key_to_egui(key.key) {
                        self.raw_input.events.push(egui::Event::Key {
                            key: egui_key,
                            physical_key: None,
                            pressed: true,
                            repeat: key.repeat as u8 != 0,
                            modifiers: egui::Modifiers::default(),
                        });
                    }
                }
                t if t == crate::SDL_EventType::SDL_EVENT_KEY_UP as u32 => {
                    let key = &event.key;
                    if let Some(egui_key) = sdl_key_to_egui(key.key) {
                        self.raw_input.events.push(egui::Event::Key {
                            key: egui_key,
                            physical_key: None,
                            pressed: false,
                            repeat: false,
                            modifiers: egui::Modifiers::default(),
                        });
                    }
                }
                t if t == crate::SDL_EventType::SDL_EVENT_TEXT_INPUT as u32 => {
                    let text = &event.text;
                    let cstr = std::ffi::CStr::from_ptr(text.text);
                    if let Ok(s) = cstr.to_str() {
                        self.raw_input.events.push(egui::Event::Text(s.to_string()));
                    }
                }
                _ => {}
            }
        }
    }

    /// Begin UI frame
    pub fn begin_frame(&mut self, screen_width: f32, screen_height: f32) {
        self.raw_input.screen_rect = Some(egui::Rect::from_min_max(
            egui::Pos2::ZERO,
            egui::Pos2::new(screen_width, screen_height),
        ));
        self.raw_input.time = Some(self.start_time.elapsed().as_secs_f64());
        let raw_input = std::mem::take(&mut self.raw_input);
        self.ctx.begin_pass(raw_input);
        // Ensure events vector is cleared and reset to empty state
        self.raw_input.events.clear();
        self.raw_input = egui::RawInput::default();
    }

    /// End UI frame and get tessellated output
    pub fn end_frame(&mut self) -> (Vec<egui::ClippedPrimitive>, egui::TexturesDelta) {
        let output = self.ctx.end_pass();
        let shapes = self.ctx.tessellate(output.shapes, 1.0);
        (shapes, output.textures_delta)
    }

    /// Get egui context for advanced usage
    pub fn context(&self) -> &egui::Context {
        &self.ctx
    }

    /// Update selected option
    pub fn set_selected_option(&mut self, option: &'static str) {
        self.selected_option = option;
    }

    /// Update data display
    pub fn set_data_display(&mut self, data: &'static str) {
        self.data_display = data;
    }
}

/// Convert SDL3 key code to egui key
fn sdl_key_to_egui(key: crate::SDL_Keycode) -> Option<egui::Key> {
    match key {
        crate::SDLK_BACKSPACE => Some(egui::Key::Backspace),
        crate::SDLK_DELETE => Some(egui::Key::Delete),
        crate::SDLK_RETURN => Some(egui::Key::Enter),
        crate::SDLK_TAB => Some(egui::Key::Tab),
        crate::SDLK_LEFT => Some(egui::Key::ArrowLeft),
        crate::SDLK_RIGHT => Some(egui::Key::ArrowRight),
        crate::SDLK_UP => Some(egui::Key::ArrowUp),
        crate::SDLK_DOWN => Some(egui::Key::ArrowDown),
        crate::SDLK_HOME => Some(egui::Key::Home),
        crate::SDLK_END => Some(egui::Key::End),
        crate::SDLK_PAGEUP => Some(egui::Key::PageUp),
        crate::SDLK_PAGEDOWN => Some(egui::Key::PageDown),
        crate::SDLK_ESCAPE => Some(egui::Key::Escape),
        crate::SDLK_A => Some(egui::Key::A),
        crate::SDLK_C => Some(egui::Key::C),
        crate::SDLK_V => Some(egui::Key::V),
        crate::SDLK_X => Some(egui::Key::X),
        crate::SDLK_Z => Some(egui::Key::Z),
        _ => None,
    }
}
