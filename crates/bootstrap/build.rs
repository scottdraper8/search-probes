use std::{
    collections::BTreeSet,
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
struct Export {
    ordinal: u16,
    name: Option<String>,
}

fn parse_exports(path: &Path) -> Vec<Export> {
    let input = fs::read_to_string(path).expect("read winmm.exports");
    let mut exports = Vec::new();
    let mut names = BTreeSet::new();
    for (line_number, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let ordinal: u16 = fields
            .next()
            .unwrap_or_default()
            .parse()
            .unwrap_or_else(|_| panic!("invalid WinMM ordinal on line {}", line_number + 1));
        let raw_name = fields
            .next()
            .unwrap_or_else(|| panic!("missing WinMM name on line {}", line_number + 1));
        assert!(
            fields.next().is_none(),
            "extra WinMM field on line {}",
            line_number + 1
        );
        let name = if raw_name == "-" {
            None
        } else {
            assert!(
                raw_name
                    .bytes()
                    .enumerate()
                    .all(|(index, byte)| byte == b'_'
                        || byte.is_ascii_alphabetic()
                        || (index > 0 && byte.is_ascii_digit())),
                "invalid WinMM export name on line {}",
                line_number + 1
            );
            assert!(names.insert(raw_name.to_owned()), "duplicate WinMM name");
            Some(raw_name.to_owned())
        };
        exports.push(Export { ordinal, name });
    }
    exports
}

fn validate(exports: &[Export]) {
    assert_eq!(
        exports.len(),
        189,
        "complete WinMM union must have 189 exports"
    );
    for (index, export) in exports.iter().enumerate() {
        assert_eq!(
            export.ordinal as usize,
            index + 2,
            "WinMM ordinals must preserve the Windows 10/11 sequence"
        );
    }
    assert!(exports[0].name.is_none(), "ordinal 2 must remain unnamed");
    let names: BTreeSet<_> = exports
        .iter()
        .filter_map(|export| export.name.as_deref())
        .collect();
    for required in [
        "timeBeginPeriod",
        "timeEndPeriod",
        "waveOutReset",
        "waveOutOpen",
        "waveInOpen",
        "mixerOpen",
        "timeSetEvent",
    ] {
        assert!(
            names.contains(required),
            "missing required WinMM export {required}"
        );
    }
}

fn generate_assembly(exports: &[Export]) -> String {
    let mut output = String::from(
        "# Generated from winmm.exports. Better Flight contributors supplied the source inventory.\n\
         .text\n",
    );
    for (index, export) in exports.iter().enumerate() {
        let symbol = export
            .name
            .clone()
            .unwrap_or_else(|| format!("ord_{}", export.ordinal));
        writeln!(
            output,
            ".globl sp_x_{symbol}\n\
             .p2align 3\n\
             sp_x_{symbol}:\n\
                 movq {{table}}+{offset}(%rip), %rax\n\
                 testq %rax, %rax\n\
                 jz 1f\n\
                 jmpq *%rax\n\
             1:\n\
                 movl ${index}, %r10d\n\
                 jmp sp_winmm_lazy_common",
            offset = index * 8,
        )
        .expect("write WinMM assembly");
    }
    output.push_str(
        ".globl sp_winmm_lazy_common\n\
         .seh_proc sp_winmm_lazy_common\n\
         sp_winmm_lazy_common:\n\
             pushq %rcx\n\
             .seh_pushreg %rcx\n\
             pushq %rdx\n\
             .seh_pushreg %rdx\n\
             pushq %r8\n\
             .seh_pushreg %r8\n\
             pushq %r9\n\
             .seh_pushreg %r9\n\
             pushq %rax\n\
             .seh_pushreg %rax\n\
             pushq %r10\n\
             .seh_pushreg %r10\n\
             subq $0x68, %rsp\n\
             .seh_stackalloc 0x68\n\
             .seh_endprologue\n\
             movdqu %xmm0, 0x20(%rsp)\n\
             movdqu %xmm1, 0x30(%rsp)\n\
             movdqu %xmm2, 0x40(%rsp)\n\
             movdqu %xmm3, 0x50(%rsp)\n\
             call {resolver}\n\
             movdqu 0x20(%rsp), %xmm0\n\
             movdqu 0x30(%rsp), %xmm1\n\
             movdqu 0x40(%rsp), %xmm2\n\
             movdqu 0x50(%rsp), %xmm3\n\
             addq $0x68, %rsp\n\
             popq %r10\n\
             popq %rax\n\
             popq %r9\n\
             popq %r8\n\
             popq %rdx\n\
             popq %rcx\n\
             leaq {table}(%rip), %r11\n\
             jmpq *(%r11,%r10,8)\n\
         .seh_endproc\n\
         .globl sp_time_begin_export\n\
         sp_time_begin_export:\n\
             jmp {time_begin}\n\
         .globl sp_time_end_export\n\
         sp_time_end_export:\n\
             jmp {time_end}\n",
    );
    output
}

fn generate_definition(exports: &[Export]) -> String {
    let mut output = String::from(
        "; Generated from winmm.exports. Better Flight contributors supplied the source inventory.\n\
         LIBRARY winmm\n\
         EXPORTS\n",
    );
    for export in exports {
        match export.name.as_deref() {
            None => writeln!(
                output,
                "    sp_x_ord_{} @{} NONAME",
                export.ordinal, export.ordinal
            ),
            Some("timeBeginPeriod") => writeln!(
                output,
                "    timeBeginPeriod = sp_time_begin_export @{}",
                export.ordinal
            ),
            Some("timeEndPeriod") => writeln!(
                output,
                "    timeEndPeriod = sp_time_end_export @{}",
                export.ordinal
            ),
            Some(name) => writeln!(output, "    {name} = sp_x_{name} @{}", export.ordinal),
        }
        .expect("write WinMM definition");
    }
    output
}

fn generate_rust(exports: &[Export]) -> String {
    let mut output = format!(
        "const WINMM_EXPORT_COUNT: usize = {};\n\
         const WINMM_TIME_BEGIN_PERIOD: usize = {};\n\
         const WINMM_TIME_END_PERIOD: usize = {};\n\
         const WINMM_EXPORTS: [WinmmExport; WINMM_EXPORT_COUNT] = [\n",
        exports.len(),
        exports
            .iter()
            .position(|export| export.name.as_deref() == Some("timeBeginPeriod"))
            .expect("timeBeginPeriod index"),
        exports
            .iter()
            .position(|export| export.name.as_deref() == Some("timeEndPeriod"))
            .expect("timeEndPeriod index"),
    );
    for export in exports {
        match export.name.as_deref() {
            Some(name) => writeln!(output, "    WinmmExport::Name(c\"{name}\"),"),
            None => writeln!(output, "    WinmmExport::Ordinal({}),", export.ordinal),
        }
        .expect("write WinMM Rust inventory");
    }
    output.push_str("];\n");
    output
}

fn main() {
    println!("cargo:rerun-if-changed=winmm.exports");
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ENV").as_deref(),
        Ok("gnu"),
        "Search Probes currently supports the Windows GNU target"
    );
    let exports = parse_exports(Path::new("winmm.exports"));
    validate(&exports);
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let assembly = output.join("winmm_stubs.s");
    let definition = output.join("winmm.def");
    fs::write(&assembly, generate_assembly(&exports)).expect("write WinMM assembly");
    fs::write(&definition, generate_definition(&exports)).expect("write WinMM definition");
    fs::write(output.join("winmm_exports.rs"), generate_rust(&exports))
        .expect("write WinMM Rust inventory");
    println!("cargo:rustc-cdylib-link-arg={}", definition.display());
}
