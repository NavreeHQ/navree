use navree_grpc::{Request, Response, Status, grpc_service};

mod pb {
    include!("../support/pb/navree.test.v1.rs");
}

use pb::ping_server::{Ping, PingServer};

#[navree_macros::injectable]
struct PingGrpc;

#[grpc_service(server = PingServer)]
impl Ping for PingGrpc {
    #[guard(std::sync::Arc::new(Always))]
    async fn ping(&self, request: Request<pb::PingRequest>) -> Result<Response<pb::PingReply>, Status> {
        let _ = request;
        Ok(Response::new(pb::PingReply::default()))
    }
}

#[derive(Debug)]
struct Always;

impl navree_http::Guard for Always {
    fn check<'a>(
        &'a self,
        _parts: &'a mut http::request::Parts,
    ) -> navree_kernel::BoxFuture<'a, Result<(), navree_http::AppError>> {
        Box::pin(async { Ok(()) })
    }
}

fn main() {}
