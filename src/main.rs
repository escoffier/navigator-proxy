use async_trait::async_trait;
use log::info;
use navigator_proxy::inpod::{self, netns::InpodNetns};
use nix::unistd::Pid;
use pingora::listeners::TcpSocketOptions;
use prometheus::register_int_counter;
use structopt::StructOpt;

use pingora_core::server::configuration::Opt;
use pingora_core::server::Server;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_core::Result;
use pingora_http::ResponseHeader;
use pingora_proxy::{ProxyHttp, Session};

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

        let authority = session.req_header().uri.authority();

        println!("header {session.req_header():?}");
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
        let client_addr = session.downstream_session.client_addr().unwrap();
        println!("Hello, world!");

        println!("down stream {client_addr:?}");

        let remote = session.req_header().uri.authority().unwrap();
        // println!("remote addr {remote:?}");
        println!("remote addr {remote}");

        let addr = if session.req_header().uri.path().starts_with("/family") {
            ("44.193.104.184", 443)
        } else {
            ("44.193.104.184", 443)
        };

        info!("connecting to {addr:?}");
        println!("connecting to {addr:?}");

        let peer = Box::new(HttpPeer::new(addr, true, "httpbin.org".to_string()));
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
fn main() {
    env_logger::init();

    let opt = Opt::parse_args();

    // read command line arguments
    let podns = inpod::new_inpod_netns(Pid::from_raw(3784226)).unwrap();
    let _ = podns.run(|| {
        let mut my_server = Server::new(Some(opt)).unwrap();
        my_server.bootstrap();

        let mut my_proxy = pingora_proxy::http_proxy_service(
            &my_server.configuration,
            MyGateway {
                req_metric: register_int_counter!("reg_counter", "Number of requests").unwrap(),
            },
        );
        let mut options = TcpSocketOptions::default();
        options.tp_proxy = Some(true);
        options.mark = Some(1337);
        // my_proxy.add_tcp("0.0.0.0:6191");
        my_proxy.add_tcp_with_settings(
            "0.0.0.0:6191",
            options,
        );

        my_server.add_service(my_proxy);

        let mut prometheus_service_http =
            pingora_core::services::listening::Service::prometheus_http_service();
        prometheus_service_http.add_tcp("127.0.0.1:6192");
        my_server.add_service(prometheus_service_http);

        my_server.run_forever();
    });

    println!("Hello, world!");
}
