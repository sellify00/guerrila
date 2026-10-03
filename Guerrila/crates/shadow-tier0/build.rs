fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let dpdk_enabled = std::env::var_os("CARGO_FEATURE_DPDK").is_some();
    if target_os != "linux" || !dpdk_enabled {
        return;
    }

    println!("cargo:rerun-if-changed=src/dpdk_shim.c");

    let libdpdk = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("libdpdk")
        .expect("dpdk feature requires pkg-config libdpdk on Linux");

    let mut build = cc::Build::new();
    build.file("src/dpdk_shim.c");
    build.include("src");
    build.cargo_metadata(true);

    for include in &libdpdk.include_paths {
        build.include(include);
    }
    for (key, value) in &libdpdk.defines {
        build.define(key, value.as_deref());
    }
    apply_pkg_config_cflags(&mut build, "libdpdk");

    build.compile("shadow_dpdk_shim");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set for DPDK build");
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static=shadow_dpdk_shim");
    emit_pkg_config_link_args("libdpdk");
}

fn apply_pkg_config_cflags(build: &mut cc::Build, package: &str) {
    let output = std::process::Command::new("pkg-config")
        .args(["--cflags", package])
        .output()
        .unwrap_or_else(|err| panic!("failed to query pkg-config cflags for {package}: {err}"));
    if !output.status.success() {
        panic!(
            "pkg-config --cflags {package} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let cflags = String::from_utf8_lossy(&output.stdout);
    let mut args = cflags.split_whitespace().peekable();
    while let Some(arg) = args.next() {
        if let Some(rest) = arg.strip_prefix("-I") {
            if rest.is_empty() {
                if let Some(path) = args.next() {
                    build.include(path);
                }
            } else {
                build.include(rest);
            }
            continue;
        }
        if let Some(rest) = arg.strip_prefix("-D") {
            if let Some((key, value)) = rest.split_once('=') {
                build.define(key, Some(value));
            } else if !rest.is_empty() {
                build.define(rest, None);
            }
            continue;
        }
        build.flag(arg);
    }
}

fn emit_pkg_config_link_args(package: &str) {
    let output = std::process::Command::new("pkg-config")
        .args(["--libs", "--static", package])
        .output()
        .unwrap_or_else(|err| panic!("failed to query pkg-config libs for {package}: {err}"));
    if !output.status.success() {
        panic!(
            "pkg-config --libs {package} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let libs = String::from_utf8_lossy(&output.stdout);
    for arg in libs.split_whitespace() {
        println!("cargo:rustc-link-arg={arg}");
    }
}
