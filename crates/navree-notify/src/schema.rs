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

/// The schema of a [`Preferences`](crate::Preferences) document.
fn preference_set() -> Value {
    let switches = json!({
        "type": "object",
        "description": "Channel switches (mail, database, slack, teams, sms, whatsapp, web_push, push).",
        "additionalProperties": {"type": "boolean"},
    });
    let digest = json!({"type": "string", "enum": ["off", "hourly", "daily", "weekly"]});
    json!({
        "type": "object",
        "description": "Notification preferences: every field is optional and inherited when left out.",
        "properties": {
            "channels": switches,
            "categories": {
                "type": "object",
                "additionalProperties": {
                    "type": "object",
                    "properties": {"channels": switches, "digest": digest},
                },
            },
            "digest": digest,
            "quiet_hours": {
                "type": "object",
                "required": ["start", "end"],
                "properties": {
                    "start": {"type": "string", "examples": ["22:00"]},
                    "end": {"type": "string", "examples": ["07:00"]},
                },
            },
        },
    })
}

/// `("NotificationPreferences", ..)`: an owner's choices, the workspace's
/// and what each category gets.
pub(crate) fn preferences_schema() -> (String, Value) {
    let set = preference_set();
    let schema = json!({
        "type": "object",
        "description": "Notification preferences with the workspace's defaults and the effective value of each category.",
        "required": ["data", "workspace", "categories", "channels", "time_zone"],
        "properties": {
            "data": set,
            "workspace": {"anyOf": [set, {"type": "null"}]},
            "categories": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["category", "types", "required", "digest", "channels"],
                    "properties": {
                        "category": {"type": "string"},
                        "types": {"type": "array", "items": {"type": "string"}},
                        "required": {"type": "boolean"},
                        "digest": {"type": "string", "enum": ["off", "hourly", "daily", "weekly"]},
                        "channels": {"type": "object", "additionalProperties": {"type": "boolean"}},
                    },
                },
            },
            "channels": {"type": "array", "items": {"type": "string"}},
            "time_zone": {"type": "string", "examples": ["Asia/Kolkata"]},
        },
    });
    ("NotificationPreferences".to_owned(), schema)
}

/// `("NotificationPreferencesPatch", ..)`: a JSON merge patch of the
/// choices (`null` removes one).
pub(crate) fn preferences_patch_schema() -> (String, Value) {
    let mut schema = preference_set();
    if let Some(fields) = schema.as_object_mut() {
        fields.insert(
            "description".to_owned(),
            json!("A JSON merge patch of the preferences: objects merge, null removes a choice."),
        );
    }
    ("NotificationPreferencesPatch".to_owned(), schema)
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

/// `NewPushDevice`: what a native app sends to register its device.
pub(crate) fn new_push_device_schema() -> (String, Value) {
    let schema = json!({
        "type": "object",
        "description": "A native app's push device: the token Apple or Firebase gave the app.",
        "required": ["platform", "token", "app_id"],
        "properties": {
            "platform": {"type": "string", "enum": ["ios", "android"]},
            "token": {"type": "string", "maxLength": 4096},
            "app_id": {"type": "string", "description": "The bundle id or package name."},
            "environment": {"type": "string", "enum": ["sandbox", "production"], "default": "production"},
        },
    });
    ("NewPushDevice".to_owned(), schema)
}

/// `PushDevice`: a saved device (its token is never returned).
pub(crate) fn push_device_schema() -> (String, Value) {
    let schema = json!({
        "type": "object",
        "description": "A saved push device.",
        "required": ["id", "notifiable_type", "notifiable_id", "platform", "app_id", "environment", "created_at", "last_seen_at", "last_used_at"],
        "properties": {
            "id": {"type": "string", "format": "uuid"},
            "notifiable_type": {"type": "string"},
            "notifiable_id": {"type": "string"},
            "platform": {"type": "string", "enum": ["ios", "android"]},
            "app_id": {"type": "string"},
            "environment": {"type": "string", "enum": ["sandbox", "production"]},
            "created_at": {"type": "string", "format": "date-time"},
            "last_seen_at": {"type": "string", "format": "date-time"},
            "last_used_at": {"type": ["string", "null"], "format": "date-time"},
        },
    });
    ("PushDevice".to_owned(), schema)
}

/// The body of `DELETE /notifications/push-devices`.
pub(crate) fn remove_device_schema() -> (String, Value) {
    let schema = json!({
        "type": "object",
        "required": ["token"],
        "properties": {
            "token": {"type": "string"},
        },
    });
    ("PushDeviceRemoval".to_owned(), schema)
}
