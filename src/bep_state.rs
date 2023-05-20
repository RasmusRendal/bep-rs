use super::models::*;
use super::peer_connection::PeerConnection;
use super::sync_directory;
use diesel::prelude::*;
use std::fs;
use std::path::PathBuf;

use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations/");

/// This structure maintains the state of a bep
/// client
pub struct BepState {
    pub data_directory: PathBuf,
    connection: SqliteConnection,
    pub listeners: Vec<PeerConnection>,
}

fn from_i64(i: i64) -> u64 {
    u64::from_ne_bytes(i.to_ne_bytes())
}

fn from_u64(i: u64) -> i64 {
    i64::from_ne_bytes(i.to_ne_bytes())
}

impl BepState {
    /// Initialize the state. It tries to read a database
    /// from the location data_directory. If this fails,
    /// it creates a new database
    pub fn new(mut data_directory: PathBuf) -> Self {
        match data_directory.try_exists() {
            Ok(v) => {
                if !v {
                    if let Err(e) = fs::create_dir(&data_directory) {
                        panic!("Could not create data directory due to error {}", e);
                    }
                }
            }
            Err(_e) => {
                panic!("Could not access XDG CONFIG DIR");
            }
        }
        data_directory.push("db.sqlite");
        let mut connection = SqliteConnection::establish(data_directory.to_str().unwrap())
            .unwrap_or_else(|_| panic!("Error connecting to database"));
        if let Err(e) = connection.run_pending_migrations(MIGRATIONS) {
            panic!("Error when applying database migrations: {}", e);
        }
        let mut s = BepState {
            data_directory,
            connection,
            listeners: vec![],
        };
        if !s.is_initialized() {
            use crate::schema::device_options;

            // TODO: Generate random name, maybe copy code from Docker or something
            use rcgen::generate_simple_self_signed;
            let subject_alt_names =
                vec!["hello.world.example".to_string(), "localhost".to_string()];

            let scert = generate_simple_self_signed(subject_alt_names).unwrap();
            let new_options = DeviceOption {
                id: Some(1),
                device_name: "Device".to_string(),
                cert: scert.serialize_der().unwrap(),
                key: scert.serialize_private_key_der(),
            };
            diesel::insert_into(device_options::table)
                .values(&new_options)
                .execute(&mut s.connection)
                .expect("Error adding directory");
        }
        s
    }

    /// Get list of directories to be synced
    pub fn get_sync_directories(&mut self) -> Vec<sync_directory::SyncDirectory> {
        use super::schema::sync_folders::dsl::*;
        sync_folders
            .load::<SyncFolder>(&mut self.connection)
            .unwrap_or_default()
            .iter()
            .map(|x| sync_directory::SyncDirectory {
                id: x.id.clone().unwrap(),
                label: x.label.clone(),
                path: PathBuf::from(x.dir_path.clone()),
            })
            .collect::<Vec<_>>()
    }

    /// Get a specific sync directory
    pub fn get_sync_directory(&mut self, id: &String) -> Option<sync_directory::SyncDirectory> {
        // TODO: Handle this better
        self.get_sync_directories()
            .into_iter()
            .find(|dir| dir.id == *id)
    }

    fn get_versions(&mut self, id: i32) -> Vec<(u64, u64)> {
        use super::schema::sync_file_versions;
        sync_file_versions::dsl::sync_file_versions
            .filter(sync_file_versions::dsl::sync_file_id.eq(id))
            .load::<SyncFileVersion>(&mut self.connection)
            .unwrap()
            .iter()
            .map(|x| (from_i64(x.user_id), from_i64(x.version_id)))
            .collect::<Vec<(u64, u64)>>()
    }

