use crate::device_id::DeviceID;
use crate::peer_connection::PeerConnection;
use crate::sync_directory::SyncDirectory;

use super::models::*;
use super::sync_directory;
use diesel::prelude::*;
use rand::distr::Alphanumeric;
use rand::distr::SampleString;
use ring::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations/");

pub type BepEventHandler<Args> =
    Option<Arc<dyn Fn(Args) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>>;

pub type NewFolderHandler = BepEventHandler<SyncDirectory>;

/// This structure maintains the state of a bep
/// client
pub struct BepState {
    pub data_directory: PathBuf,
    connection: SqliteConnection,
    new_folder_handler: NewFolderHandler,
    /// A series of peer connections, which are informed about changes to the state.
    /// TODO: Remove them when they die
    peer_connections: Vec<PeerConnection>,
}

fn from_i64(i: i64) -> u64 {
    u64::from_ne_bytes(i.to_ne_bytes())
}

fn from_u64(i: u64) -> i64 {
    i64::from_ne_bytes(i.to_ne_bytes())
}

fn model_to_sync_dir(folder: SyncFolder) -> sync_directory::SyncDirectory {
    sync_directory::SyncDirectory {
        id: folder.id.clone(),
        label: folder.label.clone(),
        path: folder.dir_path.as_ref().map(PathBuf::from),
    }
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
            new_folder_handler: None,
            peer_connections: vec![],
        };
        if !s.is_initialized() {
            use crate::schema::device_options;

            use rcgen::generate_simple_self_signed;
            let subject_alt_names = vec!["syncthing".to_string()];

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

    pub fn get_directory_peers(&mut self, directory: &str) -> Vec<Peer> {
        use crate::schema::folder_shares;
        use crate::schema::peers;
        use crate::schema::sync_folders;

        peers::table
            .inner_join(folder_shares::table.inner_join(sync_folders::table))
            .filter(sync_folders::id.eq(directory))
            .select(peers::all_columns)
            .load::<Peer>(&mut self.connection)
            .unwrap()
    }

    pub fn get_synced_directories(
        &mut self,
        peer_id: i32,
    ) -> Vec<(sync_directory::SyncDirectory, Vec<Peer>)> {
        use crate::schema::folder_shares;
        use crate::schema::peers;
        use crate::schema::sync_folders;
        peers::table
            .inner_join(folder_shares::table.inner_join(sync_folders::table))
            .filter(peers::id.eq(peer_id))
            .select(sync_folders::all_columns)
            .load::<SyncFolder>(&mut self.connection)
            .unwrap()
            .into_iter()
            .map(|d| {
                let model = model_to_sync_dir(d);
                let id = model.id.clone();
                (model, self.get_directory_peers(&id))
            })
            .collect()
    }

    /// Get list of directories to be synced
    pub fn get_sync_directories(&mut self) -> Vec<sync_directory::SyncDirectory> {
        use super::schema::sync_folders::dsl::*;
        sync_folders
            .load::<SyncFolder>(&mut self.connection)
            .unwrap_or_default()
            .iter()
            .map(|x| sync_directory::SyncDirectory {
                id: x.id.clone(),
                label: x.label.clone(),
                path: x.dir_path.as_ref().map(PathBuf::from),
            })
            .collect::<Vec<_>>()
    }

    /// Get a specific sync directory
    pub fn get_sync_directory(
        &mut self,
        folder_id: &String,
    ) -> Option<sync_directory::SyncDirectory> {
        // TODO: Handle this better
        self.get_sync_directories()
            .into_iter()
            .find(|dir| dir.id == *folder_id)
    }

    pub fn set_sync_directory_path(
        &mut self,
        dir: &sync_directory::SyncDirectory,
        path: Option<PathBuf>,
    ) {
        use super::schema::sync_folders::dsl::*;

        let path_string: Option<String> =
            path.clone().and_then(|p| p.to_str().map(|s| s.to_owned()));

        diesel::update(sync_folders.filter(id.eq(dir.id.clone())))
            .set(dir_path.eq(path_string))
            .execute(&mut self.connection)
            .unwrap();
        if path.is_some() {
            for conn in &self.peer_connections {
                let cc = conn.clone();
                let mut dr = dir.clone();
                dr.path = path.clone();
                tokio::spawn(async move {
                    cc.get_directory(&dr).await.unwrap();
                });
            }
        }
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

    pub fn get_blocks(&mut self, file_id: i32) -> Vec<sync_directory::SyncBlock> {
        use super::schema::sync_file_blocks::dsl::*;

        sync_file_blocks
            .filter(sync_file_id.eq(file_id))
            .order(offset.asc())
            .load::<SyncFileBlock>(&mut self.connection)
            .unwrap()
            .into_iter()
            .map(|b| sync_directory::SyncBlock {
                offset: b.offset,
                size: b.size,
                hash: b.hash,
            })
            .collect()
    }

    pub fn get_sync_files(&mut self, dir_id: &String) -> Vec<sync_directory::SyncFile> {
        use super::schema::sync_files::dsl::*;
        sync_files
            .filter(folder_id.eq(dir_id))
            .load::<SyncFile>(&mut self.connection)
            .unwrap()
            .iter()
            .map(|x| {
                let path = PathBuf::from(&x.name);
                let dir = sync_directory::SyncFile {
                    id: x.id,
                    path,
                    hash: x.hash.as_ref().unwrap().clone(),
                    modified_by: from_i64(x.modified_by),
                    synced_version: from_i64(x.synced_version_id),
                    versions: self.get_versions(x.id.unwrap()),
                    blocks: self.get_blocks(x.id.unwrap()),
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
        use super::schema::sync_file_blocks;
        use super::schema::sync_file_blocks::dsl::*;
        use super::schema::sync_file_versions;
        use super::schema::sync_files;

        assert!(!file.versions.is_empty());
        let sync_file = SyncFile {
            id: file.id,
            name: file.get_name(),
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

        if !file.blocks.is_empty() {
            diesel::delete(sync_file_blocks.filter(sync_file_id.eq(inserted)))
                .execute(&mut self.connection)
                .unwrap();

            diesel::insert_into(sync_file_blocks::table)
                .values(
                    file.blocks
                        .clone()
                        .into_iter()
                        .map(|b| SyncFileBlock {
                            id: None,
                            sync_file_id: inserted,
                            offset: b.offset,
                            size: b.size,
                            hash: b.hash,
                            weak_hash: 0,
                        })
                        .collect::<Vec<_>>(),
                )
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

    pub fn get_id(&mut self) -> DeviceID {
        use ring::digest;
        let cert = self.get_options().unwrap().cert;
        let hash = digest::digest(&digest::SHA256, &cert);
        let hashbox: Box<DeviceID> = hash
            .as_ref()
            .to_owned()
            .into_boxed_slice()
            .try_into()
            .unwrap();
        *hashbox
    }

    /// By the spec, returns an integer representing the first 64 bits of the
    /// device ID.
    pub fn get_short_id(&mut self) -> u64 {
        u64::from_ne_bytes(self.get_id()[0..8].try_into().unwrap())
    }

    pub fn get_certificate(&mut self) -> Vec<u8> {
        self.get_options().unwrap().cert
    }

    pub fn get_key(&mut self) -> Vec<u8> {
        self.get_options().unwrap().key
    }

    pub fn get_keypair(&mut self) -> EcdsaKeyPair {
        let key = self.get_key();
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &key).unwrap()
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

    pub fn add_address(&mut self, peer: Peer, addr: String) {
        use crate::schema::peer_addresses;
        diesel::insert_into(peer_addresses::table)
            .values(PeerAddress {
                id: None,
                address: addr,
                peer_id: peer.id,
            })
            .execute(&mut self.connection)
            .expect("Error adding address");
    }

    /// Stop syncing some directory
    pub fn remove_sync_directory(&mut self, to_remove: String) {
        use crate::schema::sync_folders::dsl::sync_folders;
        use crate::schema::sync_folders::id;

        diesel::delete(sync_folders.filter(id.eq(to_remove)))
            .execute(&mut self.connection)
            .expect("Error deleting folder");
    }

    /// Adds a sync directory to the database. If ID is unset, one is generated.
    /// If path is unset, this implies that it belongs to a peer, but we haven't
    /// accepted it yet.
    pub fn add_sync_directory(
        &mut self,
        path: Option<PathBuf>,
        label: String,
        id: Option<String>,
    ) -> sync_directory::SyncDirectory {
        use crate::schema::sync_folders;

        // If both path and id is none, then it's a directory that doesn't exist
        // on our side, and it doesn't exist with a peer either. This makes very
        // little sense.
        assert!(path.is_some() || id.is_some());

        if let Some(p) = &path {
            if !p.exists() {
                panic!("Folder {} not found", p.display());
            }
        }

        let folder_id = id
            .clone()
            .unwrap_or_else(|| Alphanumeric.sample_string(&mut rand::rng(), 12));

        let new_dir = SyncFolder {
            id: folder_id.clone(),
            label,
            dir_path: path.and_then(|p| p.to_str().map(|s| s.to_owned())),
        };

        diesel::insert_into(sync_folders::table)
            .values(&new_dir)
            .execute(&mut self.connection)
            .expect("Error adding directory");
        self.get_sync_directory(&folder_id).unwrap()
    }

    pub fn get_peer(&mut self, peer_name: String) -> Peer {
        use crate::schema::peers::dsl::*;
        peers
            .filter(name.eq(peer_name))
            .limit(1)
            .load::<Peer>(&mut self.connection)
            .unwrap()
            .pop()
            .unwrap()
    }

    pub fn get_peer_by_id(&mut self, dev_id: DeviceID) -> Peer {
        use crate::schema::peers::dsl::*;
        let bytes: Vec<u8> = dev_id.to_vec();
        peers
            .filter(device_id.eq(bytes))
            .limit(1)
            .load::<Peer>(&mut self.connection)
            .unwrap()
            .pop()
            .unwrap()
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
        let p = Peer {
            id: None,
            device_id: Some(peer_id.to_vec()),
            name: peer_name.clone(),
        };
        diesel::insert_into(peers::table)
            .values(&p)
            .execute(&mut self.connection)
            .expect("Error adding peer");

        self.get_peer(peer_name)
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

    pub fn is_directory_synced(&mut self, directory: String, peer: i32) -> bool {
        use crate::schema::folder_shares::dsl::*;

        folder_shares
            .filter(sync_folder_id.eq(directory))
            .filter(peer_id.eq(peer))
            .load::<FolderShare>(&mut self.connection)
            .map(|x| !x.is_empty())
            .unwrap_or(false)
    }

    /// Called when a connection received an as to yet unknown folder
    /// from a peer.
    pub fn new_folder(&mut self, directory: sync_directory::SyncDirectory) {
        if let Some(handler) = &self.new_folder_handler {
            let h = handler.clone();
            tokio::spawn(async move {
                h(directory).await;
            });
        }
    }

    pub fn set_new_folder_handler(&mut self, handler: NewFolderHandler) {
        self.new_folder_handler = handler;
    }

    pub fn add_peer_connection(&mut self, peer_connection: PeerConnection) {
        self.peer_connections.push(peer_connection);
    }

    pub fn directory_changed(&mut self, dir: &sync_directory::SyncDirectory) {
        for conn in &self.peer_connections {
            let cc = conn.clone();
            let dr = dir.clone();
            tokio::spawn(async move {
                if let Err(e) = cc.directory_updated(&dr).await {
                    log::error!("Error on directory changed handler: {}", e);
                }
            });
        }
    }
}
