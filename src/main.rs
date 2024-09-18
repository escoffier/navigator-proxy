use async_trait::async_trait;
// use libc::sleep;
use log::info;
use navigator_proxy::inpod::{self, netns::InpodNetns};
use nix::unistd::Pid;
use pingora::{
    listeners::{Listeners, ServerAddress, TcpSocketOptions},
    upstreams::peer::Peer,
};
use pingora_core::server::configuration::Opt;
use pingora_core::server::Server;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_core::Result;
use pingora_http::ResponseHeader;
use pingora_proxy::{ProxyHttp, Session};
use prometheus::register_int_counter;
use std::{
    process::Command,
    thread,
    time::{self, Instant},
};
use structopt::StructOpt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};

fn check_login(req: &pingora_http::RequestHeader) -> bool {
    // implement you logic check logic here
    req.headers.get("Authorization").map(|v| v.as_bytes()) == Some(b"password")
}

pub struct MyGateway {
    req_metric: prometheus::IntCounter,
}

#[async_trait]
impl ProxyHttp for MyGateway {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}

    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let client_addr = session.downstream_session.client_addr().unwrap();
        println!("down stream {client_addr:?}");

        let uri = session.req_header().uri.to_string();

        println!("header {uri:?}");
        if session.req_header().uri.path().starts_with("/login")
            && !check_login(session.req_header())
        {
            let _ = session.respond_error(403).await;
            return Ok(true);
        }
        Ok(false)
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let client_addr: &pingora::protocols::l4::socket::SocketAddr =
            session.downstream_session.client_addr().unwrap();

        println!("down stream {client_addr:?}");

        let server_addr = session.downstream_session.server_addr();

        info!("server addr {server_addr:?}");

        let authority = session.req_header().uri.authority();
        // println!("remote addr {remote:?}");
        println!("remote addr {authority:?}");

        let addr = if session.req_header().uri.path().starts_with("/family") {
            ("44.193.104.184", 443)
        } else {
            ("44.193.104.184", 443)
        };

        info!("connecting to {addr:?}");
        println!("connecting to {addr:?}");

        let mut peer = Box::new(HttpPeer::new(addr, true, "httpbin.org".to_string()));
        let options = peer.get_mut_peer_options();
        options.map(|o| {
            o.mark = Some(1337);
            o
        });
        info!("peer options {peer:?}");
        Ok(peer)
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        // replace existing header if any
        upstream_response
            .insert_header("Server", "MyGateway")
            .unwrap();
        // because we don't support h3
        upstream_response.remove_header("alt-svc");

        Ok(())
    }

    async fn logging(
        &self,
        session: &mut Session,
        _e: Option<&pingora_core::Error>,
        ctx: &mut Self::CTX,
    ) {
        let response_code = session
            .response_written()
            .map_or(0, |resp| resp.status.as_u16());
        info!(
            "{} response code: {response_code}",
            self.request_summary(session, ctx)
        );

        self.req_metric.inc();
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let opt = Opt::parse_args();

    let mut my_server = Server::new(Some(opt)).unwrap();
    my_server.bootstrap();
    let vec = vec![1165, 3860141];
    // let vec = vec![1165];

    // read command line arguments
    for pid in vec {
        info!("pid {pid:?}");
        let podns = inpod::new_inpod_netns(Pid::from_raw(pid)).unwrap();
        info!("workload_netns_id: {:?}", podns.workload_netns_id());

        let _ = podns.run(|| {
            let now = Instant::now();
            let output = Command::new("sh").arg("-c").arg("ip a").output().unwrap();
            info!(
                "command complete in {:?}; code={}, stdout={}, stderr={}",
                now.elapsed(),
                output.status,
                std::str::from_utf8(&output.stdout).unwrap(),
                std::str::from_utf8(&output.stderr).unwrap()
            );

            let mut owned_string: String = "reg_counter_".to_owned();
            let pid_str = pid.to_string();
            owned_string.push_str(&pid_str);

            // let mut proxy: pingora_proxy::HttpProxy<_> = pingora_proxy::HttpProxy::new(inner, conf.clone());
            // proxy.handle_init_modules();
            // Service::new(name.to_string(), proxy)

            let mut my_proxy = pingora_proxy::http_proxy_service(
                &my_server.configuration,
                MyGateway {
                    req_metric: register_int_counter!(owned_string, "Number of request").unwrap(),
                },
            );
            let mut options = TcpSocketOptions::default();
            options.tp_proxy = Some(true);
            options.mark = Some(1337);

            // let l4 = ServerAddress::Tcp(String::from("0.0.0.0:6191"), Some(options));
            let mut listeners = Listeners::tcp(&String::from("0.0.0.0:15006"));
            let listeners = listeners.build(None);
            for mut listener in listeners {
                tokio::spawn(async move {
                    listener.listen().await.unwrap();
                    info!("create listener for :15006");
                    let stream = listener.accept().await.unwrap();
                    stream.handshake().await.unwrap();
                });
            }
            // let mut listener = ListenerEndpoint::new(ServerAddress::Tcp(addr.into(), None));

            // my_proxy.add_tcp("0.0.0.0:6191");
            my_proxy.add_tcp_with_settings("0.0.0.0:6191", options);

            // my_server.add_service(my_proxy);
            my_server.run_service1(my_proxy);
            let ten_millis = time::Duration::from_secs(2);
            thread::sleep(ten_millis);
        });
    }

    sleep(Duration::from_secs(200)).await;

    // let mut prometheus_service_http =
    //     pingora_core::services::listening::Service::prometheus_http_service();
    // prometheus_service_http.add_tcp("127.0.0.1:6192");
    // my_server.add_service(prometheus_service_http);

    // my_server.run_forever();
}
