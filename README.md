# Cap'n Proto RPC on QUIC

So I decided to find out whether using QUIC
as a drop-in replacement to TCP for Cap'n Proto.

Turns out. It's not bad at all.

Copy-pasted the Cap'n Proto example.
Also added TLS using rustls on the TCP one
for a fair comparison.

## How did you test it?

Start the server.

The client connects, connects to server,
run for n times, then disconnect.

## So, is it better than TCP?

| Network   | Protocl | Repetitions | Connection time | Slowest run | Fastest run | Average run | Network round trip average | 
|-----------|---------|-------------|-----------------|-------------|-------------|-------------|----------------------------|
| Localhost | QUIC    | 1000        | 11.8ms          | 1.7ms       | 0.4ms       | 0.5ms       | 0.08ms                     |
| Localhost | TLS/TCP | 1000        | 3.1ms           | 8.6ms       | 0.4ms       | 0.6ms       | 0.1ms                      |
| Remote    | QUIC    | 1000        | 27.4ms          | 449.9ms     | 137.7ms     | 177.0ms     | 29.5ms                     |
| Remote    | TLS/TCP | 1000        | 110.0ms         | 788.7ms     | 174.6ms     | 211.1ms     | 35.7ms                     | 

So QUIC might be the way. Although, the difference is not that much.
Plus, I was basically cheating on UDP connections by using 0-RTT.

It's not the best test though. The RPC run does not change at all every transaction.

### My laptop (Macbook Pro M1 Pro) (client/localhost server)

- CPU: 8 power cores @ 3.2GHz and 2 efficiency cores @ 2.0GHz
- RAM: 16 GiB (probably lower)
- OS/Arch: macOS AARCH64

### My VPS (remote server)

- CPU: 1 KVM core @ 2.80GHz
- RAM: 989.38 MiB
- OS/Arch: FreeBSD x86_64
- Average ping: 28.7ms

## To do's:
- [x] Copy-paste from quinn-rs and capnproto-rust examples,
- [x] Make them somehow work together,
- [x] Clean up the code,
- [x] Add benchmarks (I tried),
- [ ] Turn into a fully functional plugin
  like [h3-quinn](https://github.com/hyperium/h3/tree/master/h3-quinn)
  or even [tonic](https://github.com/hyperium/tonic).