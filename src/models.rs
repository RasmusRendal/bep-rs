use super::schema::syncfolders;
use diesel::prelude::*;

#[derive(Queryable, Insertable)]
pub struct Syncfolder {
    pub id: Option<String>,
    pub label: String,
    pub dir_path: String,
}
