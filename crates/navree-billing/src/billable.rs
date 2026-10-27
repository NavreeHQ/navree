//! [`Billable`]: what can be charged (a workspace, a user, a type of your
//! own), and [`BillableRef`], a billable by type and id.

use axum::extract::FromRequestParts;
use http::request::Parts;
use navree_auth::{CurrentUser, User};
use navree_http::AppError;
use navree_kernel::BoxFuture;
use navree_workspace::{CurrentWorkspace, Workspace};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A type you charge: it gets a customer at the provider, subscriptions,
/// trials and usage.
///
/// navree-billing implements it for the workspace model
/// (`navree_workspace::Workspace`, type `workspace`), the auth user
/// (`navree_auth::User`, type `user`) and their request extractors
/// `CurrentWorkspace` and `CurrentUser`. Register the type you bill with
/// [`BillingModule::for_feature`](crate::BillingModule::for_feature).
///
/// ```
/// use navree_billing::Billable;
/// use uuid::Uuid;
///
/// struct Organization {
///     id: Uuid,
///     legal_name: String,
///     billing_email: Option<String>,
/// }
///
/// impl Billable for Organization {
///     /// Stored with customers and subscriptions; keep it stable.
///     const BILLABLE_TYPE: &'static str = "organization";
///
///     fn billable_id(&self) -> Uuid {
///         self.id
///     }
///
///     fn billing_name(&self) -> String {
///         self.legal_name.clone()
///     }
///
///     fn billing_email(&self) -> Option<String> {
///         self.billing_email.clone()
///     }
/// }
/// ```
pub trait Billable {
    /// The type name stored with customers and subscriptions
    /// (`workspace`, `user`); keep it stable.
    const BILLABLE_TYPE: &'static str;

    /// The id of this billable.
    fn billable_id(&self) -> Uuid;

    /// The customer name at the provider.
    fn billing_name(&self) -> String;

    /// The customer email at the provider; `None` lets the checkout ask for
    /// one (or take the one passed to `customer_email`).
    fn billing_email(&self) -> Option<String>;

    /// The billable of an HTTP request, for the `/api/billing` routes and
    /// the [`Subscribed`](crate::Subscribed) extractor. The built-in
    /// billables resolve it from `CurrentWorkspace` and `CurrentUser`; the
    /// default answers 500, so implement it to use a billable of your own
    /// on routes.
    fn from_request(parts: &mut Parts) -> BoxFuture<'_, Result<BillableRef, AppError>>
    where
        Self: Sized,
    {
        let _ = parts;
        let message = format!(
            "billable type `{}` does not implement Billable::from_request",
            Self::BILLABLE_TYPE
        );
        Box::pin(std::future::ready(Err(AppError::internal(message))))
    }
}

/// A billable by type and id, with its name and email: what
/// [`Billing`](crate::Billing) stores and the provider sees.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BillableRef {
    /// `Billable::BILLABLE_TYPE`.
    pub billable_type: String,
    /// `Billable::billable_id`.
    pub billable_id: Uuid,
    /// `Billable::billing_name`.
    pub name: String,
    /// `Billable::billing_email`.
    pub email: Option<String>,
}

impl BillableRef {
    /// The reference to `billable`.
    pub fn of<B: Billable>(billable: &B) -> BillableRef {
        BillableRef {
            billable_type: B::BILLABLE_TYPE.to_owned(),
            billable_id: billable.billable_id(),
            name: billable.billing_name(),
            email: billable.billing_email(),
        }
    }

    /// A reference by type and id alone (name empty, no email).
    pub fn new(billable_type: impl Into<String>, billable_id: Uuid) -> BillableRef {
        BillableRef {
            billable_type: billable_type.into(),
            billable_id,
            name: String::new(),
            email: None,
        }
    }
}

/// Anything [`Billing`](crate::Billing) can act on: every [`Billable`],
/// and a [`BillableRef`].
pub trait AsBillable {
    /// The reference to the billable.
    fn billable_ref(&self) -> BillableRef;
}

impl<B: Billable> AsBillable for B {
    fn billable_ref(&self) -> BillableRef {
        BillableRef::of(self)
    }
}

impl AsBillable for BillableRef {
    fn billable_ref(&self) -> BillableRef {
        self.clone()
    }
}

/// The billable type of workspaces.
pub const WORKSPACE_BILLABLE: &str = "workspace";
/// The billable type of users.
pub const USER_BILLABLE: &str = "user";

