use bep_rs::bep_state_reference::BepStateRef;
use bep_rs::models::Peer;
use bep_rs::peer_connection::*;
use bep_rs::sync_directory::SyncBlock;
use bep_rs::sync_directory::SyncDirectory;
use bep_rs::sync_directory::SyncFile;
use bep_rs::DeviceID;
use error::PeerCommandError;
use error::PeerConnectionError;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use std::{thread, time};
use tokio::io;

const FILE_CONTENTS: &str = "hello world";
const FILE_CONTENTS2: &str = "hello other world";
const FILE_SIZE1: i32 = FILE_CONTENTS.len() as i32;
const FILE_NAME: &str = "testfile";
const FILE_HASH: &[u8] = b"\xb9\x4d\x27\xb9\x93\x4d\x3e\x08\xa5\x2e\x52\xd7\xda\x7d\xab\xfa\xc4\x84\xef\xe3\x7a\x53\x80\xee\x90\x88\xf7\xac\xe2\xef\xcd\xe9";

struct TestStruct {
    pub state1: BepStateRef,
    pub state2: BepStateRef,

    pub conn1: Option<PeerConnection>,
    pub conn2: Option<PeerConnection>,

    pub peer1: Option<Peer>,
    pub peer2: Option<Peer>,

    pub peer1dirpath: PathBuf,
    pub peer1dir: Option<SyncDirectory>,
    pub peer2dirpath: PathBuf,
    pub peer2dir: Option<SyncDirectory>,
}

impl TestStruct {
    pub async fn new() -> Self {
        let test_dir = PathBuf::from(tempfile::tempdir().unwrap().path());
        fs::create_dir(test_dir.clone()).unwrap();
        let mut statedir1 = test_dir.clone();
        statedir1.push("statedir1");
        let mut statedir2 = test_dir.clone();
        statedir2.push("statedir2");

        let state1 = BepStateRef::new(statedir1.clone());
        state1.set_name("con1".to_string()).await;
        let state2 = BepStateRef::new(statedir2.clone());
        state2.set_name("con2".to_string()).await;

        let mut peer1dirpath = test_dir.clone();
        peer1dirpath.push("peer1dir");
        fs::create_dir(peer1dirpath.clone()).unwrap();

        let mut peer2dirpath = test_dir.clone();
        peer2dirpath.push("peer2dir");
        fs::create_dir(peer2dirpath.clone()).unwrap();

        TestStruct {
            state1,
            state2,
            conn1: None,
            conn2: None,
            peer1: None,
            peer2: None,
            peer1dirpath,
            peer2dirpath,
            peer1dir: None,
            peer2dir: None,
        }
    }

    pub async fn peer(&mut self) {
        let peer1 = self
            .state2
            .add_peer("con1".to_string(), self.state1.get_id().await)
            .await;
        let peer2 = self
            .state1
            .add_peer("con2".to_string(), self.state2.get_id().await)
            .await;
        self.peer1 = Some(peer1);
        self.peer2 = Some(peer2);
    }

    pub async fn connect(
        &mut self,
    ) -> Result<(PeerConnection, PeerConnection), PeerConnectionError> {
        let (client, server) = tokio::io::duplex(64);
        let connection1 = PeerConnection::new(client, self.state1.clone(), false);
        let connection2 = PeerConnection::new(server, self.state2.clone(), true);
        self.conn1 = Some(connection1.clone());
        self.conn2 = Some(connection2.clone());
        connection1.wait_for_ready().await?;
        connection2.wait_for_ready().await?;
        Ok((connection1, connection2))
    }

    pub async fn add_sync_dirs(&mut self) {
        self.peer1dir = Some(
            self.state1
                .add_sync_directory(self.peer1dirpath.clone(), None)
                .await,
        );
        self.peer2dir = Some(
            self.state2
                .add_sync_directory(
                    self.peer2dirpath.clone(),
                    Some(self.peer1dir.as_ref().unwrap().id.clone()),
                )
                .await,
        );
        log::info!("We have assigned {:?} to connection1", self.peer1dirpath);
        log::info!("We have assigned {:?} to connection2", self.peer2dirpath);
    }

