pub mod addressbook_capnp {
    include!(concat!(env!("OUT_DIR"), "/addressbook_capnp.rs"));
}
pub mod calculator_capnp {
    include!(concat!(env!("OUT_DIR"), "/calculator_capnp.rs"));
}

pub mod key_cert_bytes {
    pub const KEY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/key.der"));
    pub const CERT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/cert.der"));
}