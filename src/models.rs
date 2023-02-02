use super::schema::*;
use diesel::prelude::*;

#[derive(Queryable, Insertable)]
pub struct Syncfolder {
    pub id: Option<String>,
    pub label: String,
    pub dir_path: String,
}

#[derive(Identifiable, Queryable, Insertable)]
pub struct Peer {
    pub id: Option<i32>,
    pub name: String,
}

#[derive(Identifiable, Queryable, Insertable, Associations)]
#[diesel(belongs_to(Peer))]
#[diesel(table_name = peer_addresses)]
pub struct PeerAddress {
    pub id: Option<i32>,
    pub address: String,
    pub peer_id: Option<i32>,
}