    pub fn get_sync_files(&mut self, dir_id: &String) -> Vec<sync_directory::SyncFile> {
        use super::schema::sync_files::dsl::*;
        let dir = self.get_sync_directory(dir_id).unwrap();
        sync_files
            .filter(folder_id.eq(dir_id))
            .load::<SyncFile>(&mut self.connection)
            .unwrap()
            .iter()
            .map(|x| {
                let mut path = dir.path.clone();
                path.push(x.name.clone());
                let dir = sync_directory::SyncFile {
                    id: x.id,
                    path,
                    hash: x.hash.as_ref().unwrap().clone(),
                    modified_by: from_i64(x.modified_by),
                    synced_version: from_i64(x.synced_version_id),
                    versions: self.get_versions(x.id.unwrap()),
                };
                assert!(!dir.versions.is_empty());
                dir
            })
            .collect::<Vec<_>>()
    }

    pub fn update_sync_file(
        &mut self,
        dir: &sync_directory::SyncDirectory,
        file: &sync_directory::SyncFile,
    ) {
        use super::schema::sync_file_versions;
        use super::schema::sync_files;
        assert!(!file.versions.is_empty());
        let sync_file = SyncFile {
            id: file.id,
            name: file.get_name(dir),
            modified_by: from_u64(file.modified_by),
            sequence: from_u64(0),
            synced_version_id: from_u64(file.synced_version),
            hash: Some(file.hash.clone()),
            folder_id: dir.id.clone(),
        };
        let inserted: Vec<SyncFile> = diesel::insert_into(sync_files::table)
            .values(sync_file.clone())
            .on_conflict(sync_files::dsl::id)
            .do_update()
            .set(sync_file)
            .get_results(&mut self.connection)
            .unwrap();

        let inserted = inserted.first().unwrap().id.unwrap();

        let versions = sync_file_versions::dsl::sync_file_versions
            .filter(sync_file_versions::dsl::sync_file_id.eq(inserted))
            .order_by(sync_file_versions::dsl::id.asc())
            .load::<SyncFileVersion>(&mut self.connection)
            .unwrap()
            .len();

        // We shouldn't have more versions than the file being requested
        assert!(versions <= file.versions.len());

        if versions < file.versions.len() {
            let versions_to_insert = file.versions.as_slice()[versions..]
                .iter()
                .map(|x| SyncFileVersion {
                    id: None,
                    version_id: from_u64(x.1),
                    user_id: from_u64(x.0),
                    sync_file_id: inserted,
                })
                .collect::<Vec<_>>();

            diesel::insert_into(sync_file_versions::table)
                .values(versions_to_insert)
                .execute(&mut self.connection)
                .unwrap();
        }
        assert!(!self.get_versions(inserted).is_empty());
    }

    fn get_options(&mut self) -> Option<DeviceOption> {
        use super::schema::device_options::dsl::*;
        device_options
            .load::<DeviceOption>(&mut self.connection)
            .unwrap_or_default()
            .pop()
    }

    fn is_initialized(&mut self) -> bool {
        self.get_options().is_some()
    }

    pub fn get_id(&mut self) -> [u8; 32] {
        use ring::digest;
        let cert = self.get_options().unwrap().cert;
        let hash = digest::digest(&digest::SHA256, &cert);
        let hashbox: Box<[u8; 32]> = hash
            .as_ref()
            .to_owned()
            .into_boxed_slice()
            .try_into()
            .unwrap();
        *hashbox
    }

    pub fn get_short_id(&mut self) -> u64 {
        u64::from_ne_bytes(self.get_id()[0..8].try_into().unwrap())
    }

    pub fn get_certificate(&mut self) -> Vec<u8> {
        self.get_options().unwrap().cert
    }

    pub fn get_key(&mut self) -> Vec<u8> {
        self.get_options().unwrap().key
    }

    pub fn get_name(&mut self) -> String {
        self.get_options()
            .map(|x| x.device_name)
            .unwrap_or("Device".to_string())
    }

    pub fn set_name(&mut self, name: String) {
        use crate::schema::device_options;
        use crate::schema::device_options::dsl;
        if let Some(mut options) = self.get_options() {
            options.device_name = name.clone();
            diesel::insert_into(device_options::table)
                .values(options)
                .on_conflict(dsl::id)
                .do_update()
                .set(dsl::device_name.eq(name))
                .execute(&mut self.connection)
                .unwrap();
        } else {
            panic!("Invalid database: No options");
        }
    }

