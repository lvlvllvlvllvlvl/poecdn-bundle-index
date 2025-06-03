use sea_query::{InsertStatement, Query, SqliteQueryBuilder};
use crate::entity::{bundles, dirs, files};
use crate::entity::prelude::*;

pub fn insert_dirs() -> InsertStatement {
    Query::insert()
        .into_table(Dirs)
        .columns([dirs::Column::Id, dirs::Column::Name, dirs::Column::Parent])
        .to_owned()
}

pub fn insert_bundles() -> InsertStatement {
    Query::insert()
        .into_table(Bundles)
        .columns([
            bundles::Column::Id,
            bundles::Column::Name,
            bundles::Column::Size,
        ])
        .to_owned()
}

pub fn insert_files() -> InsertStatement {
    Query::insert()
        .into_table(Files)
        .columns([
            files::Column::Hash,
            files::Column::Dir,
            files::Column::Name,
            files::Column::Bundle,
            files::Column::Offset,
            files::Column::Size,
        ])
        .to_owned()
}
