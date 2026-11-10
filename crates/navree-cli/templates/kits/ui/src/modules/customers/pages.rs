//! The customer pages: what each route loads and which view draws it. Every
//! page is for the workspace the signed-in person is in.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /customers` | the table; search, filters, sort and pages are in the address |
//! | `GET /customers/new`, `POST /customers/new` | the form that adds a customer, and its post |
//! | `GET /customers/{id}` | one customer |
//! | `GET /customers/{id}/edit`, `POST /customers/{id}/edit` | the form that edits one, and its post |
//!
//! The table and the figures stay current through the app's live hub
//! (`GET /broadcasting/stream`).

use http::Method;
use http::request::Parts;
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::i18n::ui::FromLocale;
use navree::prelude::*;
use navree::ui::form::Rejected;
use navree::ui::*;
use uuid::Uuid;

use super::entity::{Customer, Plan, Status};
use super::form::CustomerForm;
use super::service::CustomerService;
use super::table::{CustomerRow, customers_table};
use crate::ui::format::{day, phone_text, rupees};
use crate::ui::shell::Desk;

/// The page that holds a customer form, with the way back.
fn form_page(desk: &Desk, title: String, form: FormView<CustomerForm>) -> AppPage {
    desk.page(title)
        .crumb("Home", "/")
        .crumb("Customers", "/customers")
        .body(Card::new().body(form))
}

/// The form again, with what was typed and what is wrong. With the page's
/// script on, the errors appear under the fields instead.
fn again(
    rejected: Rejected<CustomerForm>,
    desk: &Desk,
    title: &str,
    submit: &'static str,
) -> Response {
    let token = desk.csrf();
    rejected.respond(|form| {
        form_page(
            desk,
            title.to_owned(),
            form.submit(submit)
                .cancel("Cancel", "/customers")
                .csrf(token),
        )
    })
}

/// Everything about one customer, in cards.
fn details(customer: &Customer) -> Stack {
    let status = Status::parse(&customer.status);
    let contact = FactCard::new("Contact", customer.name.clone())
        .icon(icons::USER)
        .detail(customer.email.clone())
        .detail(phone_text(&customer.phone))
        .detail(customer.city.clone());
    let plan = FactCard::new("Plan", Plan::parse(&customer.plan).label().to_owned())
        .icon(icons::INDIAN_RUPEE)
        .tone(Tone::Info)
        .detail(format!("{} a month", rupees(customer.monthly_paise)))
        .detail(if customer.autopay {
            "Pays by auto-pay".to_owned()
        } else {
            "Pays by invoice".to_owned()
        });
    let standing = FactCard::new("Status", status.label().to_owned())
        .icon(icons::CALENDAR)
        .tone(status.tone())
        .detail(format!("Customer since {}", day(customer.since)));
    let notes = customer
        .notes
        .as_ref()
        .map(|notes| Card::titled("Notes").body(Text::new(notes.clone())));
    Stack::gap(4).children((
        FactGrid::new().cols(3).cards([contact, plan, standing]),
        notes,
    ))
}

/// `GET /customers`, with the table's words in the reader's language.
async fn index(
    desk: Desk,
    uow: Uow,
    locale: Locale,
    query: TableQuery<CustomerRow>,
) -> Result<AppPage> {
    let labels = TableLabels::from_locale(&locale);
    let page = query
        .load_as(
            WorkspaceRepo::<Customer>::query(desk.workspace()),
            &uow,
            |record| CustomerRow::from(record.into_inner()),
        )
        .await?;
    Ok(desk
        .page("Customers")
        .crumb("Home", "/")
        .actions(
            Button::primary("New customer")
                .icon(icons::PLUS)
                .href("/customers/new"),
        )
        .body(customers_table(&query, &page, &labels)))
}

/// `GET /customers/new`.
async fn new_form(desk: Desk) -> AppPage {
    let today = (chrono::Utc::now() + chrono::TimeDelta::minutes(330)).date_naive();
    let form = FormView::<CustomerForm>::new()
        .of(&CustomerForm::blank(today))
        .post("/customers/new")
        .submit("Add customer")
        .cancel("Cancel", "/customers")
        .csrf(desk.csrf());
    form_page(&desk, "New customer".to_owned(), form)
}

