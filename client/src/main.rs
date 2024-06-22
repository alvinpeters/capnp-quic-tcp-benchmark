// Copyright (c) 2013-2016 Sandstorm Development Group, Inc. and contributors
// Licensed under the MIT License:
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

use std::io::{Error as IoError, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;


use protos::calculator_capnp::calculator;
use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};

use quinn::crypto::rustls::QuicClientConfig;
use quinn::Endpoint;
use rustls::ClientConfig;
use rustls::pki_types::{CertificateDer, ServerName};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use capnp_rpc::rpc_twoparty_capnp::Side;
use tokio::io::{split};
use tokio::net::TcpStream;
use tokio::task::spawn_local;
use tokio_rustls::TlsConnector;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use protos::key_cert_bytes::CERT;

#[derive(Clone, Copy)]
pub struct PowerFunction;

impl calculator::function::Server for PowerFunction {
    fn call(
        &mut self,
        params: calculator::function::CallParams,
        mut results: calculator::function::CallResults,
    ) -> Promise<(), capnp::Error> {
        let params = pry!(pry!(params.get()).get_params());
        if params.len() != 2 {
            Promise::err(::capnp::Error::failed(
                "Wrong number of parameters".to_string(),
            ))
        } else {
            results.get().set_value(params.get(0).powf(params.get(1)));
            Promise::ok(())
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = ::std::env::args().collect();
    if args.len() != 4 {
        println!("usage: {} HOST:PORT tcp|quic 5", args[0]);
        return Ok(());
    }
    let is_quic = if args[2].as_str() == "quic" {
        true
    } else if args[2].as_str() == "tcp" {
        false
    } else {
        println!("usage: {} HOST:PORT tcp|quic 5", args[0]);
        return Ok(());
    };
    tokio::task::LocalSet::new().run_until(try_main(args, is_quic)).await
}

async fn try_main(args: Vec<String>, is_quic: bool) -> Result<(), Box<dyn std::error::Error>> {
    use std::net::ToSocketAddrs;

    let host = match args[1].rsplit_once(":") {
        Some((host_str, _port_str)) => {
            match ServerName::try_from(host_str.to_string()) {
                Ok(s) => s,
                Err(_e) => return Err(Box::new(error_msg("Failed to parse host!"))),
            }
        },
        None => return Err(Box::new(error_msg("None found as host string"))),
    };
    let addr = args[1]
        .to_socket_addrs()?
        .next()
        .expect("could not parse address");
    // Set libcrypto provider
    #[cfg(feature = "ring")]
    rustls::crypto::ring::default_provider().install_default().unwrap();
    #[cfg(not(feature = "ring"))]
    rustls::crypto::aws_lc_rs::default_provider().install_default().unwrap();
    // Set up TLS config
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(CertificateDer::from(CERT)))?;
    let client_crypto = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let conn_timer = Instant::now();
    let mut rpc_system = if is_quic {
        connect_quic_client(client_crypto, addr, host).await?
    } else {
        connect_tcp_client(client_crypto, addr, host).await?
    };
    let calculator: calculator::Client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);
    // Get disconnector. Real essential for maintaining connection and notifying peer.
    let disconnector = rpc_system.get_disconnector();
    tokio::task::spawn_local(rpc_system);
    let conn_elapsed = conn_timer.elapsed();

    let repetitions = args[3].parse()?;
    let mut rpc_transact_timer_sum = Duration::default();
    let mut rpc_transact_fastest = Duration::new(u64::MAX, 0);
    let mut rpc_transact_slowest = Duration::default();
    let mut round_trip_total_sum = Duration::default();
    let mut round_trip_count_sum = 0;
    println!("Doing {} transactions", repetitions);
    for repeat in 1..=repetitions {
        let rpc_transact_timer = Instant::now();
        let (round_trip_total, round_trip_count) = run_calculator_rpc(&calculator).await?;
        let elapsed = rpc_transact_timer.elapsed();
        println!("RPC calculator run {} complete. Took {}μs ({}ms).", repeat, elapsed.as_micros(), elapsed.as_millis());
        round_trip_total_sum += round_trip_total;
        round_trip_count_sum += round_trip_count;
        rpc_transact_timer_sum += elapsed;
        rpc_transact_fastest = rpc_transact_fastest.min(elapsed);
        rpc_transact_slowest = rpc_transact_slowest.max(elapsed);
        if rpc_transact_fastest > elapsed {
            rpc_transact_fastest = elapsed;
        } else if rpc_transact_slowest < elapsed {
            rpc_transact_slowest = elapsed;
        }
    }

