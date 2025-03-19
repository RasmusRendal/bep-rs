use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, StreamExt,
};
use notify::{self, RecursiveMode, Watcher};
use notify::{Config, Event, RecommendedWatcher};

use super::{
    error::{PeerCommandError, PeerConnectionError},
    PeerConnection,
};

fn async_watcher() -> notify::Result<(RecommendedWatcher, Receiver<notify::Result<Event>>)> {
    let (mut tx, rx) = channel(1);

    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let watcher = RecommendedWatcher::new(
        move |res| {
            futures::executor::block_on(async {
                tx.send(res).await.unwrap();
            })
        },
        Config::default(),
    )?;

    Ok((watcher, rx))
}

// TODO: This is probably the wrong abstraction level.
// We end up with a watcher for each connection
pub async fn watch(peer_connection: PeerConnection) -> Result<(), PeerCommandError> {
    peer_connection.wait_for_ready().await?;
    let peer = peer_connection.get_peer().await?;
    let (mut watcher, mut rx) = async_watcher().unwrap();

    let dirs = peer_connection.state.get_sync_directories().await;
    for dir in dirs {
        if peer_connection
            .state
            .is_directory_synced(dir.id.clone(), peer.id.unwrap())
            .await
        {
            if let Some(path) = &dir.path {
                watcher.watch(path, RecursiveMode::Recursive).unwrap();
                log::info!(
                    "{} Watching {:?}",
                    peer_connection.get_name().await,
                    dir.path
                );
            }
        }
    }
    let cancellation_token = peer_connection.cancellation_token.clone();
    loop {
        match cancellation_token.run_until_cancelled(rx.next()).await {
            Some(Some(Ok(e))) => {
                if e.kind.is_create() || e.kind.is_modify() || e.kind.is_remove() {
                    for dir in peer_connection.state.get_sync_directories().await {
                        dir.generate_index(&peer_connection.state).await;
                    }
                }
            }
            Some(Some(Err(e))) => return Err(PeerCommandError::Other(e.to_string())),
            Some(None) => return Ok(()),
            None => return Ok(()),
        }
    }
}
