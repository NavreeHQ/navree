//! `GET {api_prefix}/audit`: the audit trail, filtered and paginated.

use std::sync::Arc;

use http::Method;
use navree_db::Page;
use navree_http::list::List;
use navree_http::{ApiMeta, AppError, Controller, RouteDef, list_spec};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};

use crate::schema::audit_page_schema;
use crate::{AuditList, AuditLog, AuditRecord};

/// The permission `GET /api/audit` requires.
pub const AUDIT_READ_PERMISSION: &str = "audit.read";

/// Serves the audit trail (when `AuditOptions::routes` is on).
pub(crate) struct AuditController {
    log: Arc<AuditLog>,
}

impl Injectable for AuditController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<AuditLog>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(AuditController {
                log: r.get::<AuditLog>().await?,
            })
        })
    }
}

impl Controller for AuditController {
    fn prefix(&self) -> &'static str {
        "/audit"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.log.options().routes {
            return Vec::new();
        }
        let this = self.clone();
        vec![
            RouteDef::on(Method::GET, "/", move |list: List<AuditList>| async move {
                this.index(list).await
            })
            .permissions(&[AUDIT_READ_PERMISSION])
            .name("audit.index")
            .api(ApiMeta {
                summary: Some("List audit entries, newest first".to_owned()),
                tags: vec!["Audit".to_owned()],
                responses: vec![400, 403],
                response: Some(audit_page_schema),
                query: Some(list_spec::<AuditList>),
                ..ApiMeta::default()
            }),
        ]
    }
}

impl AuditController {
    async fn index(&self, list: List<AuditList>) -> Result<Page<AuditRecord>, AppError> {
        Ok(self.log.list_query(&list).await?)
    }
}
