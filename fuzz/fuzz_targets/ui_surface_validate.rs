//! The check a model's arguments for a page component pass (navree-ui's generative UI
//! catalogue).
//!
//! Invariants: whatever the bytes, the check never panics and never takes longer than
//! its bounds allow; what it takes checks again to itself, holds only the properties
//! the component names, and holds no text with a control character or a mark that
//! turns text around.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_ui::genui::{
    AppError, AppLink, BoxFuture, Catalog, Day, Int, Key, List, Surface, SurfaceCx, SurfaceView,
    UserText,
};
use serde_json::Value;

struct Reader;

impl SurfaceCx for Reader {
    fn can(&self, _action: &str, _subject: &str) -> bool {
        true
    }
}

navree_ui::choices! {
    /// A period.
    pub enum Period {
        /// Today.
        Today = "today",
        /// This week.
        ThisWeek = "this_week",
    }
}

navree_ui::props! {
    /// A row.
    pub struct Row {
        /// The order.
        pub order: Key,
        /// A caption.
        pub caption: Option<UserText<40>>,
    }
}

navree_ui::props! {
    /// Everything a component can take.
    pub struct Everything {
        /// When.
        pub period: Period,
        /// How many.
        pub count: Int<1, 50>,
        /// Since.
        pub since: Option<Day>,
        /// The rows.
        pub rows: List<Row, 8>,
        /// A link.
        pub page: Option<AppLink>,
        /// A flag.
        pub refunds: bool,
    }
}

struct Shown;

impl Surface<Reader> for Shown {
    type Props = Everything;
    const NAME: &'static str = "everything";

    fn title(&self, _props: &Everything) -> String {
        String::new()
    }

    fn render<'a>(
        &'a self,
        _props: &'a Everything,
        _cx: &'a Reader,
    ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
        Box::pin(async { Ok(SurfaceView::new("")) })
    }
}

fn clean(value: &Value) {
    match value {
        Value::String(text) => assert!(
            !text.chars().any(|c| (c.is_control() && c != '\n' && c != '\t')
                || ('\u{202A}'..='\u{202E}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c)),
            "{text:?}"
        ),
        Value::Array(items) => items.iter().for_each(clean),
        Value::Object(map) => map.values().for_each(clean),
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<Value>(data) else {
        return;
    };
    let catalog = Catalog::<Reader>::new().surface(Shown).link("/orders/{key}");
    if let Ok(checked) = catalog.check("show_everything", &value) {
        let names = ["period", "count", "since", "rows", "page", "refunds"];
        for key in checked.props.as_object().expect("an object").keys() {
            assert!(names.contains(&key.as_str()), "{key}");
        }
        clean(&checked.props);
        let again = catalog
            .check("show_everything", &checked.props)
            .expect("what passed passes again");
        assert_eq!(again.props, checked.props);
    }
});
