use navis_macros::{controller, injectable};

#[injectable]
struct A;

#[controller("/a", versoin = "1")]
impl A {
    #[get("/")]
    #[public]
    async fn index(&self) -> &'static str {
        "a"
    }
}

#[injectable]
struct B;

#[controller("/b", version = "v/1")]
impl B {
    #[get("/")]
    #[public]
    async fn index(&self) -> &'static str {
        "b"
    }
}

#[injectable]
struct C;

#[controller("/c/:id")]
impl C {
    #[get("/")]
    #[public]
    async fn index(&self) -> &'static str {
        "c"
    }
}

trait Named {
    fn name(&self) -> &'static str;
}

#[injectable]
struct D;

#[controller("/d")]
impl Named for D {
    fn name(&self) -> &'static str {
        "d"
    }
}

#[controller("/e")]
struct E;

fn main() {}
