#version 460
#extension GL_EXT_nonuniform_qualifier : require
#extension GL_EXT_shader_explicit_arithmetic_types : require

// Bindless runtime array of combined image samplers (descriptor-buffer, set=0, binding=0).
layout(set = 0, binding = 0) uniform sampler2D textures[];

layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;

layout(location = 0) out vec4 out_color;

layout(push_constant) uniform PushConstants {
    uint64_t vertex_ptr;    // 8 bytes (used by vertex shader)
    float window_width;     // 4 bytes (used by vertex shader)
    float window_height;    // 4 bytes (used by vertex shader)
    uint texture_index;     // 4 bytes (used by fragment shader)
} pc;

void main() {
    float atlas_alpha = texture(textures[nonuniformEXT(pc.texture_index)], v_uv).a;
    out_color = v_color * atlas_alpha;
}
