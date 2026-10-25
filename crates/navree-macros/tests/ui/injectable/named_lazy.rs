use navree_kernel::Lazy;
use navree_macros::injectable;

struct Mailer;

#[injectable]
struct Service {
    #[inject(name = "bulk")]
    mailer: Lazy<Mailer>,
}

fn main() {}
