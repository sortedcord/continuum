fn main() -> std::io::Result<()> {
    let protos = ["proto/continuum/v1/session.proto"];
    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }
    prost_build::compile_protos(&protos, &["proto"])
}
