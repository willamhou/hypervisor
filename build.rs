use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    if arch == "aarch64" {
        // Compile boot.S
        let boot_s = "arch/aarch64/boot.S";
        let boot_o = out_dir.join("boot.o");

        println!("cargo:rerun-if-changed={}", boot_s);

        let status = Command::new("aarch64-linux-gnu-gcc")
            .args(&[
                "-c",
                boot_s,
                "-o",
                boot_o.to_str().unwrap(),
                "-nostdlib",
                "-ffreestanding",
            ])
            .status()
            .expect("Failed to compile boot.S");

        assert!(status.success(), "Failed to compile boot.S");

        // Create archive
        let boot_a = out_dir.join("libboot.a");
        let status = Command::new("aarch64-linux-gnu-ar")
            .args(&["crs", boot_a.to_str().unwrap(), boot_o.to_str().unwrap()])
            .status()
            .expect("Failed to create archive");

        assert!(status.success(), "Failed to create archive");

        // Link the archive
        println!("cargo:rustc-link-search=native={}", out_dir.display());
        println!("cargo:rustc-link-lib=static=boot");
    }
}
