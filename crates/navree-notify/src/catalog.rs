//! The registered notification types and their categories:
//! [`NotificationKind`], [`NotificationCatalog`] and
//! [`ModuleBuilderNotifyExt`].

use std::collections::{BTreeMap, BTreeSet};

use navree_kernel::ModuleBuilder;
use serde::Serialize;

use crate::{Digest, Notification};

/// What the app knows about one notification type without an instance:
/// its type, category, digest and urgency (the constants of
/// [`Notification`]). Registered with
/// [`NotifyModule::for_feature`](crate::NotifyModule::for_feature) or
/// [`ModuleBuilderNotifyExt::notification`], so the preference routes can
/// list the categories people choose for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct NotificationKind {
    /// [`Notification::TYPE`].
    #[serde(rename = "type")]
    pub notification_type: &'static str,
    /// [`Notification::CATEGORY`].
    pub category: &'static str,
    /// [`Notification::DIGEST`].
    pub digest: Digest,
    /// [`Notification::URGENT`].
    pub urgent: bool,
}

impl NotificationKind {
    /// The kind of `N`.
    pub fn of<N: Notification>() -> NotificationKind {
        NotificationKind {
            notification_type: N::TYPE,
            category: N::CATEGORY,
            digest: N::DIGEST,
            urgent: N::URGENT,
        }
    }
}

/// A category as the preference routes list it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NotificationCategory {
    /// The category name.
    pub category: String,
    /// The notification types in it.
    pub types: Vec<String>,
    /// The digest its types declare (`off` when they differ).
    pub digest: Digest,
    /// Whether it cannot be turned off (`notify.preferences.required`).
    pub required: bool,
}

/// The registered notification types, by type and by category. Provided by
/// [`NotifyModule`](crate::NotifyModule) from every module's
/// registrations: inject `Arc<NotificationCatalog>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationCatalog {
    kinds: Vec<NotificationKind>,
}

impl NotificationCatalog {
    /// A catalog of `kinds`, sorted by type; a type registered twice must
    /// be the same both times.
    pub fn new(kinds: impl IntoIterator<Item = NotificationKind>) -> Result<Self, String> {
        let mut by_type: BTreeMap<&'static str, NotificationKind> = BTreeMap::new();
        for kind in kinds {
            if let Some(existing) = by_type.get(kind.notification_type) {
                if *existing != kind {
                    return Err(format!(
                        "notification type `{}` is registered twice, as {existing:?} and as {kind:?}",
                        kind.notification_type
                    ));
                }
                continue;
            }
            crate::preferences::check_category(kind.category)
                .map_err(|error| format!("notification type `{}`: {error}", kind.notification_type))?;
            by_type.insert(kind.notification_type, kind);
        }
        Ok(Self {
            kinds: by_type.into_values().collect(),
        })
    }

    /// Every registered type, sorted by type.
    pub fn kinds(&self) -> &[NotificationKind] {
        &self.kinds
    }

    /// The kind of `notification_type`, if registered.
    pub fn kind(&self, notification_type: &str) -> Option<&NotificationKind> {
        self.kinds
            .iter()
            .find(|kind| kind.notification_type == notification_type)
    }

    /// Whether no type is registered.
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// The category names, sorted.
    pub fn category_names(&self) -> BTreeSet<String> {
        self.kinds
            .iter()
            .map(|kind| kind.category.to_owned())
            .collect()
    }

    /// The categories, sorted by name, with their types; `required` lists
    /// the categories that cannot be turned off.
    pub fn categories(&self, required: &[String]) -> Vec<NotificationCategory> {
        let mut categories: BTreeMap<&str, NotificationCategory> = BTreeMap::new();
        for kind in &self.kinds {
            let entry = categories
                .entry(kind.category)
                .or_insert_with(|| NotificationCategory {
                    category: kind.category.to_owned(),
                    types: Vec::new(),
                    digest: kind.digest,
                    required: required.iter().any(|r| r == kind.category),
                });
            if entry.digest != kind.digest {
                entry.digest = Digest::Off;
            }
            entry.types.push(kind.notification_type.to_owned());
        }
        categories.into_values().collect()
    }
}

/// A registered notification type (a module contribution).
#[derive(Clone, Copy, Debug)]
pub(crate) struct NotificationRegistration(pub(crate) NotificationKind);

/// Notification registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderNotifyExt {
    /// Registers notification type `N` (what
    /// [`NotifyModule::for_feature::<N>()`](crate::NotifyModule::for_feature)
    /// does as an import), so its category is listed and accepted by the
    /// preference routes.
    fn notification<N: Notification>(&mut self) -> &mut Self;
}

impl ModuleBuilderNotifyExt for ModuleBuilder {
    fn notification<N: Notification>(&mut self) -> &mut Self {
        self.contribute(NotificationRegistration(NotificationKind::of::<N>()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(notification_type: &'static str, category: &'static str, digest: Digest) -> NotificationKind {
        NotificationKind {
            notification_type,
            category,
            digest,
            urgent: false,
        }
    }

    #[test]
    fn categories_group_their_types() {
        let catalog = NotificationCatalog::new([
            kind("deal_won", "deals", Digest::Daily),
            kind("deal_lost", "deals", Digest::Daily),
            kind("sign_in", "security", Digest::Off),
            kind("deal_won", "deals", Digest::Daily),
        ])
        .unwrap();
        assert_eq!(catalog.kinds().len(), 3);
        assert_eq!(catalog.kind("sign_in").unwrap().category, "security");
        assert!(catalog.kind("nope").is_none());
        assert_eq!(
            catalog.category_names(),
            BTreeSet::from(["deals".to_owned(), "security".to_owned()])
        );
        let categories = catalog.categories(&["security".to_owned()]);
        assert_eq!(
            categories,
            [
                NotificationCategory {
                    category: "deals".to_owned(),
                    types: vec!["deal_lost".to_owned(), "deal_won".to_owned()],
                    digest: Digest::Daily,
                    required: false,
                },
                NotificationCategory {
                    category: "security".to_owned(),
                    types: vec!["sign_in".to_owned()],
                    digest: Digest::Off,
                    required: true,
                },
            ]
        );
    }

    #[test]
    fn types_that_disagree_or_bad_categories_are_refused() {
        let error = NotificationCatalog::new([
            kind("deal_won", "deals", Digest::Daily),
            kind("deal_won", "sales", Digest::Daily),
        ])
        .unwrap_err();
        assert!(error.contains("registered twice"), "{error}");
        let error = NotificationCatalog::new([kind("x", "bad name", Digest::Off)]).unwrap_err();
        assert!(error.contains("category name"), "{error}");
        // Mixed digests in one category: off.
        let catalog = NotificationCatalog::new([
            kind("a", "c", Digest::Daily),
            kind("b", "c", Digest::Weekly),
        ])
        .unwrap();
        assert_eq!(catalog.categories(&[])[0].digest, Digest::Off);
        assert!(NotificationCatalog::default().is_empty());
    }
}
