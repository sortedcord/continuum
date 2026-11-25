fn main() -> std::io::Result<()> {
    let protos = [
        "proto/continuum/v1/session.proto",
        "proto/continuum/v1/pairing.proto",
        "proto/continuum/v1/storage.proto",
        "proto/continuum/features/page/v1/page.proto",
        "proto/continuum/features/trust/v1/trust.proto",
    ];
    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }
    prost_build::compile_protos(&protos, &["proto"])
}
