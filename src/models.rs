use super::schema::*;
use diesel::prelude::*;

#[derive(Identifiable, Queryable, Insertable)]
pub struct DeviceOption {
    pub id: Option<i32>,
    pub device_name: String,
}

#[derive(Queryable, Insertable)]
pub struct SyncFolder {
    pub id: Option<String>,
    pub label: String,
    pub dir_path: String,
}

#[derive(Identifiable, Queryable, Insertable)]
pub struct Peer {
    pub id: Option<i32>,
    pub name: String,
}

#[derive(Queryable, Insertable, Associations)]
#[diesel(belongs_to(SyncFolder))]
pub struct FolderShare {
    pub id: Option<i32>,
    pub sync_folder_id: String,
    pub peer_id: i32,
}

#[derive(Identifiable, Queryable, Insertable, Associations)]
#[diesel(belongs_to(Peer))]
#[diesel(table_name = peer_addresses)]
pub struct PeerAddress {
    pub id: Option<i32>,
    pub address: String,
    pub peer_id: Option<i32>,
}
