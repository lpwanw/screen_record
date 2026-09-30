use std::{path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let output = Command::new("xcrun")
        .args(["--find", "swift"])
        .output()
        .expect("xcrun is available");
    let swift = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let runtime = swift
        .ancestors()
        .nth(2)
        .expect("swift lives in <toolchain>/usr/bin")
        .join("lib/swift/macosx");
    println!("cargo:rustc-link-search=native={}", runtime.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}
