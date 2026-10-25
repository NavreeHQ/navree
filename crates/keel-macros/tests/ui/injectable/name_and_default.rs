use keel_macros::injectable;

#[injectable]
struct Service {
    #[inject(name = "x", default)]
    retries: u32,
}

fn main() {}