impl Billable for Workspace {
    const BILLABLE_TYPE: &'static str = WORKSPACE_BILLABLE;

    fn billable_id(&self) -> Uuid {
        self.id
    }

    fn billing_name(&self) -> String {
        self.name.clone()
    }

    fn billing_email(&self) -> Option<String> {
        None
    }

    fn from_request(parts: &mut Parts) -> BoxFuture<'_, Result<BillableRef, AppError>> {
        Box::pin(async move {
            let workspace = CurrentWorkspace::from_request_parts(parts, &()).await?;
            Ok(BillableRef::of(workspace.workspace()))
        })
    }
}

impl Billable for CurrentWorkspace {
    const BILLABLE_TYPE: &'static str = WORKSPACE_BILLABLE;

    fn billable_id(&self) -> Uuid {
        self.id()
    }

    fn billing_name(&self) -> String {
        self.workspace().name.clone()
    }

    fn billing_email(&self) -> Option<String> {
        None
    }

    fn from_request(parts: &mut Parts) -> BoxFuture<'_, Result<BillableRef, AppError>> {
        Workspace::from_request(parts)
    }
}

impl Billable for User {
    const BILLABLE_TYPE: &'static str = USER_BILLABLE;

    fn billable_id(&self) -> Uuid {
        self.id
    }

    fn billing_name(&self) -> String {
        self.name.clone()
    }

    fn billing_email(&self) -> Option<String> {
        Some(self.email.clone())
    }

    fn from_request(parts: &mut Parts) -> BoxFuture<'_, Result<BillableRef, AppError>> {
        Box::pin(async move {
            let CurrentUser(user) = CurrentUser::from_request_parts(parts, &()).await?;
            Ok(BillableRef::of(&user))
        })
    }
}

impl Billable for CurrentUser {
    const BILLABLE_TYPE: &'static str = USER_BILLABLE;

    fn billable_id(&self) -> Uuid {
        self.0.id
    }

    fn billing_name(&self) -> String {
        self.0.name.clone()
    }

    fn billing_email(&self) -> Option<String> {
        Some(self.0.email.clone())
    }

    fn from_request(parts: &mut Parts) -> BoxFuture<'_, Result<BillableRef, AppError>> {
        User::from_request(parts)
    }
}

/// A billable type registered with
/// [`BillingModule::for_feature`](crate::BillingModule::for_feature).
#[derive(Clone, Copy)]
pub(crate) struct BillableRegistration {
    pub(crate) billable_type: &'static str,
    pub(crate) resolve: for<'a> fn(&'a mut Parts) -> BoxFuture<'a, Result<BillableRef, AppError>>,
}

impl BillableRegistration {
    pub(crate) fn of<B: Billable>() -> BillableRegistration {
        BillableRegistration {
            billable_type: B::BILLABLE_TYPE,
            resolve: B::from_request,
        }
    }
}

impl std::fmt::Debug for BillableRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BillableRegistration")
            .field("billable_type", &self.billable_type)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn workspaces_and_users_are_billable() {
        let now = Utc::now();
        let workspace = Workspace {
            id: Uuid::now_v7(),
            name: "Acme".into(),
            slug: "acme".into(),
            owner_id: Uuid::now_v7(),
            kind: "live".into(),
            created_at: now,
            updated_at: now,
        };
        let reference = BillableRef::of(&workspace);
        assert_eq!(reference.billable_type, "workspace");
        assert_eq!(reference.billable_id, workspace.id);
        assert_eq!(reference.name, "Acme");
        assert_eq!(reference.email, None);
        assert_eq!(workspace.billable_ref(), reference);
        assert_eq!(reference.billable_ref(), reference);

        let user = User {
            id: Uuid::now_v7(),
            name: "Ada".into(),
            email: "ada@example.com".into(),
            password_hash: String::new(),
            email_verified_at: None,
            two_factor_secret: None,
            two_factor_recovery_codes: None,
            two_factor_confirmed_at: None,
            created_at: now,
            updated_at: now,
        };
        let reference = user.billable_ref();
        assert_eq!(reference.billable_type, "user");
        assert_eq!(reference.email.as_deref(), Some("ada@example.com"));
        let current = CurrentUser(user.clone());
        assert_eq!(current.billable_ref(), reference);
        assert_eq!(BillableRef::new("user", user.id).billable_id, user.id);
    }
}