    // Disconnect client.
    // This will also notify the TLS peer.
    // https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof
    disconnector.await?;
    let round_trip_ave = round_trip_total_sum / round_trip_count_sum;
    let rpc_transact_timer_ave = rpc_transact_timer_sum / repetitions;
    let conn_name = if is_quic { "QUIC on UDP" } else { "TLS on TCP" };

    println!("Took {}μs ({}ms) to establish {} RPC connection.", conn_elapsed.as_micros(), conn_elapsed.as_millis(), conn_name);
    println!("Took {}μs ({}ms) on average to complete {} calculator runs.", rpc_transact_timer_ave.as_micros(), rpc_transact_timer_ave.as_millis(), repetitions);
    println!("Fastest transaction took {}μs ({}ms). Slowest transaction took {}μs ({}ms).", rpc_transact_fastest.as_micros(), rpc_transact_fastest.as_millis(), rpc_transact_slowest.as_micros(), rpc_transact_slowest.as_millis());
    println!("{} network round trips averaging {}μs ({}ms). (This should be the same as your ping)", round_trip_count_sum, round_trip_ave.as_micros(), round_trip_ave.as_millis());
    Ok(())
}

async fn connect_quic_client(
    client_crypto: ClientConfig,
    addr: SocketAddr,
    host: ServerName<'static>,
) -> Result<RpcSystem<Side>, Box<dyn std::error::Error>> {
    let client_config =
        quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(client_crypto)?));

    // Set up client and connection. Requires matching the IP family
    let bind_socket = SocketAddr::new(
        match addr.ip() {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        },
        0
    );
    let mut endpoint = Endpoint::client(bind_socket)?;
    endpoint.set_default_client_config(client_config);

    // Actually connect

    // 0-RTT BABY!
    // Don't use this in real life please
    let conn = match endpoint.connect(addr, &host.to_str())?.into_0rtt() {
        Ok((c, accepted)) => {
            // Spawn another task to let us know that we just fully connected
            println!("QUIC 0-RTT BABY!");
            spawn_local(async move {
                accepted.await;
                println!("Finally connected for real");
            });
            c
        }
        Err(c) => c.await?
    };
    let (writer, reader) = conn
        .open_bi()
        .await?;

    let network = Box::new(twoparty::VatNetwork::new(
        reader.compat(),
        writer.compat_write(),
        Side::Client,
        Default::default(),
    ));
    Ok(RpcSystem::new(network, None))
}

async fn connect_tcp_client(
    client_crypto: ClientConfig,
    addr: SocketAddr,
    host: ServerName<'static>,
) -> Result<RpcSystem<Side>, Box<dyn std::error::Error>> {
    let tls_connector = TlsConnector::from(Arc::new(client_crypto));
    let tcp_stream = TcpStream::connect(&addr).await?;
    tcp_stream.set_nodelay(true)?;
    let stream = tls_connector.connect(host, tcp_stream).await?;
    let (reader, writer) = split(stream);
    let network = Box::new(twoparty::VatNetwork::new(
        reader.compat(),
        writer.compat_write(),
        rpc_twoparty_capnp::Side::Client,
        Default::default(),
    ));
    Ok(RpcSystem::new(network, None))
}