    pub async fn connect_sync_dirs(&mut self) {
        self.state1
            .sync_directory_with_peer(
                self.peer1dir.as_ref().unwrap(),
                self.peer2.as_ref().unwrap(),
            )
            .await;
        self.state2
            .sync_directory_with_peer(
                self.peer2dir.as_ref().unwrap(),
                self.peer1.as_ref().unwrap(),
            )
            .await;
    }

    /// Writes `contents` to `FILE_NAME` in `peer2dir`
    pub fn write_hello_file(&mut self, contents: &str) {
        let mut helloworld = self.peer2dirpath.clone();
        helloworld.push(FILE_NAME);
        let mut o = File::create(helloworld).unwrap();
        o.write_all(contents.as_bytes()).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_open_close() -> io::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test_struct = TestStruct::new().await;
    let _ = test_struct.peer().await;
    let (connection1, connection2) = test_struct.connect().await.unwrap();

    connection1.close().await.unwrap();
    connection2.close().await.unwrap();
    assert!(connection1.get_peer_name().await.unwrap() == "con2".to_string());
    assert!(connection2.get_peer_name().await.unwrap() == "con1".to_string());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_double_close_err() -> io::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test_struct = TestStruct::new().await;
    let _ = test_struct.peer().await;
    let (connection1, connection2) = test_struct.connect().await.unwrap();

    log::info!("we have connected");

    connection1.close().await?;
    log::info!("test_double_close_err: Close 1 completed");
    connection2.close().await?;
    log::info!("test_double_close_err: Close 2 completed");
    connection1.close().await?;
    log::info!("test_double_close_err: Close 3 completed");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_close_nonpeer() -> io::Result<()> {
    // If we connect two nonpeered instances, the connection should close rapidly
    let _ = env_logger::builder().is_test(true).try_init();
    let mut test_struct = TestStruct::new().await;
    let err = test_struct.connect().await;

    assert!(err.is_err());
    log::info!("We got the error {}", err.clone().err().unwrap());
    assert!(matches!(
        err.err().unwrap(),
        PeerConnectionError::UnknownPeer
    ));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_get_nonexistent_file() -> io::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test_struct = TestStruct::new().await;

    test_struct.peer().await;
    test_struct.add_sync_dirs().await;
    test_struct.connect_sync_dirs().await;

    let (connection1, connection2) = test_struct.connect().await.unwrap();

    let mut dstfile = test_struct.peer1dirpath.clone();
    dstfile.push(FILE_NAME);
    let file = SyncFile {
        id: None,
        path: PathBuf::from(FILE_NAME),
        hash: FILE_HASH.to_vec(),
        modified_by: 0,
        synced_version: 0,
        versions: vec![(test_struct.state2.get_short_id().await, 0)],
        blocks: vec![SyncBlock {
            offset: 0,
            size: FILE_SIZE1,
            hash: vec![],
        }],
    };

    log::info!("Getting file");
    let e = connection1
        .get_file(test_struct.peer1dir.as_ref().unwrap(), &file)
        .await;
    log::info!("Got response {:?}", e);
    assert!(e.is_err());
    assert!(matches!(e.err().unwrap(), PeerCommandError::NoSuchFile));

    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_get_file() -> io::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test_struct = TestStruct::new().await;

    test_struct.write_hello_file(FILE_CONTENTS);
    test_struct.peer().await;
    test_struct.add_sync_dirs().await;
    test_struct.connect_sync_dirs().await;

    let (connection1, connection2) = test_struct.connect().await.unwrap();

    let mut dstfile = test_struct.peer1dirpath.clone();
    dstfile.push(FILE_NAME);
    let file = SyncFile {
        id: None,
        path: PathBuf::from(FILE_NAME),
        hash: FILE_HASH.to_vec(),
        modified_by: 0,
        synced_version: 0,
        versions: vec![(test_struct.state2.get_short_id().await, 0)],
        blocks: vec![SyncBlock {
            offset: 0,
            size: FILE_SIZE1,
            hash: vec![],
        }],
    };

    connection1.wait_for_ready().await.unwrap();
    connection2.wait_for_ready().await.unwrap();
    log::info!("Requesting file");
    connection1
        .get_file(&test_struct.peer1dir.as_ref().unwrap(), &file)
        .await
        .unwrap();
    log::info!("File acquired");
    let file = File::open(dstfile).unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;
    assert_eq!(contents, FILE_CONTENTS);
    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_nonsynced_directory() -> io::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test_struct = TestStruct::new().await;
    test_struct.peer().await;
    test_struct.write_hello_file(FILE_CONTENTS);
    test_struct.add_sync_dirs().await;

    let (connection1, connection2) = test_struct.connect().await.unwrap();

    let mut dstfile = test_struct.peer1dirpath.clone();
    dstfile.push(FILE_NAME);
    let file = SyncFile {
        id: None,
        path: PathBuf::from(FILE_NAME),
        hash: FILE_HASH.to_vec(),
        modified_by: 0,
        synced_version: 0,
        versions: vec![(test_struct.state2.get_short_id().await, 0)],
        blocks: vec![SyncBlock {
            offset: 0,
            size: FILE_SIZE1,
            hash: vec![],
        }],
    };

    let e = connection1
        .get_file(test_struct.peer1dir.as_ref().unwrap(), &file)
        .await;

    assert!(e.is_err());
    assert!(matches!(e.err().unwrap(), PeerCommandError::InvalidFile));
    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_get_directory() -> io::Result<()> {
    // Instead of requesting testfile, we should request an entire directory, and the testfile
    // should be in there
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test_struct = TestStruct::new().await;
    test_struct.write_hello_file(FILE_CONTENTS);
    test_struct.peer().await;
    test_struct.add_sync_dirs().await;
    test_struct.connect_sync_dirs().await;

    test_struct
        .peer1dir
        .as_ref()
        .unwrap()
        .generate_index(&test_struct.state1)
        .await;

    test_struct
        .peer2dir
        .as_ref()
        .unwrap()
        .generate_index(&test_struct.state2)
        .await;

    let (connection1, connection2) = test_struct.connect().await.unwrap();

    thread::sleep(time::Duration::from_millis(400));
    connection1
        .get_directory(test_struct.peer1dir.as_ref().unwrap())
        .await
        .unwrap();

    let mut dstfile = test_struct.peer1dirpath.clone();
    dstfile.push("testfile");
    let file = File::open(dstfile);
    assert!(file.is_ok());
    let file = file.unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;
    assert_eq!(contents, FILE_CONTENTS);
    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_update_file() -> io::Result<()> {
    // After receiving a file, we should be able to change it
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test_struct = TestStruct::new().await;
    test_struct.write_hello_file(FILE_CONTENTS);
    test_struct.peer().await;
    test_struct.add_sync_dirs().await;
    test_struct.connect_sync_dirs().await;

    test_struct
        .peer1dir
        .as_ref()
        .unwrap()
        .generate_index(&test_struct.state1)
        .await;

    test_struct
        .peer2dir
        .as_ref()
        .unwrap()
        .generate_index(&test_struct.state2)
        .await;

    let (connection1, connection2) = test_struct.connect().await.unwrap();
    // Wait for the index to be received
    // TODO: Introduce a call that lets us wait until the connection is set up
    thread::sleep(time::Duration::from_millis(200));
    connection1
        .get_directory(test_struct.peer1dir.as_ref().unwrap())
        .await
        .unwrap();

    // Now we requets the file from the peer

    let mut dstfile = test_struct.peer1dirpath.clone();
    dstfile.push("testfile");
    let file = File::open(dstfile);
    assert!(file.is_ok());
    let file = file.unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;
    assert_eq!(contents, FILE_CONTENTS);

    log::info!("Successfully synced the first file");

    // Now we overwrite the file ourselves, and notify our peer

    test_struct.write_hello_file(FILE_CONTENTS2);

    log::info!("Generating second index");
    connection2
        .directory_updated(test_struct.peer1dir.as_ref().unwrap())
        .await;

    // And then we request the file we just overwrote
    // However, requesting this file shouldn't overwrite it in dstdir,
    // because the change we just did is newer than the index in state2

    log::info!("Successfully generated second index");
    thread::sleep(time::Duration::from_millis(200));

    let mut dstfile = test_struct.peer1dirpath.clone();
    dstfile.push(FILE_NAME);
    let file = File::open(dstfile);
    assert!(file.is_ok());
    let file = file.unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut actual_contents = String::new();
    buf_reader.read_to_string(&mut actual_contents)?;
    assert_eq!(actual_contents, FILE_CONTENTS2);

    let i = test_struct
        .peer1dir
        .as_ref()
        .unwrap()
        .generate_index(&test_struct.state1)
        .await;

    assert_eq!(i.len(), 1);
    for v in &i[0].versions {
        log::info!("Version {} was authored by {}", v.1, v.0);
    }
    assert_eq!(i[0].versions.len(), 2);

    // Sync the file modified in dstdir to srcdir
    thread::sleep(time::Duration::from_millis(200));
    connection2
        .get_directory(test_struct.peer2dir.as_ref().unwrap())
        .await
        .unwrap();

    let mut dstfile = test_struct.peer2dirpath.clone();
    dstfile.push(FILE_NAME);
    let file = File::open(dstfile);
    assert!(file.is_ok());
    let file = file.unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;
    assert_eq!(contents, FILE_CONTENTS2);
    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_track_dir() -> io::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test_struct = TestStruct::new().await;

    test_struct.peer().await;
    test_struct.add_sync_dirs().await;
    test_struct.connect_sync_dirs().await;

    let (connection1, mut connection2) = test_struct.connect().await.unwrap();

    connection1.wait_for_ready().await.unwrap();
    connection2.wait_for_ready().await.unwrap();
    connection2.watch();

    thread::sleep(time::Duration::from_millis(200));

    test_struct.write_hello_file(FILE_CONTENTS);

    thread::sleep(time::Duration::from_millis(200));
    let mut dstfile = test_struct.peer1dirpath.clone();
    dstfile.push(FILE_NAME);
    let file = File::open(dstfile).unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;
    assert_eq!(contents, FILE_CONTENTS);
    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_accept_dir_with_handler() -> io::Result<()> {
    // Instead of requesting testfile, we should request an entire directory, and the testfile
    // should be in there
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test_struct = TestStruct::new().await;

    test_struct.write_hello_file(FILE_CONTENTS);
    test_struct.peer().await;

    test_struct.peer1dir = Some(
        test_struct
            .state1
            .add_sync_directory(test_struct.peer1dirpath.clone(), None)
            .await,
    );
    test_struct
        .state1
        .sync_directory_with_peer(
            test_struct.peer1dir.as_ref().unwrap(),
            test_struct.peer2.as_ref().unwrap(),
        )
        .await;

    let state2c = test_struct.state2.clone();
    let dev1id = test_struct.state1.get_id().await.clone();
    let newsyncdir = test_struct.peer2dirpath.clone();
    test_struct
        .state2
        .set_new_folder_handler(Some(Arc::new(
            move |(id, device_id): (String, DeviceID)| {
                let dev1id = dev1id.clone();
                let state2c = state2c.clone();
                let newsyncdir = newsyncdir.clone();
                Box::pin(async move {
                    log::info!("New folder share request {}", id);
                    if device_id == dev1id {
                        log::info!("And it matches");
                        let sd = state2c.add_sync_directory(newsyncdir, Some(id)).await;
                        state2c
                            .sync_directory_with_peer(&sd, &state2c.get_peer_by_id(device_id).await)
                            .await;
                    }
                })
            },
        )))
        .await;

    test_struct
        .peer1dir
        .as_ref()
        .unwrap()
        .generate_index(&test_struct.state1)
        .await;

    let (connection1, connection2) = test_struct.connect().await.unwrap();

    thread::sleep(time::Duration::from_millis(400));
    connection1
        .get_directory(test_struct.peer1dir.as_ref().unwrap())
        .await
        .unwrap();

    let mut dstfile = test_struct.peer1dirpath.clone();
    dstfile.push("testfile");
    let file = File::open(dstfile);
    assert!(file.is_ok());
    let file = file.unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;
    assert_eq!(contents, FILE_CONTENTS);
    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}
