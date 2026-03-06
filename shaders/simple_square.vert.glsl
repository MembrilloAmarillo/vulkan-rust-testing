#version 460
#extension GL_EXT_buffer_reference : require
#extension GL_EXT_scalar_block_layout : require
#extension GL_EXT_shader_explicit_arithmetic_types : require

struct Vertex {
    vec2 pos;
    vec3 normal;
    vec2 uv;
};

layout(push_constant) uniform PushConstants {
    uint64_t vertex_ptr;
    uint64_t mpv_ptr;
} pc;

layout(buffer_reference, scalar) readonly buffer MPVBuffer {
    mat4 mvp;
};

layout(buffer_reference, scalar) readonly buffer VertexBuffer {
    Vertex data[];
};

layout(location = 0) out vec3 v_color;

void main() {
    uint idx = uint(gl_VertexIndex) % 6u;
    VertexBuffer vb = VertexBuffer(pc.vertex_ptr);
    MPVBuffer mpv = MPVBuffer(pc.mpv_ptr);
    
    Vertex v = vb.data[idx];
    vec2 position = v.pos;
    vec3 color = v.normal;

    v_color = color;
    gl_Position = mpv.mvp * vec4(position, 0.0, 1.0);
}
