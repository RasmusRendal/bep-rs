use bep_rs::bep_state::BepState;
use bep_rs::bep_state_reference::BepStateRef;
use bep_rs::peer_connection::*;
use bep_rs::sync_directory::SyncFile;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::{thread, time};
use tokio::io;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_open_close() -> io::Result<()> {
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
        .add_peer("strange_name".to_string(), state1.lock().unwrap().get_id());
    state1
        .lock()
        .unwrap()
        .add_peer("stranger_name".to_string(), state2.lock().unwrap().get_id());

    let (client, server) = tokio::io::duplex(64);
    let mut connection1 = PeerConnection::new(client, state1, false);
    let mut connection2 = PeerConnection::new(server, state2, true);
    connection1.close().await.unwrap();
    connection2.close().await.unwrap();
    assert!(connection1.get_peer_name().unwrap() == "stranger_name".to_string());
    assert!(connection2.get_peer_name().unwrap() == "strange_name".to_string());
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
async fn test_close_nonpeer() -> io::Result<()> {
    // If we connect two nonpeered instances, the connection should close rapidly
    let _ = env_logger::builder().is_test(true).try_init();

    let statedir1 = tempfile::tempdir().unwrap().into_path();
    let state1 = Arc::new(Mutex::new(BepState::new(statedir1)));
    state1.lock().unwrap().set_name("con1".to_string());
    let statedir2 = tempfile::tempdir().unwrap().into_path();
    let state2 = Arc::new(Mutex::new(BepState::new(statedir2)));
    state2.lock().unwrap().set_name("con2".to_string());

    let (client, server) = tokio::io::duplex(64);
    let mut connection1 = PeerConnection::new(client, state1, false);
    let mut connection2 = PeerConnection::new(server, state2, true);
    connection1.wait_for_close().await?;
    connection2.wait_for_close().await?;

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
        id: None,
        path: dstfile.clone(),
        hash,
        modified_by: 0,
        synced_version: 0,
        versions: vec![(1, 1)],
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
        id: None,
        path: filepath,
        hash,
        modified_by: 0,
        synced_version: 1,
        versions: vec![(1, 1)],
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
    let state1ref = BepStateRef::new(state1.clone());
    let state2ref = BepStateRef::new(state2.clone());
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

    srcdir.generate_index(&state2ref).await;
    dstdir.generate_index(&state1ref).await;

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
        .sync_directory_with_peer(&dstdir, &peer2);

    let (client, server) = tokio::io::duplex(1024);
    let mut connection1 = PeerConnection::new(client, state1, false);
    let mut connection2 = PeerConnection::new(server, state2, true);

    log::info!("Waiting for testfile");
    thread::sleep(time::Duration::from_millis(200));
    connection1.get_directory(&dstdir).await.unwrap();

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_update_file() -> io::Result<()> {
    // After receiving a file, we should be able to change it
    let _ = env_logger::builder().is_test(true).try_init();

    // Constants
    let file_contents1 = "hello world";
    let file_contents2 = "hello other world";
    let filename = "testfile";

    let statedir1 = tempfile::tempdir().unwrap().into_path();
    let statedir2 = tempfile::tempdir().unwrap().into_path();
    let state1 = Arc::new(Mutex::new(BepState::new(statedir1)));
    let state2 = Arc::new(Mutex::new(BepState::new(statedir2)));
    state1.lock().unwrap().set_name("con1".to_string());
    state2.lock().unwrap().set_name("con2".to_string());
    let state1ref = BepStateRef::new(state1.clone());
    let state2ref = BepStateRef::new(state2.clone());

    let srcpath = tempfile::tempdir().unwrap().into_path();
    let dstpath = tempfile::tempdir().unwrap().into_path();

    {
        let mut helloworld = srcpath.clone();
        helloworld.push(filename);
        let mut o = File::create(helloworld)?;
        o.write_all(file_contents1.as_bytes())?;
    }

    let srcdir = state2
        .lock()
        .unwrap()
        .add_sync_directory(srcpath.clone(), None);
    let dstdir = state1
        .lock()
        .unwrap()
        .add_sync_directory(dstpath.clone(), Some(srcdir.id.clone()));

    srcdir.generate_index(&state2ref).await;
    dstdir.generate_index(&state1ref).await;

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
        .sync_directory_with_peer(&dstdir, &peer2);

    let (client, server) = tokio::io::duplex(1024);
    let mut connection1 = PeerConnection::new(client, state1.clone(), false);
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
    assert_eq!(contents, file_contents1);

    log::info!("Successfully synced the first file");

    // Now we overwrite the file in dstdir
    {
        let mut helloworld = dstpath.clone();
        helloworld.push(filename);
        let mut o = File::create(helloworld)?;
        o.write_all(file_contents2.as_bytes())?;
    }

    //srcdir.generate_index(&state2ref).await;
    log::info!("Generating second index");
    dstdir.generate_index(&state1ref).await;
    log::info!("Successfully generated second index");
    thread::sleep(time::Duration::from_millis(200));

    // And then we request the file we just overwrote
    // However, requesting this file shouldn't overwrite it in dstdir,
    // because the change we just did is newer than the index in state2
    connection1.get_directory(&dstdir).await?;
    let mut dstfile = dstpath.clone();
    dstfile.push("testfile");
    let file = File::open(dstfile);
    assert!(file.is_ok());
    let file = file.unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut actual_contents = String::new();
    buf_reader.read_to_string(&mut actual_contents)?;
    assert_eq!(actual_contents, file_contents2);

    let stateref = BepStateRef::new(state1);
    let i = dstdir.generate_index(&stateref).await;
    assert_eq!(i[0].versions.len(), 2);

    // Sync the file modified in dstdir to srcdir
    thread::sleep(time::Duration::from_millis(200));
    connection2.get_directory(&srcdir).await?;
    let mut dstfile = srcpath.clone();
    dstfile.push("testfile");
    let file = File::open(dstfile);
    assert!(file.is_ok());
    let file = file.unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;
    assert_eq!(contents, file_contents2);
    connection1.close().await?;
    connection2.close().await?;
    Ok(())
}
