#include <metal_stdlib>
using namespace metal;
struct Quad { float4 rect; float4 color; };
struct Raster { float4 position [[position]]; float2 uv; float opacity; };
vertex Raster quad_vertex(uint id [[vertex_id]], constant Quad& quad [[buffer(0)]]) {
    const float2 corners[] = {float2(0,0),float2(1,0),float2(0,1),float2(1,1)};
    float2 uv = corners[id];
    Raster out;
    out.position = float4(quad.rect.x + uv.x * quad.rect.z, quad.rect.y - uv.y * quad.rect.w, 0, 1);
    out.uv = uv;
    out.opacity = quad.color.x;
    return out;
}
fragment float4 quad_fragment(Raster in [[stage_in]], texture2d<float> image [[texture(0)]]) {
    constexpr sampler sampling(filter::linear, address::clamp_to_edge);
    float4 pixel = image.sample(sampling,in.uv);
    return float4(pixel.rgb,pixel.a*in.opacity);
}
