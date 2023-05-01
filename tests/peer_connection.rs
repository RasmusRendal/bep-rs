use beercanlib::bep_state::BepState;
use beercanlib::peer_connection::*;
use beercanlib::sync_directory::SyncFile;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::{thread, time};
use tokio::io;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_open_close() -> io::Result<()> {
    console_subscriber::init();
    let _ = env_logger::builder().is_test(true).try_init();
    let statedir1 = tempfile::tempdir().unwrap().into_path();
    let state1 = Arc::new(Mutex::new(BepState::new(statedir1)));
    state1.lock().unwrap().set_name("con1".to_string());
    let statedir2 = tempfile::tempdir().unwrap().into_path();
    let state2 = Arc::new(Mutex::new(BepState::new(statedir2)));
    state2.lock().unwrap().set_name("con2".to_string());
    state2
        .lock()
        .unwrap()
        .add_peer("con1".to_string(), state1.lock().unwrap().get_id());
    state1
        .lock()
        .unwrap()
        .add_peer("con2".to_string(), state2.lock().unwrap().get_id());

    let (client, server) = tokio::io::duplex(64);
    let mut connection1 = PeerConnection::new(client, state1, false);
    let mut connection2 = PeerConnection::new(server, state2, true);
    connection1.close().await.unwrap();
    connection2.close().await.unwrap();
    assert!(connection1.get_peer_name().unwrap() == "con2".to_string());
    assert!(connection2.get_peer_name().unwrap() == "con1".to_string());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_double_close_err() -> io::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    let statedir1 = tempfile::tempdir().unwrap().into_path();
    let state1 = Arc::new(Mutex::new(BepState::new(statedir1)));
    state1.lock().unwrap().set_name("con1".to_string());
    let statedir2 = tempfile::tempdir().unwrap().into_path();
    let state2 = Arc::new(Mutex::new(BepState::new(statedir2)));
    state2.lock().unwrap().set_name("con2".to_string());
    state2
        .lock()
        .unwrap()
        .add_peer("con1".to_string(), state1.lock().unwrap().get_id());
    state1
        .lock()
        .unwrap()
        .add_peer("con2".to_string(), state2.lock().unwrap().get_id());

    let (client, server) = tokio::io::duplex(64);
    let mut connection1 = PeerConnection::new(client, state1, false);
    let mut connection2 = PeerConnection::new(server, state2, true);
    connection1.close().await?;
    log::info!("test_double_close_err: Close 1 completed");
    connection2.close().await?;
    log::info!("test_double_close_err: Close 2 completed");
    connection1.close().await?;
    log::info!("test_double_close_err: Close 3 completed");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_get_file() -> io::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    // Constants
    let file_contents = "hello world";
    let filename = "testfile";
    let hash: Vec<u8> = b"\xb9\x4d\x27\xb9\x93\x4d\x3e\x08\xa5\x2e\x52\xd7\xda\x7d\xab\xfa\xc4\x84\xef\xe3\x7a\x53\x80\xee\x90\x88\xf7\xac\xe2\xef\xcd\xe9".to_vec();

    let statedir1 = tempfile::tempdir().unwrap().into_path();
    let statedir2 = tempfile::tempdir().unwrap().into_path();
    let state1 = Arc::new(Mutex::new(BepState::new(statedir1)));
    let state2 = Arc::new(Mutex::new(BepState::new(statedir2)));
    state1.lock().unwrap().set_name("wrongname".to_string());
    state2.lock().unwrap().set_name("wrongname".to_string());
    state1.lock().unwrap().set_name("con1".to_string());
    state2.lock().unwrap().set_name("con2".to_string());

    let srcpath = tempfile::tempdir().unwrap().into_path();
    let dstpath = tempfile::tempdir().unwrap().into_path();

    {
        let mut helloworld = srcpath.clone();
        helloworld.push(filename);
        let mut o = File::create(helloworld)?;
        o.write_all(file_contents.as_bytes())?;
    }

    let srcdir = state2
        .lock()
        .unwrap()
        .add_sync_directory(srcpath.clone(), None);
    let dstdir = state1
        .lock()
        .unwrap()
        .add_sync_directory(dstpath.clone(), Some(srcdir.id.clone()));

    state1
        .lock()
        .unwrap()
        .add_peer("con2".to_string(), state2.lock().unwrap().get_id());
    let peer = state2
        .lock()
        .unwrap()
        .add_peer("con1".to_string(), state1.lock().unwrap().get_id());
    state2
        .lock()
        .unwrap()
        .sync_directory_with_peer(&srcdir, &peer);
    let (client, server) = tokio::io::duplex(1024);
    let mut connection1 = PeerConnection::new(client, state1, false);
    let mut connection2 = PeerConnection::new(server, state2, true);

    let mut dstfile = dstpath.clone();
    dstfile.push("testfile");
    let file = SyncFile {
        path: dstfile.clone(),
        hash,
        modified_by: 0,
        synced_version: 0,
        versions: vec![],
    };

    connection1.get_file(&dstdir, &file).await?;
    let file = File::open(dstfile).unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;
    assert_eq!(contents, file_contents);
    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}

