//! Sheet imports and exports whose permission names a community plugin's
//! namespace.

use navree_contracts::{BoxError, ModuleBuilderPermissionsExt, Permission};
use navree_db::Uow;
use navree_kernel::{Module, ModuleBuilder, PluginInfo};
use navree_macros::injectable;
use navree_sheets::{
    Export, ExportContext, ExportPage, Exportable, Import, ImportContext, Importable,
    ModuleBuilderSheetsExt,
};
use navree_validation::Validate;
use serde::Deserialize;

use crate::support::builder;

mod navree_plugin_khata {
    use super::*;

    const KHATA: PluginInfo =
        PluginInfo::new("navree-plugin-khata", module_path!(), "khata", "1.0.0");

    pub struct KhataPlugin(pub bool);

    impl Module for KhataPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA);
            if self.0 {
                m.permissions(&[
                    Permission::new("khata.import", "Import Ludhiana ledger entries"),
                    Permission::new("khata.export", "Export Ludhiana ledger entries"),
                ]);
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Validate, Importable)]
struct LedgerImportRow {
    #[validate(length(min = 1))]
    pub party: String,
}

#[injectable]
struct LedgerImport;

impl Import for LedgerImport {
    const NAME: &'static str = "ledgers";
    type Row = LedgerImportRow;

    fn permission() -> Option<String> {
        Some("khata.import".to_owned())
    }

    async fn insert(
        &self,
        _ctx: &ImportContext,
        _uow: &Uow,
        _row: Self::Row,
    ) -> Result<(), BoxError> {
        Ok(())
    }
}

#[derive(Debug, Exportable)]
#[export(sheet = "Ledgers")]
struct LedgerExportRow {
    pub party: String,
}

#[injectable]
struct LedgerExport;

impl Export for LedgerExport {
    const NAME: &'static str = "ledgers";
    type Row = LedgerExportRow;
    type Params = ();

    fn permission() -> Option<String> {
        Some("khata.export".to_owned())
    }

    async fn rows(
        &self,
        _ctx: &ExportContext,
        _params: &(),
        _page: ExportPage,
    ) -> Result<Vec<Self::Row>, BoxError> {
        Ok(Vec::new())
    }
}

struct AppSheetImport;

impl Module for AppSheetImport {
    fn register(&self, m: &mut ModuleBuilder) {
        m.importer::<LedgerImport>();
    }
}

struct AppSheetExport;

impl Module for AppSheetExport {
    fn register(&self, m: &mut ModuleBuilder) {
        m.export::<LedgerExport>();
    }
}

#[tokio::test]
async fn a_sheet_import_or_export_that_requires_such_a_permission_fails_the_boot() {
    use navree_plugin_khata::KhataPlugin;

    let import_error = match builder()
        .module(KhataPlugin(false))
        .module(AppSheetImport)
        .try_build()
        .await
    {
        Ok(_) => panic!("boot must fail when `khata.import` is not described by the plugin"),
        Err(error) => error.to_string(),
    };
    let expected_import = "plugin `navree-plugin-khata` uses the namespace `khata`, the first word of the permission `khata.import` that the sheet import `ledgers` of `AppSheetImport` requires, and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word";
    assert!(
        import_error.contains(expected_import),
        "expected `{expected_import}` in:\n{import_error}"
    );

    let export_error = match builder()
        .module(KhataPlugin(false))
        .module(AppSheetExport)
        .try_build()
        .await
    {
        Ok(_) => panic!("boot must fail when `khata.export` is not described by the plugin"),
        Err(error) => error.to_string(),
    };
    let expected_export = "plugin `navree-plugin-khata` uses the namespace `khata`, the first word of the permission `khata.export` that the sheet export `ledgers` of `AppSheetExport` requires, and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word";
    assert!(
        export_error.contains(expected_export),
        "expected `{expected_export}` in:\n{export_error}"
    );

    let app = builder()
        .module(KhataPlugin(true))
        .module(AppSheetImport)
        .module(AppSheetExport)
        .build()
        .await;
    app.shutdown().await.unwrap();
}
