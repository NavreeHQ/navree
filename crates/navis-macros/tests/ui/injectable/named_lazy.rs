use navis_kernel::Lazy;
use navis_macros::injectable;

struct Mailer;

#[injectable]
struct Service {
    #[inject(name = "bulk")]
    mailer: Lazy<Mailer>,
}

fn main() {}
