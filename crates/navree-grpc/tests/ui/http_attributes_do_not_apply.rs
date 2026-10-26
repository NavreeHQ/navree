use navree_grpc::{Request, Response, Status, grpc_service};

mod pb {
    include!("../support/pb/navree.test.v1.rs");
}

use pb::ping_server::{Ping, PingServer};

#[navree_macros::injectable]
struct PingGrpc;

#[grpc_service(server = PingServer)]
#[public]
impl Ping for PingGrpc {
    #[throttle(limit = 10, per = "1m")]
    async fn ping(&self, request: Request<pb::PingRequest>) -> Result<Response<pb::PingReply>, Status> {
        let _ = request;
        Ok(Response::new(pb::PingReply::default()))
    }
}

fn main() {}
