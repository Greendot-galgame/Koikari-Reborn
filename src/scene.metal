#include <metal_stdlib>
using namespace metal;
struct Quad {
    float4 rect;
    float4 parameters;
    float4 gamma;
    float4 floor;
    float4 ceiling;
    float4 tint;
};
struct Raster { float4 position [[position]]; float2 uv; };
vertex Raster quad_vertex(uint id [[vertex_id]], constant Quad& quad [[buffer(0)]]) {
    const float2 corners[] = {float2(0,0),float2(1,0),float2(0,1),float2(1,1)};
    float2 uv = corners[id];
    Raster out;
    out.position = float4(quad.rect.x + uv.x * quad.rect.z, quad.rect.y - uv.y * quad.rect.w, 0, 1);
    out.uv = uv;
    return out;
}
fragment float4 quad_fragment(Raster in [[stage_in]], constant Quad& q [[buffer(0)]], texture2d<float> image [[texture(0)]]) {
    constexpr sampler sampling(filter::linear, address::clamp_to_edge);
    float4 pixel = image.sample(sampling,in.uv);
    if (q.parameters.y > 0.5) pixel.rgb = dot(pixel.rgb,float3(0.299,0.587,0.114));
    pixel.rgb = mix(q.floor.rgb,q.ceiling.rgb,pow(max(pixel.rgb,float3(0)),1.0/q.gamma.rgb));
    pixel.rgb = mix(pixel.rgb,q.tint.rgb,q.tint.a);
    return float4(clamp(pixel.rgb,0.0,1.0),pixel.a*q.parameters.x);
}
vertex Raster fullscreen_vertex(uint id [[vertex_id]]) {
    const float2 corners[] = {float2(0,0),float2(1,0),float2(0,1),float2(1,1)};
    Raster out;
    out.uv = corners[id];
    out.position = float4(out.uv.x*2-1,1-out.uv.y*2,0,1);
    return out;
}
fragment float4 composite_fragment(Raster in [[stage_in]], constant float4* parameters [[buffer(0)]],
    texture2d<float> previous [[texture(0)]], texture2d<float> current [[texture(1)]], texture2d<float> rule [[texture(2)]]) {
    constexpr sampler sampling(filter::linear,address::clamp_to_edge);
    float4 effect = parameters[0];
    float2 beforeUV = in.uv, afterUV = in.uv;
    if (effect.y > 2.5) {
        beforeUV = (in.uv-0.5)/parameters[1].x+0.5;
        afterUV = (in.uv-0.5)/parameters[1].y+0.5;
    }
    float4 before = previous.sample(sampling,beforeUV);
    float4 after = current.sample(sampling,afterUV);
    float p = clamp(effect.x,0.0,1.0);
    if (p <= 0.0) return before;
    if (p >= 1.0) return after;
    if (effect.y > 1.5 && effect.y < 2.5) {
        // Authentic whiteout: 600ms to opaque white, then 600ms to target.
        return p < 0.5 ? mix(before,float4(1),p*2) : mix(float4(1),after,(p-0.5)*2);
    }
    if (effect.y > 0.5 && effect.y < 1.5) {
        float value = rule.sample(sampling,in.uv).r;
        float vague = max(effect.z,1.0/65536.0);
        p = clamp((p*(1+vague)-value)/vague,0.0,1.0);
    }
    // Mix whole, already-composited scenes so face/body alpha stays correct.
    return mix(before,after,p);
}
