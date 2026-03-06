#version 460

layout(set = 0, binding = 0) uniform sampler2D font_texture;

layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;

layout(location = 0) out vec4 out_color;

void main() {
    // Sample the font atlas alpha. For colored (non-text) geometry the UVs
    // point to a solid-white region of the atlas so the sample returns ~1.0.
    float atlas_alpha = texture(font_texture, v_uv).a;
    out_color = v_color * atlas_alpha;
}
