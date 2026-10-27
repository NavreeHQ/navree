//! The OpenAPI schemas of the `/api/notifications` responses (hand-written
//! `navree_http::SchemaFn`s, so navree-notify needs no schema derive).

use serde_json::{Value, json};

/// `("NotificationPage", schema)`; the `Notification` item schema travels in
/// `x-navree-components` for navree-api to hoist into `components.schemas`.
pub(crate) fn notification_page_schema() -> (String, Value) {
    let notification = json!({
        "type": "object",
        "description": "A notification stored by the database channel.",
        "required": ["id", "type", "data", "read_at", "created_at"],
        "properties": {
            "id": {"type": "string", "format": "uuid"},
            "type": {"type": "string", "examples": ["deal_assigned"]},
            "data": {"type": "object", "additionalProperties": true},
            "read_at": {"type": ["string", "null"], "format": "date-time"},
            "created_at": {"type": "string", "format": "date-time"},
        },
    });
    let count = json!({"type": "integer", "format": "int64", "minimum": 0});
    let page = json!({
        "type": "object",
        "description": "A page of notifications, newest first.",
        "required": ["data", "meta"],
        "properties": {
            "data": {"type": "array", "items": {"$ref": "#/components/schemas/Notification"}},
            "meta": {
                "type": "object",
                "required": ["current_page", "per_page", "total", "last_page"],
                "properties": {
                    "current_page": count,
                    "per_page": count,
                    "total": count,
                    "last_page": count,
                },
            },
        },
        "x-navree-components": {"Notification": notification},
    });
    ("NotificationPage".to_owned(), page)
}

/// `("UnreadNotificationCount", {count})`.
pub(crate) fn unread_count_schema() -> (String, Value) {
    let schema = json!({
        "type": "object",
        "description": "How many notifications are unread.",
        "required": ["count"],
        "properties": {
            "count": {"type": "integer", "format": "int64", "minimum": 0},
        },
    });
    ("UnreadNotificationCount".to_owned(), schema)
}

/// `("PushKey", {public_key})`.
pub(crate) fn push_key_schema() -> (String, Value) {
    let schema = json!({
        "type": "object",
        "description": "The application server key (VAPID public key, base64url) for `pushManager.subscribe`.",
        "required": ["public_key"],
        "properties": {
            "public_key": {"type": "string"},
        },
    });
    ("PushKey".to_owned(), schema)
}

/// `("NewPushSubscription", ..)`: what `subscription.toJSON()` gives.
pub(crate) fn new_push_subscription_schema() -> (String, Value) {
    let schema = json!({
        "type": "object",
        "description": "A browser push subscription, as `PushSubscription.toJSON()` gives it.",
        "required": ["endpoint", "keys"],
        "properties": {
            "endpoint": {"type": "string", "format": "uri"},
            "expirationTime": {"type": ["integer", "null"]},
            "keys": {
                "type": "object",
                "required": ["p256dh", "auth"],
                "properties": {
                    "p256dh": {"type": "string"},
                    "auth": {"type": "string"},
                },
            },
        },
    });
    ("NewPushSubscription".to_owned(), schema)
}

/// `("PushSubscription", ..)`: a saved subscription, without its keys.
pub(crate) fn push_subscription_schema() -> (String, Value) {
    let schema = json!({
        "type": "object",
        "description": "A saved push subscription.",
        "required": ["id", "notifiable_type", "notifiable_id", "endpoint", "user_agent", "created_at", "last_used_at"],
        "properties": {
            "id": {"type": "string", "format": "uuid"},
            "notifiable_type": {"type": "string"},
            "notifiable_id": {"type": "string"},
            "endpoint": {"type": "string", "format": "uri"},
            "user_agent": {"type": ["string", "null"]},
            "created_at": {"type": "string", "format": "date-time"},
            "last_used_at": {"type": ["string", "null"], "format": "date-time"},
        },
    });
    ("PushSubscription".to_owned(), schema)
}

/// `("PushUnsubscribe", {endpoint})`.
pub(crate) fn unsubscribe_schema() -> (String, Value) {
    let schema = json!({
        "type": "object",
        "required": ["endpoint"],
        "properties": {
            "endpoint": {"type": "string", "format": "uri"},
        },
    });
    ("PushUnsubscribe".to_owned(), schema)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::DatabaseNotification;

    #[test]
    fn the_item_schema_lists_every_serialized_field() {
        let (name, page) = notification_page_schema();
        assert_eq!(name, "NotificationPage");
        let item = &page["x-navree-components"]["Notification"];
        let required: Vec<&str> = item["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field.as_str().unwrap())
            .collect();
        let serialized = serde_json::to_value(DatabaseNotification {
            id: Uuid::now_v7(),
            notification_type: "deal_won".to_owned(),
            data: json!({}),
            read_at: None,
            created_at: Utc::now(),
        })
        .unwrap();
        let mut fields: Vec<&str> = serialized
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        fields.sort_unstable();
        let mut required = required;
        required.sort_unstable();
        assert_eq!(fields, required);
    }
}
