use std::{env, fs, path::PathBuf};

fn main() {
    let path = "src/overlay/hdr.wgsl";
    println!("cargo:rerun-if-changed={path}");
    let math = "src/overlay/hdr_math.wgsl";
    println!("cargo:rerun-if-changed={math}");
    let math = fs::read_to_string(math).expect("HDR colour functions");
    let source = math + &fs::read_to_string(path).expect("HDR shader source");
    let module = naga::front::wgsl::parse_str(&source)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("valid HDR shader");
    for (stage, entry, name) in [
        (naga::ShaderStage::Vertex, "vertex", "hdr.vertex.spv"),
        (naga::ShaderStage::Fragment, "fragment", "hdr.fragment.spv"),
    ] {
        let words = naga::back::spv::write_vec(
            &module,
            &info,
            &naga::back::spv::Options::default(),
            Some(&naga::back::spv::PipelineOptions {
                shader_stage: stage,
                entry_point: entry.into(),
            }),
        )
        .expect("HDR SPIR-V generation");
        let bytes: Vec<_> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        fs::write(
            PathBuf::from(env::var_os("OUT_DIR").unwrap()).join(name),
            bytes,
        )
        .expect("compiled HDR shader");
    }
}
