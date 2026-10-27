use navree_macros::notification;

trait Notification {
    const TYPE: &'static str;
}

#[notification(category = "deals")]
struct NotAnImpl;

#[notification(label = "deals")]
impl Notification for UnknownKey {
    const TYPE: &'static str = "unknown_key";
}

#[notification(digest = "monthly")]
impl Notification for BadDigest {
    const TYPE: &'static str = "bad_digest";
}

#[notification(category = "big deals")]
impl Notification for BadCategory {
    const TYPE: &'static str = "bad_category";
}

#[notification(type = "twice")]
impl Notification for TypeTwice {
    const TYPE: &'static str = "type_twice";
}

#[notification(digest = "daily", urgent)]
impl Notification for UrgentDigest {
    const TYPE: &'static str = "urgent_digest";
}

struct UnknownKey;
struct BadDigest;
struct BadCategory;
struct TypeTwice;
struct UrgentDigest;

fn main() {}
