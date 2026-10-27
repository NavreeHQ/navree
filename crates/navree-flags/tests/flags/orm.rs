//! The global flag states as navree-orm models.

use navree_flags::entity::feature_flag;
use navree_flags::{Flag, FlagScope};
use navree_orm::Model as _;

use crate::support::{DealExport, NewPipeline, World};

type FlagRow = feature_flag::Model;

#[tokio::test]
async fn global_flag_states_are_orm_models() {
    let world = World::new().await;
    let flags = world.flags().await;
    flags.kill::<NewPipeline>().await.unwrap();
    flags
        .deactivate::<DealExport>(FlagScope::everyone())
        .await
        .unwrap();
    let db = world.db().await;

    assert_eq!(FlagRow::table_name(), "feature_flags");
    let killed = FlagRow::query()
        .where_(FlagRow::KILLED.eq(true))
        .pluck(&db, FlagRow::NAME)
        .await
        .unwrap();
    assert_eq!(killed, [NewPipeline::NAME]);
    let export = FlagRow::find(&db, DealExport::NAME.to_owned())
        .await
        .unwrap()
        .expect("the stored state");
    assert_eq!(export.everyone, Some(false));
    assert!(!export.killed);
}
