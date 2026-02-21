use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    if arch == "aarch64" {
        // Determine which boot file and linker script to use based on features
        let sel2 = env::var("CARGO_FEATURE_SEL2").is_ok();

        let boot_asm = if sel2 {
            "arch/aarch64/boot_sel2.S"
        } else {
            "arch/aarch64/boot.S"
        };

        let linker_script = if sel2 {
            "arch/aarch64/linker_sel2.ld"
        } else {
            "arch/aarch64/linker.ld"
        };

        // List of assembly files to compile
        let asm_files = [
            (boot_asm, "boot.o"),
            ("arch/aarch64/exception.S", "exception.o"),
        ];

        let mut object_files = Vec::new();

        // Compile each assembly file
        for (asm_src, obj_name) in &asm_files {
            let obj_path = out_dir.join(obj_name);

            println!("cargo:rerun-if-changed={}", asm_src);

            // Try gcc first, fall back to as
            let status = Command::new("aarch64-linux-gnu-gcc")
                .args(&[
                    "-c",
                    asm_src,
                    "-o",
                    obj_path.to_str().unwrap(),
                    "-nostdlib",
                    "-ffreestanding",
                ])
                .status()
                .or_else(|_| {
                    // Fallback to using assembler directly
                    Command::new("aarch64-linux-gnu-as")
                        .args(&[asm_src, "-o", obj_path.to_str().unwrap()])
                        .status()
                })
                .unwrap_or_else(|_| panic!("Failed to compile {}", asm_src));

            assert!(status.success(), "Failed to compile {}", asm_src);

            object_files.push(obj_path);
        }

        // Create archive with all object files
        let boot_a = out_dir.join("libboot.a");
        let mut ar_cmd = Command::new("aarch64-linux-gnu-ar");
        ar_cmd.arg("crs").arg(boot_a.to_str().unwrap());

        for obj in &object_files {
            ar_cmd.arg(obj.to_str().unwrap());
        }

        let status = ar_cmd.status().expect("Failed to create archive");

        assert!(status.success(), "Failed to create archive");

        // Output link search path
        println!("cargo:rustc-link-search=native={}", out_dir.display());

        // Linker script (feature-gated: sel2 uses linker_sel2.ld)
        println!("cargo:rerun-if-changed={}", linker_script);
        println!("cargo:rustc-link-arg=-T{}", linker_script);

        // Output link directives with whole-archive
        println!("cargo:rustc-link-arg=--whole-archive");
        println!("cargo:rustc-link-lib=static=boot");
        println!("cargo:rustc-link-arg=--no-whole-archive");
    }
}
