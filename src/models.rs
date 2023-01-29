use diesel::prelude::*;

#[derive(Queryable)]
pub struct Syncfolder {
    pub id: Option<String>,
    pub label: String,
    pub dir_path: String,
}
