//! Boot-time wiring checks: visibility and missing providers.

use navree_kernel::{KernelError, Navree};

use crate::support::{boot_errors, injectable, module};

mod visibility {
    use super::*;

    pub struct Repo;

    injectable!(UserService { repo: Repo });

    module!(DataModule, |m| {
        m.value(Repo).exports::<Repo>();
    });

    module!(PrivateDataModule, |m| {
        m.value(Repo);
    });

    module!(UsersModule, |m| {
        m.import(DataModule).provider::<UserService>();
    });

    module!(UsersWithPrivateData, |m| {
        m.import(PrivateDataModule).provider::<UserService>();
    });

    module!(UsersAlone, |m| {
        m.provider::<UserService>();
    });

    #[tokio::test]
    async fn resolves_exported_provider_from_imported_module() {
        let app = Navree::app().module(UsersModule).boot().await.expect("boot");
        let service = app.ctx().get::<UserService>().await.expect("UserService");
        let repo = app.ctx().get::<Repo>().await.expect("Repo");
        assert!(std::sync::Arc::ptr_eq(&service.repo, &repo));
    }

    #[tokio::test]
    async fn rejects_unexported_provider() {
        let errors = boot_errors(UsersWithPrivateData).await;
        assert!(
            matches!(
                errors.0.as_slice(),
                [KernelError::NotExported { module: "UsersWithPrivateData", owner: "PrivateDataModule", requested }]
                    if requested.ends_with("Repo")
            ),
            "{errors:?}"
        );
    }

    module!(UsersWithoutImport, |m| {
        m.provider::<UserService>();
    });

    module!(AppWithUnimportedData, |m| {
        m.imports((DataModule, UsersWithoutImport));
    });

    #[tokio::test]
    async fn rejects_provider_from_module_not_imported() {
        let errors = boot_errors(AppWithUnimportedData).await;
        assert!(
            matches!(
                errors.0.as_slice(),
                [KernelError::NotImported {
                    module: "UsersWithoutImport",
                    owner: "DataModule",
                    ..
                }]
            ),
            "{errors:?}"
        );
        assert!(errors.to_string().contains("does not import"), "{errors}");
    }

    module!(OtherDataModule, |m| {
        m.value(Repo).exports::<Repo>();
    });

    module!(UsersWithTwoRepos, |m| {
        m.imports((DataModule, OtherDataModule))
            .provider::<UserService>();
    });

    #[tokio::test]
    async fn rejects_provider_exported_by_two_imports() {
        let errors = boot_errors(UsersWithTwoRepos).await;
        assert!(
            matches!(
                errors.0.as_slice(),
                [KernelError::AmbiguousProvider { module: "UsersWithTwoRepos", owners, .. }]
                    if owners == &["DataModule", "OtherDataModule"]
            ),
            "{errors:?}"
        );
    }

    module!(RepoReexporter, |m| {
        m.import(DataModule).exports::<Repo>();
    });

    module!(UsersViaReexport, |m| {
        m.imports((RepoReexporter, DataModule))
            .provider::<UserService>();
    });

    #[tokio::test]
    async fn reexported_provider_is_the_same_instance() {
        let app = Navree::app()
            .module(UsersViaReexport)
            .boot()
            .await
            .expect("boot");
        let service = app.ctx().get::<UserService>().await.expect("UserService");
        let repo = app.ctx().get::<Repo>().await.expect("Repo");
        assert!(std::sync::Arc::ptr_eq(&service.repo, &repo));
    }

    module!(ExportsWhatItLacks, |m| {
        m.exports::<Repo>();
    });

    #[tokio::test]
    async fn rejects_export_of_unknown_provider() {
        let errors = boot_errors(ExportsWhatItLacks).await;
        assert!(
            matches!(
                errors.0.as_slice(),
                [KernelError::MissingProvider { module: "ExportsWhatItLacks", required_by, .. }]
                    if required_by.contains("exports")
            ),
            "{errors:?}"
        );
    }

    #[tokio::test]
    async fn reports_missing_provider_with_module_and_type() {
        let errors = boot_errors(UsersAlone).await;
        assert!(
            matches!(
                errors.0.as_slice(),
                [KernelError::MissingProvider {
                    module: "UsersAlone",
                    ..
                }]
            ),
            "{errors:?}"
        );
        let message = errors.to_string();
        assert!(message.contains("UsersAlone"), "{message}");
        assert!(message.contains(std::any::type_name::<Repo>()), "{message}");
        assert!(
            message.contains(std::any::type_name::<UserService>()),
            "names the dependent: {message}"
        );

        // The same error is found statically, without constructing anything.
        let errors = Navree::app()
            .module(UsersAlone)
            .validate()
            .expect_err("validate must fail");
        assert!(
            matches!(
                errors.0.as_slice(),
                [KernelError::MissingProvider {
                    module: "UsersAlone",
                    ..
                }]
            ),
            "{errors:?}"
        );
    }
}
