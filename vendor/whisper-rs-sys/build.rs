use std::{env, path::PathBuf};

fn main() {
    // This metadata comes from our direct dependency, so Cargo builds GGML first.
    let cmake_dir = PathBuf::from(
        env::var_os("DEP_LLAMA_GGML_CMAKE_DIR")
            .expect("llama-cpp-sys-2 must provide its GGML CMake package"),
    );
    let prefix = cmake_dir.parent().unwrap().parent().unwrap();
    let include = prefix.join("include");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    println!("cargo:rerun-if-changed=CMakeLists.txt");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=whisper.cpp");
    // Generate against the very same GGML headers used to compile Whisper.
    bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg("-Iwhisper.cpp/include")
        .clang_arg(format!("-I{}", include.display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("could not generate Whisper bindings")
        .write_to_file(out.join("bindings.rs"))
        .expect("could not write Whisper bindings");

    let destination = cmake::Config::new(".")
        .profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("WHISPER_USE_SYSTEM_GGML", "ON")
        .define("ggml_DIR", cmake_dir.join("ggml"))
        .define("WHISPER_BUILD_TESTS", "OFF")
        .define("WHISPER_BUILD_EXAMPLES", "OFF")
        .define("WHISPER_ALL_WARNINGS", "OFF")
        .define("CMAKE_INSTALL_LIBDIR", "lib")
        .pic(true)
        .build();
    println!(
        "cargo:rustc-link-search=native={}",
        destination.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=whisper");
    println!("cargo:WHISPER_CPP_VERSION=1.8.3");
    // GGML and platform libraries are linked by llama-cpp-sys-2, never here.
}