// TODO: DRY
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_nonsynced_directory() -> io::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    // Constants
    let file_contents = "hello world";
    let filename = "testfile";
    let hash: Vec<u8> = b"\xb9\x4d\x27\xb9\x93\x4d\x3e\x08\xa5\x2e\x52\xd7\xda\x7d\xab\xfa\xc4\x84\xef\xe3\x7a\x53\x80\xee\x90\x88\xf7\xac\xe2\xef\xcd\xe9".to_vec();

    let statedir1 = tempfile::tempdir().unwrap().into_path();
    let statedir2 = tempfile::tempdir().unwrap().into_path();
    let state1 = Arc::new(Mutex::new(BepState::new(statedir1)));
    let state2 = Arc::new(Mutex::new(BepState::new(statedir2)));
    state1.lock().unwrap().set_name("con1".to_string());
    state2.lock().unwrap().set_name("con2".to_string());

    let srcpath = tempfile::tempdir().unwrap().into_path();
    let dstpath = tempfile::tempdir().unwrap().into_path();

    {
        let mut helloworld = srcpath.clone();
        helloworld.push(filename);
        let mut o = File::create(helloworld)?;
        o.write_all(file_contents.as_bytes())?;
    }

    let srcdir = state2
        .lock()
        .unwrap()
        .add_sync_directory(srcpath.clone(), None);
    let dstdir = state1
        .lock()
        .unwrap()
        .add_sync_directory(dstpath.clone(), Some(srcdir.id.clone()));

    state1
        .lock()
        .unwrap()
        .add_peer("con2".to_string(), state2.lock().unwrap().get_id());
    let _ = state2
        .lock()
        .unwrap()
        .add_peer("con1".to_string(), state1.lock().unwrap().get_id());
    let (client, server) = tokio::io::duplex(1024);
    let mut connection1 = PeerConnection::new(client, state1, false);
    let mut connection2 = PeerConnection::new(server, state2, true);

    let mut filepath = dstpath.clone();
    filepath.push("testfile");
    let file = SyncFile {
        path: filepath,
        hash,
        modified_by: 0,
        synced_version: 1,
        versions: vec![],
    };

    let e = connection1.get_file(&dstdir, &file).await;
    assert!(e.is_err());
    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_get_directory() -> io::Result<()> {
    // Instead of requesting testfile, we should request an entire directory, and the testfile
    // should be in there
    let _ = env_logger::builder().is_test(true).try_init();

    // Constants
    let file_contents = "hello world";
    let filename = "testfile";

    let statedir1 = tempfile::tempdir().unwrap().into_path();
    let statedir2 = tempfile::tempdir().unwrap().into_path();
    let state1 = Arc::new(Mutex::new(BepState::new(statedir1)));
    let state2 = Arc::new(Mutex::new(BepState::new(statedir2)));
    state1.lock().unwrap().set_name("con1".to_string());
    state2.lock().unwrap().set_name("con2".to_string());

    let srcpath = tempfile::tempdir().unwrap().into_path();
    let dstpath = tempfile::tempdir().unwrap().into_path();

    {
        let mut helloworld = srcpath.clone();
        helloworld.push(filename);
        let mut o = File::create(helloworld)?;
        o.write_all(file_contents.as_bytes())?;
    }

    let srcdir = state2
        .lock()
        .unwrap()
        .add_sync_directory(srcpath.clone(), None);
    let dstdir = state1
        .lock()
        .unwrap()
        .add_sync_directory(dstpath.clone(), Some(srcdir.id.clone()));

    let peer1 = state2
        .lock()
        .unwrap()
        .add_peer("con1".to_string(), state1.lock().unwrap().get_id());
    let peer2 = state1
        .lock()
        .unwrap()
        .add_peer("con2".to_string(), state2.lock().unwrap().get_id());
    state2
        .lock()
        .unwrap()
        .sync_directory_with_peer(&srcdir, &peer1);
    state1
        .lock()
        .unwrap()
        .sync_directory_with_peer(&srcdir, &peer2);

    let (client, server) = tokio::io::duplex(1024);
    let mut connection1 = PeerConnection::new(client, state1, false);
    let mut connection2 = PeerConnection::new(server, state2, true);

    // Wait for the index to be received
    // TODO: Introduce a call that lets us wait until the connection is set up
    thread::sleep(time::Duration::from_millis(200));
    connection1.get_directory(&dstdir).await?;

    let mut dstfile = dstpath.clone();
    dstfile.push("testfile");
    let file = File::open(dstfile);
    assert!(file.is_ok());
    let file = file.unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;
    assert_eq!(contents, file_contents);
    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}
