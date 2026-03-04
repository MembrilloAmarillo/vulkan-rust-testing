#version 450
#extension GL_EXT_buffer_reference : require
#extension GL_EXT_scalar_block_layout : require
#extension GL_EXT_nonuniform_qualifier : require

layout(location = 0) in vec3 v_color;
layout(location = 1) in vec2 v_uv;
layout(location = 0) out vec4 outColor;

// Push constant data matching vertex shader
layout(push_constant, std430) uniform PushConstants {
    mat4 mvp;
    uint texture_heap_lo;
    uint texture_heap_hi;
    uint texture_index;
    uint padding;
} push;

// Descriptor reference (simplified - in a real implementation this would be vkGetDescriptorEXT)
// For now, we'll sample the texture indirectly through the heap pointer
void main() {
    // For this demo, we blend the vertex colors with a texture-like pattern
    // A full bindless implementation would use vkGetDescriptorEXT and sample from the heap
    
    // Create a procedural pattern based on UV coordinates and vertex color
    vec2 tiled_uv = fract(v_uv * 4.0);
    float pattern = step(0.5, fract(tiled_uv.x + tiled_uv.y));
    
    vec3 base_color = v_color;
    vec3 pattern_color = mix(v_color * 0.5, v_color, pattern);
    
    // Apply some variation based on position for visual interest
    vec3 final_color = mix(base_color, pattern_color, 0.5);
    
    outColor = vec4(final_color, 1.0);
}