/// `POST /customers/new`.
async fn store(
    parts: Parts,
    desk: Desk,
    uow: Uow,
    service: Scoped<CustomerService>,
    sent: Result<Submit<CustomerForm>, Rejected<CustomerForm>>,
) -> Result<Response> {
    let form = match sent {
        Ok(Submit(form)) => form,
        Err(rejected) => {
            return Ok(again(rejected, &desk, "New customer", "Add customer"));
        }
    };
    let errors = service
        .problems(&uow, desk.workspace(), &form, None)
        .await?;
    if !errors.is_empty() {
        let rejected = Rejected::after_decode(&form, errors, &parts);
        return Ok(again(rejected, &desk, "New customer", "Add customer"));
    }
    let customer = service.create(&uow, desk.workspace(), form).await?;
    Ok(Reply::redirect("/customers")
        .toast(ToastTone::Success, format!("{} was added.", customer.name))
        .into_response())
}

/// `GET /customers/{id}`.
async fn show(
    desk: Desk,
    uow: Uow,
    service: Scoped<CustomerService>,
    Path(id): Path<Uuid>,
) -> Result<AppPage> {
    let customer = service
        .find(&uow, desk.workspace(), id)
        .await?
        .ok_or_else(AppError::not_found)?;
    Ok(desk
        .page(customer.name.clone())
        .crumb("Home", "/")
        .crumb("Customers", "/customers")
        .actions(
            Button::outline("Edit")
                .icon(icons::PENCIL)
                .href(format!("/customers/{id}/edit")),
        )
        .body(details(&customer)))
}

/// `GET /customers/{id}/edit`.
async fn edit_form(
    desk: Desk,
    uow: Uow,
    service: Scoped<CustomerService>,
    Path(id): Path<Uuid>,
) -> Result<AppPage> {
    let customer = service
        .find(&uow, desk.workspace(), id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let form = FormView::<CustomerForm>::new()
        .of(&CustomerForm::of(&customer))
        .post(format!("/customers/{id}/edit"))
        .submit("Save changes")
        .cancel("Cancel", format!("/customers/{id}"))
        .csrf(desk.csrf());
    Ok(form_page(&desk, format!("Edit {}", customer.name), form))
}

/// `POST /customers/{id}/edit`.
async fn update(
    parts: Parts,
    desk: Desk,
    uow: Uow,
    service: Scoped<CustomerService>,
    Path(id): Path<Uuid>,
    sent: Result<Submit<CustomerForm>, Rejected<CustomerForm>>,
) -> Result<Response> {
    let form = match sent {
        Ok(Submit(form)) => form,
        Err(rejected) => {
            return Ok(again(rejected, &desk, "Edit customer", "Save changes"));
        }
    };
    let Some(customer) = service.find(&uow, desk.workspace(), id).await? else {
        return Err(AppError::not_found());
    };
    let errors = service
        .problems(&uow, desk.workspace(), &form, Some(id))
        .await?;
    if !errors.is_empty() {
        let rejected = Rejected::after_decode(&form, errors, &parts);
        return Ok(again(rejected, &desk, "Edit customer", "Save changes"));
    }
    let name = form.name.trim().to_owned();
    service
        .update(&uow, desk.workspace(), customer, form)
        .await?;
    Ok(Reply::redirect(format!("/customers/{id}"))
        .toast(ToastTone::Success, format!("{name} was saved."))
        .into_response())
}

/// The routes above. Every one is for signed-in users and is a page, so a guest
/// is sent to the sign-in page and back.
#[injectable]
pub struct CustomerPages;

impl Controller for CustomerPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/customers", index)
                .name("customers.index")
                .authenticated()
                .page(),
            RouteDef::on(Method::GET, "!/customers/new", new_form)
                .name("customers.new")
                .authenticated()
                .page(),
            RouteDef::on(Method::POST, "!/customers/new", store)
                .name("customers.store")
                .authenticated()
                .page(),
            RouteDef::on(Method::GET, "!/customers/{id}", show)
                .name("customers.show")
                .authenticated()
                .page(),
            RouteDef::on(Method::GET, "!/customers/{id}/edit", edit_form)
                .name("customers.edit")
                .authenticated()
                .page(),
            RouteDef::on(Method::POST, "!/customers/{id}/edit", update)
                .name("customers.update")
                .authenticated()
                .page(),
        ]
    }
}
