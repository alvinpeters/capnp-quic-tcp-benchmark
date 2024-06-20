# Cap'n Proto RPC on QUIC

Obviously written on Rust.
Quite a dirty attempt but it is somehow working LMAO

Copy-pasted the Cap'n Proto example but added
TLS using rustls for a fair comparison.

## So, is it better than TCP?

Initial tests shows that QUIC has slower connection
times compared to TLS over TCP.
need to compare streaming speeds later.

## To do's:
- [x] Copy-paste from quinn-rs and capnproto-rust examples,
- [x] Make them somehow work together,
- [ ] Clean up the code,
- [ ] Add benchmarks
- [ ] Turn into a fully functional plugin
  like [h3-quinn](https://github.com/hyperium/h3/tree/master/h3-quinn)