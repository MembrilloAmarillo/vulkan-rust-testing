#version 460
#extension GL_EXT_buffer_reference : require
#extension GL_EXT_scalar_block_layout : require
#extension GL_EXT_shader_explicit_arithmetic_types : require

struct Vertex {
    vec2 position;
    vec2 uv;
    uint color;
};

layout(push_constant) uniform PushConstants {
    uint64_t vertex_ptr;
    float window_width;
    float window_height;
} pc;

layout(buffer_reference, scalar) readonly buffer VertexBuffer {
    Vertex data[];
};

layout(location = 0) out vec2 v_uv;
layout(location = 1) out vec4 v_color;

void main() {
    VertexBuffer vb = VertexBuffer(pc.vertex_ptr);
    Vertex v = vb.data[gl_VertexIndex];

    // egui screen-space → Vulkan NDC; flip Y so (0,0) top-left maps to (-1,-1)
    vec2 ndc = vec2(
        (2.0 * v.position.x / pc.window_width)  - 1.0,
        1.0 - (2.0 * v.position.y / pc.window_height)
    );

    gl_Position = vec4(ndc, 0.0, 1.0);
    v_uv    = v.uv;
    v_color = unpackUnorm4x8(v.color);
}
