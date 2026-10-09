@group(0) @binding(0) var background: texture_2d<f32>;
@group(0) @binding(1) var overlay: texture_2d<f32>;
var<immediate> encoding: u32;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(positions[index], 0.0, 1.0);
}
@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let original = textureLoad(background, pixel, 0);
    let ui = textureLoad(overlay, pixel, 0);
    return composite(original, ui, encoding);
}