async fn run_calculator_rpc(calculator: &calculator::Client) -> Result<(Duration, u32), Box<dyn std::error::Error>> {
    let mut round_trip_time_sum = Duration::default();
    let mut round_trip_timed_count = 0;
    {
        // Make a request that just evaluates the literal value 123.
        //
        // What's interesting here is that evaluate() returns a "Value", which is
        // another interface and therefore points back to an object living on the
        // server.  We then have to call read() on that object to read it.
        // However, even though we are making two RPC's, this block executes in
        // *one* network round trip because of promise pipelining:  we do not wait
        // for the first call to complete before we send the second call to the
        // server.

        //println!("Evaluating a literal...");
        let round_trip = Instant::now();

        let mut eval_request = calculator.evaluate_request();
        eval_request.get().init_expression().set_literal(123.0);
        let value = eval_request.send().pipeline.get_value();
        let read_request = value.read_request();
        let response = read_request.send().promise.await?;
        assert_eq!(response.get()?.get_value(), 123.0);

        round_trip_time_sum += round_trip.elapsed();
        round_trip_timed_count += 1;
        //println!("PASS");
    }

    {
        // Make a request to evaluate 123 + 45 - 67.
        //
        // The Calculator interface requires that we first call getOperator() to
        // get the addition and subtraction functions, then call evaluate() to use
        // them.  But, once again, we can get both functions, call evaluate(), and
        // then read() the result -- four RPCs -- in the time of *one* network
        // round trip, because of promise pipelining.

        //println!("Using add and subtract... ");

        let round_trip = Instant::now();

        let add = {
            // Get the "add" function from the server.
            let mut request = calculator.get_operator_request();
            request.get().set_op(calculator::Operator::Add);
            request.send().pipeline.get_func()
        };

        let subtract = {
            // Get the "subtract" function from the server.
            let mut request = calculator.get_operator_request();
            request.get().set_op(calculator::Operator::Subtract);
            request.send().pipeline.get_func()
        };

        // Build the request to evaluate 123 + 45 - 67.
        let mut request = calculator.evaluate_request();

        {
            let mut subtract_call = request.get().init_expression().init_call();
            subtract_call.set_function(subtract);
            let mut subtract_params = subtract_call.init_params(2);
            subtract_params.reborrow().get(1).set_literal(67.0);

            let mut add_call = subtract_params.get(0).init_call();
            add_call.set_function(add);
            let mut add_params = add_call.init_params(2);
            add_params.reborrow().get(0).set_literal(123.0);
            add_params.get(1).set_literal(45.0);
        }

        // Send the evaluate() request, read() the result, and wait for read() to
        // finish.
        let eval_promise = request.send();
        let read_promise = eval_promise.pipeline.get_value().read_request().send();

        let response = read_promise.promise.await?;
        assert_eq!(response.get()?.get_value(), 101.0);

        round_trip_time_sum += round_trip.elapsed();
        round_trip_timed_count += 1;
        //println!("PASS");
    }

    {
        // Make a request to evaluate 4 * 6, then use the result in two more
        // requests that add 3 and 5.
        //
        // Since evaluate() returns its result wrapped in a `Value`, we can pass
        // that `Value` back to the server in subsequent requests before the first
        // `evaluate()` has actually returned.  Thus, this example again does only
        // one network round trip.

        //println!("Pipelining eval() calls... ");
        let round_trip = Instant::now();

        let add = {
            // Get the "add" function from the server.
            let mut request = calculator.get_operator_request();
            request.get().set_op(calculator::Operator::Add);
            request.send().pipeline.get_func()
        };

        let multiply = {
            // Get the "multiply" function from the server.
            let mut request = calculator.get_operator_request();
            request.get().set_op(calculator::Operator::Multiply);
            request.send().pipeline.get_func()
        };

        // Build the request to evaluate 4 * 6
        let mut request = calculator.evaluate_request();

        {
            let mut multiply_call = request.get().init_expression().init_call();
            multiply_call.set_function(multiply);
            let mut multiply_params = multiply_call.init_params(2);
            multiply_params.reborrow().get(0).set_literal(4.0);
            multiply_params.reborrow().get(1).set_literal(6.0);
        }

        let multiply_result = request.send().pipeline.get_value();

        // Use the result in two calls that add 3 and add 5.
        let mut add3_request = calculator.evaluate_request();
        {
            let mut add3_call = add3_request.get().init_expression().init_call();
            add3_call.set_function(add.clone());
            let mut add3_params = add3_call.init_params(2);
            add3_params
                .reborrow()
                .get(0)
                .set_previous_result(multiply_result.clone());
            add3_params.reborrow().get(1).set_literal(3.0);
        }

        let add3_promise = add3_request
            .send()
            .pipeline
            .get_value()
            .read_request()
            .send();

        let mut add5_request = calculator.evaluate_request();
        {
            let mut add5_call = add5_request.get().init_expression().init_call();
            add5_call.set_function(add);
            let mut add5_params = add5_call.init_params(2);
            add5_params
                .reborrow()
                .get(0)
                .set_previous_result(multiply_result);
            add5_params.get(1).set_literal(5.0);
        }

        let add5_promise = add5_request
            .send()
            .pipeline
            .get_value()
            .read_request()
            .send();

        // Now wait for the results.
        assert!(add3_promise.promise.await?.get()?.get_value() == 27.0);
        assert!(add5_promise.promise.await?.get()?.get_value() == 29.0);

        round_trip_time_sum += round_trip.elapsed();
        round_trip_timed_count += 1;
        //println!("PASS")
    }

    {
        // Our calculator interface supports defining functions.  Here we use it
        // to define two functions and then make calls to them as follows:
        //
        //   f(x, y) = x * 100 + y
        //   g(x) = f(x, x + 1) * 2;
        //   f(12, 34)
        //   g(21)
        //
        // Once again, the whole thing takes only one network round trip.

        //println!("Defining functions... ");
        let round_trip = Instant::now();
        let add = {
            let mut request = calculator.get_operator_request();
            request.get().set_op(calculator::Operator::Add);
            request.send().pipeline.get_func()
        };

        let multiply = {
            let mut request = calculator.get_operator_request();
            request.get().set_op(calculator::Operator::Multiply);
            request.send().pipeline.get_func()
        };

        let f = {
            let mut request = calculator.def_function_request();
            {
                let mut def_function_params = request.get();
                def_function_params.set_param_count(2);
                {
                    let mut add_call = def_function_params.init_body().init_call();
                    add_call.set_function(add.clone());
                    let mut add_params = add_call.init_params(2);
                    add_params.reborrow().get(1).set_parameter(1);

                    let mut multiply_call = add_params.get(0).init_call();
                    multiply_call.set_function(multiply.clone());
                    let mut multiply_params = multiply_call.init_params(2);
                    multiply_params.reborrow().get(0).set_parameter(0);
                    multiply_params.get(1).set_literal(100.0);
                }
            }
            request.send().pipeline.get_func()
        };

        let g = {
            let mut request = calculator.def_function_request();
            {
                let mut def_function_params = request.get();
                def_function_params.set_param_count(1);
                {
                    let mut multiply_call = def_function_params.init_body().init_call();
                    multiply_call.set_function(multiply);
                    let mut multiply_params = multiply_call.init_params(2);
                    multiply_params.reborrow().get(1).set_literal(2.0);

                    let mut f_call = multiply_params.get(0).init_call();
                    f_call.set_function(f.clone());
                    let mut f_params = f_call.init_params(2);
                    f_params.reborrow().get(0).set_parameter(0);

                    let mut add_call = f_params.get(1).init_call();
                    add_call.set_function(add);
                    let mut add_params = add_call.init_params(2);
                    add_params.reborrow().get(0).set_parameter(0);
                    add_params.get(1).set_literal(1.0);
                }
            }
            request.send().pipeline.get_func()
        };

        let mut f_eval_request = calculator.evaluate_request();
        {
            let mut f_call = f_eval_request.get().init_expression().init_call();
            f_call.set_function(f);
            let mut f_params = f_call.init_params(2);
            f_params.reborrow().get(0).set_literal(12.0);
            f_params.get(1).set_literal(34.0);
        }
        let f_eval_promise = f_eval_request
            .send()
            .pipeline
            .get_value()
            .read_request()
            .send();

        let mut g_eval_request = calculator.evaluate_request();
        {
            let mut g_call = g_eval_request.get().init_expression().init_call();
            g_call.set_function(g);
            g_call.init_params(1).get(0).set_literal(21.0);
        }
        let g_eval_promise = g_eval_request
            .send()
            .pipeline
            .get_value()
            .read_request()
            .send();

        assert!(f_eval_promise.promise.await?.get()?.get_value() == 1234.0);
        assert!(g_eval_promise.promise.await?.get()?.get_value() == 4244.0);

        // They're not joking, literally only takes one round trip
        round_trip_time_sum += round_trip.elapsed();
        round_trip_timed_count += 1;

        //println!("PASS")
    }

    {
        // Make a request that will call back to a function defined locally.
        //
        // Specifically, we will compute 2^(4 + 5).  However, exponent is not
        // defined by the Calculator server.  So, we'll implement the Function
        // interface locally and pass it to the server for it to use when
        // evaluating the expression.
        //
        // This example requires two network round trips to complete, because the
        // server calls back to the client once before finishing.  In this
        // particular case, this could potentially be optimized by using a tail
        // call on the server side -- see CallContext::tailCall().  However, to
        // keep the example simpler, we haven't implemented this optimization in
        // the sample server.

        //println!("Using a callback... ");
        let round_trip = Instant::now();
        let add = {
            let mut request = calculator.get_operator_request();
            request.get().set_op(calculator::Operator::Add);
            request.send().pipeline.get_func()
        };

        let mut request = calculator.evaluate_request();
        {
            let mut pow_call = request.get().init_expression().init_call();
            pow_call.set_function(capnp_rpc::new_client(PowerFunction));
            let mut pow_params = pow_call.init_params(2);
            pow_params.reborrow().get(0).set_literal(2.0);

            let mut add_call = pow_params.get(1).init_call();
            add_call.set_function(add);
            let mut add_params = add_call.init_params(2);
            add_params.reborrow().get(0).set_literal(4.0);
            add_params.get(1).set_literal(5.0);
        }

        let response_promise = request.send().pipeline.get_value().read_request().send();
        assert!(response_promise.promise.await?.get()?.get_value() == 512.0);

        // This one showed double time when I printed.
        round_trip_time_sum += round_trip.elapsed();
        round_trip_timed_count += 2;

        //println!("PASS");
    }
    Ok((round_trip_time_sum, round_trip_timed_count))
}

fn error_msg(err: &str) -> Box<IoError> {
    Box::new(IoError::new(ErrorKind::Other, err.to_string()))
}