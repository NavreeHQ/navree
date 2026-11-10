//! The rules of the customers module: which emails are taken, what the home
//! page counts, and telling open pages when a customer changed. Every read and
//! write is for one workspace, through `WorkspaceRepo`.

use navree::orm::OrmError;
use navree::prelude::*;
use navree::realtime::Live;
use navree::ui::form::FieldErrors;
use uuid::Uuid;

use super::entity::{Customer, Status};
use super::form::CustomerForm;
use crate::ui::live;

/// What the home page counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Figures {
    /// All customers.
    pub customers: u64,
    /// Customers whose status is Active.
    pub active: u64,
    /// Customers on trial.
    pub trials: u64,
    /// What the active customers pay each month, in paise.
    pub monthly_paise: i64,
}

/// Customers, read and written in the caller's unit of work.
#[injectable]
pub struct CustomerService {
    hub: Arc<Live>,
}

impl CustomerService {
    /// The customer of `workspace` with `id`, if there is one.
    pub async fn find(
        &self,
        uow: &Uow,
        workspace: Uuid,
        id: Uuid,
    ) -> Result<Option<Record<Customer>>, OrmError> {
        WorkspaceRepo::<Customer>::find(uow, workspace, id).await
    }

    /// What is wrong with `form` that its own rules cannot see: no fee, or an
    /// email another customer of the workspace has. `except` is the customer
    /// being edited.
    pub async fn problems(
        &self,
        uow: &Uow,
        workspace: Uuid,
        form: &CustomerForm,
        except: Option<Uuid>,
    ) -> Result<FieldErrors, OrmError> {
        let mut errors = FieldErrors::new();
        if form.monthly.0 <= 0 {
            errors.insert(
                "monthly".to_owned(),
                vec!["Enter the monthly fee in rupees.".to_owned()],
            );
        }
        let email = form.email.trim().to_lowercase();
        let mut taken = WorkspaceRepo::<Customer>::query(workspace).where_(Customer::EMAIL.eq(email));
        if let Some(id) = except {
            taken = taken.where_(Customer::ID.ne(id));
        }
        if taken.exists(uow).await? {
            errors.insert(
                "email".to_owned(),
                vec!["A customer with this email already exists.".to_owned()],
            );
        }
        Ok(errors)
    }

    /// Adds the customer `form` describes to `workspace`, and tells open pages
    /// once it is committed.
    pub async fn create(
        &self,
        uow: &Uow,
        workspace: Uuid,
        form: CustomerForm,
    ) -> Result<Record<Customer>, OrmError> {
        let customer =
            WorkspaceRepo::<Customer>::insert(uow, workspace, form.into_customer()).await?;
        self.changed(uow, workspace);
        Ok(customer)
    }

    /// Saves what `form` says onto `customer` of `workspace`, and tells open
    /// pages once it is committed.
    pub async fn update(
        &self,
        uow: &Uow,
        workspace: Uuid,
        mut customer: Record<Customer>,
        form: CustomerForm,
    ) -> Result<(), OrmError> {
        form.apply(customer.model_mut());
        WorkspaceRepo::<Customer>::update(uow, workspace, &mut customer).await?;
        self.changed(uow, workspace);
        Ok(())
    }

    /// The most recent customers of `workspace`, newest first.
    pub async fn newest(
        &self,
        uow: &Uow,
        workspace: Uuid,
        limit: u64,
    ) -> Result<Vec<Customer>, OrmError> {
        let found = WorkspaceRepo::<Customer>::query(workspace)
            .order_by_desc("since")
            .limit(limit)
            .get(uow)
            .await?;
        Ok(found
            .into_vec()
            .into_iter()
            .map(Record::into_inner)
            .collect())
    }

    /// The counts the home page shows, for `workspace`.
    pub async fn figures(&self, uow: &Uow, workspace: Uuid) -> Result<Figures, OrmError> {
        let all = || WorkspaceRepo::<Customer>::query(workspace);
        let active = all().where_(Customer::STATUS.eq(Status::Active.key()));
        let trials = all().where_(Customer::STATUS.eq(Status::Trial.key()));
        Ok(Figures {
            customers: all().count(uow).await?,
            active: active.clone().count(uow).await?,
            trials: trials.count(uow).await?,
            monthly_paise: active.sum(uow, Customer::MONTHLY_PAISE).await?,
        })
    }

    /// Tells every open page of the workspace to fetch again, after the unit of
    /// work commits: a page that asked sooner could read the data as it was.
    fn changed(&self, uow: &Uow, workspace: Uuid) {
        let hub = self.hub.clone();
        uow.after_commit(move || {
            Box::pin(async move {
                // No listener is not a problem: the next page that opens asks anyway.
                if let Err(error) = hub.nudge([live::topic(workspace)]).await {
                    tracing::warn!(%error, "open pages could not be told a customer changed");
                }
            })
        });
    }
}
