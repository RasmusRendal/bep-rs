use super::schema::*;
use diesel::prelude::*;

// TODO: It might not be the most secure thing ever to store
// the certificate in the database, at least the real syncthing
// client doesn't do this.
#[derive(Identifiable, Queryable, Insertable)]
pub struct DeviceOption {
    pub id: Option<i32>,
    pub device_name: String,
    pub cert: Vec<u8>,
    pub key: Vec<u8>,
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
    pub device_id: Option<Vec<u8>>,
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

#[derive(Identifiable, Queryable, Insertable, PartialEq, Debug)]
pub struct SyncFile {
    pub id: Option<i32>,
    pub name: String,
    pub modified_by: i64,
    pub sequence: i64,
    pub synced_version_id: i64,
    pub hash: Option<Vec<u8>>,
    pub folder_id: String,
}

#[derive(Identifiable, Queryable)]
pub struct SyncFileVersion {
    pub id: Option<i32>,
    pub version_id: i64,
    pub user_id: i64,
    pub sync_file_id: Option<i32>,
}