    /// Get list of peers to the client
    pub fn get_peers(&mut self) -> Vec<Peer> {
        use super::schema::peers::dsl::*;
        peers.load::<Peer>(&mut self.connection).unwrap_or_default()
    }

    /// Get the list of addressess associated wit hsome peer
    pub fn get_addresses(&mut self, peer: Peer) -> Vec<String> {
        use super::schema::peer_addresses::dsl::*;
        PeerAddress::belonging_to(&peer)
            .select(address)
            .load::<String>(&mut self.connection)
            .unwrap_or_default()
    }

    /// Stop syncing some directory
    pub fn remove_sync_directory(&mut self, to_remove: String) {
        use crate::schema::sync_folders::dsl::sync_folders;
        use crate::schema::sync_folders::id;

        diesel::delete(sync_folders.filter(id.eq(to_remove)))
            .execute(&mut self.connection)
            .expect("Error deleting folder");
    }

    /// Start syncing some directory
    pub fn add_sync_directory(
        &mut self,
        path: PathBuf,
        id: Option<String>,
    ) -> sync_directory::SyncDirectory {
        use crate::schema::sync_folders;
        use rand::distributions::{Alphanumeric, DistString};

        if !path.exists() {
            panic!("Folder {} not found", path.display());
        }

        let folder_id =
            id.unwrap_or_else(|| Alphanumeric.sample_string(&mut rand::thread_rng(), 12));

        let new_dir = SyncFolder {
            id: Some(folder_id.clone()),
            label: path.file_name().unwrap().to_owned().into_string().unwrap(),
            dir_path: path.display().to_string(),
        };

        diesel::insert_into(sync_folders::table)
            .values(&new_dir)
            .execute(&mut self.connection)
            .expect("Error adding directory");
        self.get_sync_directory(&folder_id).unwrap()
    }

    pub fn add_peer_vec_id(&mut self, peer_name: String, peer_id: Vec<u8>) -> Peer {
        let boxed_slice = peer_id.into_boxed_slice();
        let boxed_array: Box<[u8; 32]> = match boxed_slice.try_into() {
            Ok(ba) => ba,
            Err(o) => panic!("Expected a Vec of length {} but it was {}", 4, o.len()),
        };
        self.add_peer(peer_name, *boxed_array)
    }

    pub fn add_peer(&mut self, peer_name: String, peer_id: [u8; 32]) -> Peer {
        use crate::schema::peers;
        use crate::schema::peers::dsl::*;
        let p = Peer {
            id: None,
            device_id: Some(peer_id.to_vec()),
            name: peer_name.clone(),
        };
        diesel::insert_into(peers::table)
            .values(&p)
            .execute(&mut self.connection)
            .expect("Error adding peer");

        peers
            .filter(name.eq(peer_name))
            .limit(1)
            .load::<Peer>(&mut self.connection)
            .unwrap()
            .pop()
            .unwrap()
    }

    /// Allows the peer to request files from a directory
    pub fn sync_directory_with_peer(
        &mut self,
        directory: &sync_directory::SyncDirectory,
        peer: &Peer,
    ) {
        use crate::schema::folder_shares;
        use crate::schema::sync_folders::dsl::*;
        let s = sync_folders
            .filter(id.eq(&directory.id))
            .load::<SyncFolder>(&mut self.connection)
            .unwrap();
        assert!(!s.is_empty());

        let new_share = FolderShare {
            id: None,
            sync_folder_id: directory.id.clone(),
            peer_id: peer.id.unwrap(),
        };
        diesel::insert_into(folder_shares::table)
            .values(&new_share)
            .execute(&mut self.connection)
            .expect("Error syncing directory");
    }

    pub fn is_directory_synced(
        &mut self,
        directory: &sync_directory::SyncDirectory,
        peer: &Peer,
    ) -> bool {
        use crate::schema::folder_shares::dsl::*;

        folder_shares
            .filter(sync_folder_id.eq(directory.id.clone()))
            .filter(peer_id.eq(peer.id.unwrap()))
            .load::<FolderShare>(&mut self.connection)
            .map(|x| !x.is_empty())
            .unwrap_or(false)
    }
}
