fn main() {
    prost_build::compile_protos(&["src/protos/ipc.proto"], &["src/"]).unwrap();
    println!("cargo:rustc-link-search=external/sdl2");
}
