fn main() -> Result<(), Box<dyn std::error::Error>> {
    // tonic_build::compile_protos("proto/observer.proto")?;
    // Ok(())

    tonic_build::configure()
        .build_server(true)
        .out_dir("src/myproto")
        .compile(
            &["proto/observer.proto"],
            &["proto"], // specify the root location to search proto dependencies
        )
        .unwrap();
    Ok(())
}
