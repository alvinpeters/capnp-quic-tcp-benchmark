pub mod proto_capnp {
    include!(concat!(env!("OUT_DIR"), "/proto_capnp.rs"));
}

pub mod key_cert_bytes {
    pub const KEY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/key.der"));
    pub const CERT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/cert.der"));
}