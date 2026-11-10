//! The OpenAPI schema of `GET /api/audit` responses (a hand-written
//! `navree_http::SchemaFn`, so navree-audit needs no schema derive).

use serde_json::{Value, json};

/// `("AuditPage", schema)`; the `AuditEntry` item schema travels in
/// `x-navree-components` for navree-api to hoist into `components.schemas`.
pub(crate) fn audit_page_schema() -> (String, Value) {
    let nullable_string = json!({"type": ["string", "null"]});
    let nullable_object = json!({"type": ["object", "null"], "additionalProperties": true});
    let entry = json!({
        "type": "object",
        "description": "One audit trail entry.",
        "required": [
            "id", "kind", "action", "actor_id", "impersonator_id", "subject_type", "subject_id",
            "old_values", "new_values", "request_id", "ip", "user_agent", "metadata", "created_at"
        ],
        "properties": {
            "id": {"type": "string", "format": "uuid"},
            "kind": {"type": "string", "enum": ["data", "auth", "access", "security", "custom"]},
            "action": {"type": "string", "examples": ["deal.updated"]},
            "actor_id": {"type": ["string", "null"], "format": "uuid"},
            "impersonator_id": {"type": ["string", "null"], "format": "uuid"},
            "subject_type": nullable_string,
            "subject_id": nullable_string,
            "old_values": nullable_object,
            "new_values": nullable_object,
            "request_id": nullable_string,
            "ip": nullable_string,
            "user_agent": nullable_string,
            "metadata": {"type": "object", "additionalProperties": true},
            "created_at": {"type": "string", "format": "date-time"},
        },
    });
    let count = json!({"type": "integer", "format": "int64", "minimum": 0});
    let page = json!({
        "type": "object",
        "description": "A page of audit entries, newest first.",
        "required": ["data", "meta"],
        "properties": {
            "data": {"type": "array", "items": {"$ref": "#/components/schemas/AuditEntry"}},
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
        "x-navree-components": {"AuditEntry": entry},
    });
    ("AuditPage".to_owned(), page)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::AuditRecord;

    #[test]
    fn page_schema_references_the_entry_schema() {
        let (name, schema) = audit_page_schema();
        assert_eq!(name, "AuditPage");
        assert_eq!(
            schema["properties"]["data"]["items"],
            json!({"$ref": "#/components/schemas/AuditEntry"})
        );
        assert_eq!(
            schema["properties"]["meta"]["required"],
            json!(["current_page", "per_page", "total", "last_page"])
        );
        assert!(schema["x-navree-components"]["AuditEntry"].is_object());
    }

    #[test]
    fn entry_schema_lists_every_serialized_field() {
        let record = AuditRecord {
            id: Uuid::now_v7(),
            kind: "data".into(),
            action: "deal.created".into(),
            actor_id: None,
            impersonator_id: None,
            subject_type: None,
            subject_id: None,
            old_values: None,
            new_values: None,
            request_id: None,
            ip: None,
            user_agent: None,
            metadata: json!({}),
            created_at: Utc::now(),
        };
        let serialized = serde_json::to_value(record).unwrap();
        let mut fields: Vec<&String> = serialized.as_object().unwrap().keys().collect();
        fields.sort();

        let (_, schema) = audit_page_schema();
        let entry = &schema["x-navree-components"]["AuditEntry"];
        let mut properties: Vec<&String> =
            entry["properties"].as_object().unwrap().keys().collect();
        properties.sort();
        assert_eq!(properties, fields);
        let mut required: Vec<&str> = entry["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        required.sort();
        assert_eq!(required, fields);
    }
}
