//! The rules of the customers module: which emails are taken, what the home
//! page counts, and telling open pages when a customer changed.

use navree::orm::OrmError;
use navree::prelude::*;
use navree::ui::form::FieldErrors;
use uuid::Uuid;

use super::entity::{Customer, Status};
use super::form::CustomerForm;
use crate::ui::live::Hub;

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
    hub: Arc<Hub>,
}

impl CustomerService {
    /// The customer with `id`, if there is one.
    pub async fn find(&self, uow: &Uow, id: Uuid) -> Result<Option<Record<Customer>>, OrmError> {
        Customer::find(uow, id).await
    }

    /// What is wrong with `form` that its own rules cannot see: no fee, or an
    /// email another customer has. `except` is the customer being edited.
    pub async fn problems(
        &self,
        uow: &Uow,
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
        let mut taken = Customer::query().where_(Customer::EMAIL.eq(email));
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

    /// Adds the customer `form` describes, and tells open pages once it is committed.
    pub async fn create(
        &self,
        uow: &Uow,
        form: CustomerForm,
    ) -> Result<Record<Customer>, OrmError> {
        let customer = Customer::create(uow, form.into_customer()).await?;
        self.changed(uow);
        Ok(customer)
    }

    /// Saves what `form` says onto `customer`, and tells open pages once it is committed.
    pub async fn update(
        &self,
        uow: &Uow,
        customer: &mut Record<Customer>,
        form: CustomerForm,
    ) -> Result<(), OrmError> {
        customer.update(uow, |model| form.apply(model)).await?;
        self.changed(uow);
        Ok(())
    }

    /// The most recent customers, newest first.
    pub async fn newest(&self, uow: &Uow, limit: u64) -> Result<Vec<Customer>, OrmError> {
        let found = Customer::query()
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

    /// The counts the home page shows.
    pub async fn figures(&self, uow: &Uow) -> Result<Figures, OrmError> {
        let active = Customer::query().where_(Customer::STATUS.eq(Status::Active.key()));
        let trials = Customer::query().where_(Customer::STATUS.eq(Status::Trial.key()));
        Ok(Figures {
            customers: Customer::query().count(uow).await?,
            active: active.clone().count(uow).await?,
            trials: trials.count(uow).await?,
            monthly_paise: active.sum(uow, Customer::MONTHLY_PAISE).await?,
        })
    }

    /// Tells every open page to fetch again, after the unit of work commits: a
    /// page that asked sooner could read the data as it was.
    fn changed(&self, uow: &Uow) {
        let hub = self.hub.clone();
        uow.after_commit(move || {
            Box::pin(async move {
                hub.nudge();
            })
        });
    }
}
