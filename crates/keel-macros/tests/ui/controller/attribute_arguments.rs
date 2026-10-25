use keel_macros::{controller, injectable};

#[injectable]
struct DealController;

#[controller("/deals")]
impl DealController {
    #[get("/a")]
    #[public(always)]
    async fn a(&self) -> &'static str {
        "a"
    }

    #[get("/b")]
    #[roles()]
    async fn b(&self) -> &'static str {
        "b"
    }

    #[get("/c")]
    #[permissions("")]
    async fn c(&self) -> &'static str {
        "c"
    }

    #[get("/d")]
    #[authenticated]
    #[throttle(limit = 0, per = "1m")]
    async fn d(&self) -> &'static str {
        "d"
    }

    #[get("/e")]
    #[authenticated]
    #[throttle(limit = 10)]
    async fn e(&self) -> &'static str {
        "e"
    }

    #[get("/f")]
    #[authenticated]
    #[audit("deal.viewed")]
    async fn f(&self) -> &'static str {
        "f"
    }

    #[get("/g")]
    #[authenticated]
    #[name]
    async fn g(&self) -> &'static str {
        "g"
    }

    #[get("/h")]
    #[authenticated]
    #[api(sumary = "typo")]
    async fn h(&self) -> &'static str {
        "h"
    }

    #[get("/i")]
    #[authenticated]
    #[api(responses(404, 999))]
    async fn i(&self) -> &'static str {
        "i"
    }

    #[get(42)]
    #[authenticated]
    async fn j(&self) -> &'static str {
        "j"
    }
}

fn main() {}
