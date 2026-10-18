//! Compiles the vendored ProPresenter 7 protobuf definitions (MIT, see
//! `proto/propresenter/LICENSE`) with `protox`, so `protoc` is not needed.

fn main() {
    let dir = "proto/propresenter";
    println!("cargo:rerun-if-changed={dir}");
    let fds = protox::compile(["presentation.proto"], [dir]).expect("compile ProPresenter protos");
    let mut config = prost_build::Config::new();
    config
        .compile_fds(fds)
        .expect("generate ProPresenter protos");
}
