#version 460
#extension GL_EXT_buffer_reference : require
#extension GL_EXT_scalar_block_layout : require
#extension GL_EXT_shader_explicit_arithmetic_types : require

layout(push_constant) uniform RootPointer {
    uint root_ptr_lo;
    uint root_ptr_hi;
} pc;

struct Vertex {
    vec3 pos;
    vec3 normal;
    vec2 uv;
};

layout(buffer_reference, scalar) readonly buffer VertexBuffer {
    Vertex data[];
};

layout(buffer_reference, scalar) readonly buffer DrawData {
    uint64_t vertex_ptr;       // offset 0, 8 bytes
    uint texture_index;         // offset 8, 4 bytes  
    uint material_mode;         // offset 12, 4 bytes
    mat4 mvp;                   // offset 16, 64 bytes (with scalar layout, no padding needed)
};

layout(location = 0) out vec3 v_normal;
layout(location = 1) out vec2 v_uv;
layout(location = 2) flat out uint v_texture_index;
layout(location = 3) flat out uint v_material_mode;

void main() {
    uint64_t root_ptr = (uint64_t(pc.root_ptr_hi) << 32) | uint64_t(pc.root_ptr_lo);
    DrawData draw = DrawData(root_ptr);
    VertexBuffer vb = VertexBuffer(draw.vertex_ptr);

    Vertex v = vb.data[gl_VertexIndex];

    v_normal = normalize(v.normal);
    v_uv = v.uv;
    v_texture_index = draw.texture_index;
    v_material_mode = draw.material_mode;

    gl_Position = draw.mvp * vec4(v.pos, 1.0);
}
