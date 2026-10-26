use navree_grpc::{Request, Response, Status, Streaming, grpc_service};

mod pb {
    pub mod ping {
        include!("../support/pb/navree.test.v1.rs");
    }

    pub mod deals {
        include!("../support/pb/crm.deals.v1.rs");
    }
}

use pb::deals::deals_server::{Deals, DealsServer};
use pb::ping::ping_server::{Ping, PingServer};

// A public method in an authenticated impl block.
#[navree_macros::injectable]
struct PublicInAuthenticated;

#[grpc_service(server = PingServer)]
#[authenticated]
impl Ping for PublicInAuthenticated {
    #[public]
    async fn ping(&self, _request: Request<pb::ping::PingRequest>) -> Result<Response<pb::ping::PingReply>, Status> {
        Err(Status::unimplemented("ping"))
    }
}

// #[can] on the impl block and on a method.
#[navree_macros::injectable]
struct TwoAbilities;

#[grpc_service(server = PingServer)]
#[can("read", "Ping")]
impl Ping for TwoAbilities {
    #[can("update", "Ping")]
    async fn ping(&self, _request: Request<pb::ping::PingRequest>) -> Result<Response<pb::ping::PingReply>, Status> {
        Err(Status::unimplemented("ping"))
    }
}

// `from = "id"` on a method that receives a stream.
#[navree_macros::injectable]
struct FromOnAStream;

type Events = tokio_stream::Empty<Result<pb::deals::DealEvent, Status>>;

#[grpc_service(server = DealsServer)]
#[authenticated]
impl Deals for FromOnAStream {
    async fn get_deal(
        &self,
        _request: Request<pb::deals::GetDealRequest>,
    ) -> Result<Response<pb::deals::Deal>, Status> {
        Err(Status::unimplemented("get_deal"))
    }

    async fn list_deals(
        &self,
        _request: Request<pb::deals::ListDealsRequest>,
    ) -> Result<Response<pb::deals::ListDealsResponse>, Status> {
        Err(Status::unimplemented("list_deals"))
    }

    async fn move_deal(
        &self,
        _request: Request<pb::deals::MoveDealRequest>,
    ) -> Result<Response<pb::deals::Deal>, Status> {
        Err(Status::unimplemented("move_deal"))
    }

    type WatchDealsStream = Events;

    async fn watch_deals(
        &self,
        _request: Request<pb::deals::WatchDealsRequest>,
    ) -> Result<Response<Events>, Status> {
        Err(Status::unimplemented("watch_deals"))
    }

    #[can("create", "Deal", from = "id")]
    async fn import_deals(
        &self,
        _request: Request<Streaming<pb::deals::Deal>>,
    ) -> Result<Response<pb::deals::ImportDealsResponse>, Status> {
        Err(Status::unimplemented("import_deals"))
    }

    type EchoStream = Events;

    async fn echo(
        &self,
        _request: Request<Streaming<pb::deals::DealEvent>>,
    ) -> Result<Response<Events>, Status> {
        Err(Status::unimplemented("echo"))
    }
}

fn main() {}
