//! `navree make:module`: the files of a feature module.

use navree_cli::clock::Timestamp;
use navree_cli::make::module_files;
use navree_cli::names::Name;

/// 2026-10-25T14:30:05Z, so migration names are stable.
fn now() -> Timestamp {
    Timestamp::from_unix(1_792_938_605)
}

fn name(s: &str) -> Name {
    Name::parse("module", s).unwrap()
}

#[test]
fn crud_module_has_the_feature_module_anatomy() {
    let files = module_files(&name("posts"), true, now()).unwrap();
    assert_eq!(
        files.paths(),
        [
            "posts/mod.rs",
            "posts/controller.rs",
            "posts/service.rs",
            "posts/entity.rs",
            "posts/dto.rs",
            "posts/migrations/mod.rs",
            "posts/migrations/m20261025_143005_create_posts_table.rs",
            "posts/tests/mod.rs",
        ]
    );
}

#[test]
fn make_module_crud_snapshot() {
    let files = module_files(&name("posts"), true, now()).unwrap();
    insta::assert_snapshot!("make_module_posts_crud", files.to_string());
}

#[test]
fn make_module_plain_snapshot() {
    let files = module_files(&name("posts"), false, now()).unwrap();
    insta::assert_snapshot!("make_module_posts_plain", files.to_string());
}

#[test]
fn multi_word_names_follow_rust_and_http_conventions() {
    let files = module_files(&name("blog_posts"), true, now()).unwrap();
    let module = files.get("blog_posts/mod.rs").unwrap();
    assert!(module.contains("pub struct BlogPostsModule;"), "{module}");
    let controller = files.get("blog_posts/controller.rs").unwrap();
    assert!(
        controller.contains(r#"#[controller("/blog-posts", version = "1")]"#),
        "{controller}"
    );
    assert!(
        controller.contains("pub struct BlogPostController"),
        "{controller}"
    );
    let entity = files.get("blog_posts/entity.rs").unwrap();
    assert!(
        entity.contains(r#"#[model(table = "blog_posts", timestamps, subject)]"#),
        "{entity}"
    );
    assert!(entity.contains("pub struct BlogPost {"), "{entity}");
    let service = files.get("blog_posts/service.rs").unwrap();
    assert!(
        service.contains("Accessible<BlogPost, Read>"),
        "the subject is the model: {service}"
    );
    assert!(
        files
            .get("blog_posts/migrations/m20261025_143005_create_blog_posts_table.rs")
            .is_some()
    );
}

#[test]
fn generated_rust_never_leaks_template_syntax() {
    for crud in [true, false] {
        let files = module_files(&name("posts"), crud, now()).unwrap();
        for file in files.iter() {
            for marker in ["{{", "}}", "{%", "%}", "{#"] {
                assert!(
                    !file.contents.contains(marker),
                    "{} contains `{marker}`",
                    file.path
                );
            }
            assert!(
                file.contents.ends_with('\n'),
                "{} ends with a newline",
                file.path
            );
            assert!(
                !file.contents.contains('\r'),
                "{} uses LF line endings",
                file.path
            );
        }
    }
}

#[test]
fn generated_modules_never_name_seaorm() {
    for crud in [true, false] {
        let files = module_files(&name("posts"), crud, now()).unwrap();
        for file in files.iter() {
            let contents = file.contents.to_lowercase();
            assert!(
                !contents.contains("sea_orm") && !contents.contains("sea-orm"),
                "{} names SeaORM",
                file.path
            );
        }
    }
}
