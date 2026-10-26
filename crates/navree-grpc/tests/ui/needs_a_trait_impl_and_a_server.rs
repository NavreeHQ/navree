use navree_grpc::grpc_service;

struct PingGrpc;

#[grpc_service(server = PingServer)]
impl PingGrpc {}

trait Ping {}

#[grpc_service(transport = Http2)]
impl Ping for PingGrpc {}

fn main() {}
