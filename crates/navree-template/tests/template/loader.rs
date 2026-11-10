//! Templates from a loader, with reloading.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use navree_template::{Environment, ErrorKind, TemplateSource};
use serde_json::json;

#[test]
fn a_loader_serves_templates_and_reloads_stale_ones() {
    let files: Arc<Mutex<BTreeMap<String, (String, usize)>>> = Arc::default();
    let reads = Arc::new(AtomicUsize::new(0));
    files
        .lock()
        .expect("lock")
        .insert("a.txt".to_owned(), ("one".to_owned(), 1));

    let mut env = Environment::new();
    let (store, counter) = (files.clone(), reads.clone());
    env.set_loader(move |name| {
        counter.fetch_add(1, Ordering::SeqCst);
        let files = store.lock().expect("lock");
        Ok(files.get(name).map(|(source, version)| {
            let (store, name, version) = (store.clone(), name.to_owned(), *version);
            TemplateSource::new(source.clone()).with_uptodate_check(move || {
                store
                    .lock()
                    .expect("lock")
                    .get(&name)
                    .is_some_and(|(_, current)| *current == version)
            })
        }))
    });

    let render = |env: &Environment, name: &str| {
        env.get_template(name)
            .and_then(|template| template.render(json!({})))
    };
    assert_eq!(render(&env, "a.txt").expect("renders"), "one");
    assert_eq!(render(&env, "a.txt").expect("renders"), "one");
    assert_eq!(reads.load(Ordering::SeqCst), 1, "cached while current");

    files
        .lock()
        .expect("lock")
        .insert("a.txt".to_owned(), ("two".to_owned(), 2));
    assert_eq!(render(&env, "a.txt").expect("renders"), "two");
    assert_eq!(reads.load(Ordering::SeqCst), 2);

    files.lock().expect("lock").remove("a.txt");
    assert_eq!(
        render(&env, "a.txt").expect_err("removed").kind(),
        ErrorKind::TemplateNotFound
    );
    assert_eq!(
        render(&env, "b.txt").expect_err("missing").kind(),
        ErrorKind::TemplateNotFound
    );
}

#[test]
fn added_templates_win_over_the_loader() {
    let mut env = Environment::new();
    env.set_loader(|_| Ok(Some(TemplateSource::new("loaded"))));
    env.add_template("a.txt", "added").expect("compiles");
    let render = |name: &str| {
        env.get_template(name)
            .and_then(|template| template.render(json!({})))
            .expect("renders")
    };
    assert_eq!(render("a.txt"), "added");
    assert_eq!(render("b.txt"), "loaded");
}

#[test]
fn a_loaded_template_with_a_syntax_error_says_so() {
    let mut env = Environment::new();
    env.set_loader(|_| Ok(Some(TemplateSource::new("{% if %}"))));
    let error = env.get_template("a.txt").expect_err("syntax");
    assert_eq!(error.kind(), ErrorKind::SyntaxError);
    assert_eq!(error.name(), Some("a.txt"));
}

#[test]
fn environments_are_shared_across_threads() {
    fn is_shareable<T: Send + Sync>() {}
    is_shareable::<Environment>();

    let mut env = Environment::new();
    env.add_template("a.txt", "{{ n }}").expect("compiles");
    let env = Arc::new(env);
    let threads: Vec<_> = (0..4)
        .map(|n| {
            let env = env.clone();
            std::thread::spawn(move || {
                env.get_template("a.txt")
                    .and_then(|template| template.render(json!({ "n": n })))
                    .expect("renders")
            })
        })
        .collect();
    for (n, thread) in threads.into_iter().enumerate() {
        assert_eq!(thread.join().expect("joins"), n.to_string());
    }
}
