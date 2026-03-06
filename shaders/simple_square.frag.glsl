#version 450
#extension GL_EXT_nonuniform_qualifier : require

layout(location = 0) in vec3 v_color;
layout(location = 0) out vec4 out_color;

// Minimal push constants for bindless texture selection.
// Keep this small so it can fit alongside other push constants if needed.
layout(push_constant, std430) uniform PushConstants {
    uint texture_index;
} push;

// Bindless runtime array of combined image samplers.
// Expected to be provided via VK_EXT_descriptor_buffer binding at set=0,binding=0.
layout(set = 0, binding = 0) uniform sampler2D textures[];

// Temporary UVs (until you pass real UVs from the vertex stage).
vec2 default_uv()
{
    return fract(gl_FragCoord.xy * 0.01);
}

void main()
{
    vec2 uv = default_uv();

    // Non-uniform indexing is required for bindless arrays.
    vec4 t = texture(textures[nonuniformEXT(push.texture_index)], uv);

    // Treat sampled texture red channel as alpha mask (handy for glyph atlases).
    float a = t.r;

    out_color = vec4(v_color, a);
}
