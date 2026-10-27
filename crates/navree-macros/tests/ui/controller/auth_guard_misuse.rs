use navree_macros::{controller, injectable};

#[injectable]
struct AccountController;

#[controller("/account")]
#[authenticated]
impl AccountController {
    #[delete("/")]
    #[recent_auth(hours = 1)]
    async fn destroy(&self) -> &'static str {
        "deleted"
    }

    #[post("/email")]
    #[recent_auth(minutes = 0)]
    async fn email(&self) -> &'static str {
        "email"
    }

    #[post("/password")]
    #[recent_auth]
    async fn password(&self) -> &'static str {
        "password"
    }

    #[post("/export")]
    #[not_while_impersonating(strict)]
    async fn export(&self) -> &'static str {
        "export"
    }

    #[post("/sessions")]
    #[not_while_impersonating]
    #[not_while_impersonating]
    async fn sessions(&self) -> &'static str {
        "sessions"
    }

    #[get("/open")]
    #[not_while_impersonating]
    async fn open(&self) -> &'static str {
        "open"
    }
}

#[injectable]
struct GuardOnlyController;

#[controller("/guarded")]
impl GuardOnlyController {
    #[get("/")]
    #[recent_auth(minutes = 10)]
    async fn index(&self) -> &'static str {
        "a guard is not an access declaration"
    }
}

fn main() {}
