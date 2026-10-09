const UI_WHITE_NITS: f32 = 200.0;
const SCRGB_WHITE_NITS: f32 = 80.0;
const M1: f32 = 2610.0 / 16384.0;
const M2: f32 = 2523.0 / 32.0;
const C1: f32 = 3424.0 / 4096.0;
const C2: f32 = 2413.0 / 128.0;
const C3: f32 = 2392.0 / 128.0;

fn pq_decode(value: vec3<f32>) -> vec3<f32> {
    let power = pow(max(value, vec3<f32>(0.0)), vec3<f32>(1.0 / M2));
    return pow(max(power - vec3<f32>(C1), vec3<f32>(0.0)) / max(vec3<f32>(C2) - C3 * power, vec3<f32>(0.000001)), vec3<f32>(1.0 / M1)) * (10000.0 / UI_WHITE_NITS);
}
fn pq_encode(value: vec3<f32>) -> vec3<f32> {
    let power = pow(clamp(value * (UI_WHITE_NITS / 10000.0), vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(M1));
    return pow((vec3<f32>(C1) + C2 * power) / (vec3<f32>(1.0) + C3 * power), vec3<f32>(M2));
}
fn to_rec709(value: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(value, vec3<f32>(1.660491, -0.587641, -0.072850)),
        dot(value, vec3<f32>(-0.124550, 1.132900, -0.008349)),
        dot(value, vec3<f32>(-0.018151, -0.100579, 1.118730)));
}
fn to_rec2020(value: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(value, vec3<f32>(0.627404, 0.329283, 0.043313)),
        dot(value, vec3<f32>(0.069097, 0.919540, 0.011362)),
        dot(value, vec3<f32>(0.016391, 0.088013, 0.895595)));
}
fn composite(original: vec4<f32>, ui: vec4<f32>, encoding: u32) -> vec4<f32> {
    if ui.a == 0.0 { return original; }
    var linear: vec3<f32>;
    if encoding == 0u { linear = to_rec709(pq_decode(original.rgb)); }
    else { linear = original.rgb * (SCRGB_WHITE_NITS / UI_WHITE_NITS); }
    let combined = ui.rgb + linear * (1.0 - ui.a);
    if encoding == 0u { return vec4<f32>(pq_encode(to_rec2020(combined)), original.a); }
    return vec4<f32>(combined * (UI_WHITE_NITS / SCRGB_WHITE_NITS), original.a);
}
