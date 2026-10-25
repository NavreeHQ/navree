use keel_kernel::Lazy;
use keel_macros::injectable;

struct Mailer;

#[injectable]
struct Service {
    #[inject(name = "bulk")]
    mailer: Lazy<Mailer>,
}

fn main() {}
