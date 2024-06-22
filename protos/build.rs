use std::fs::File;
use std::io::{Result, Write};
use std::path::Path;

fn main() -> Result<()> {
    // Use existing key/cert pair in the crate's directory
    let key_path = Path::join(std::env::var("OUT_DIR").unwrap().as_ref(), "key.der");
    let cert_path = Path::join(std::env::var("OUT_DIR").unwrap().as_ref(), "cert.der");
    if Path::new("key.der").exists() && Path::new("cert.der").exists() {
        // Copy so include_bytes! can use it.
        std::fs::copy("key.der", key_path)?;
        std::fs::copy("cert.der", cert_path)?;
    } else {
        // Generate TLS keys
        let subject_names = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "[::1]".to_string(),
        ];
        let key_cert = rcgen::generate_simple_self_signed(subject_names)
            .expect("generating keys");
        File::create(key_path)?.write(key_cert.key_pair.serialized_der())?;
        File::create(cert_path)?.write(key_cert.cert.der())?;
    }
    // Cap'n Proto
    capnpc::CompilerCommand::new()
        .file("addressbook.capnp")
        .run()
        .expect("compiling schema");
    capnpc::CompilerCommand::new()
        .file("calculator.capnp")
        .run()
        .expect("compiling schema");

    Ok(())
}