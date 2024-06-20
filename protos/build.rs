use std::fs::File;
use std::io::{Result, Write};
use std::path::Path;

fn main() -> Result<()> {
    let subject_names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "[::1]".to_string(),
    ];
    let key_path = Path::join(std::env::var("OUT_DIR").unwrap().as_ref(), "key.der");
    let cert_path = Path::join(std::env::var("OUT_DIR").unwrap().as_ref(), "cert.der");
    capnpc::CompilerCommand::new()
        .file("addressbook.capnp")
        .run()
        .expect("compiling schema");
    capnpc::CompilerCommand::new()
        .file("calculator.capnp")
        .run()
        .expect("compiling schema");
    let key_cert = rcgen::generate_simple_self_signed(subject_names)
        .expect("generating keys");
    File::create(key_path)?.write(key_cert.key_pair.serialized_der())?;
    File::create(cert_path)?.write(key_cert.cert.der())?;
    Ok(())
}