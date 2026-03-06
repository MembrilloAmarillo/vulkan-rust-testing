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
    mat4 projection;
} pc;

layout(buffer_reference, scalar) readonly buffer VertexBuffer {
    Vertex data[];
};

layout(location = 0) out vec2 v_uv;
layout(location = 1) out vec4 v_color;

void main() {
    uint idx = gl_VertexIndex;
    
    VertexBuffer vb = VertexBuffer(pc.vertex_ptr);
    Vertex v = vb.data[idx];
    
    gl_Position = pc.projection * vec4(v.position, 0.0, 1.0);
    
    v_uv = v.uv;
    // Unpack sRGB color from u32
    vec4 color = unpackUnorm4x8(v.color);
    // Convert from sRGB to linear
    v_color = vec4(pow(color.rgb, vec3(2.2)), color.a);
}
