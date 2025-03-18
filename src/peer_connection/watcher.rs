use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, StreamExt,
};
use notify::{self, RecursiveMode, Watcher};
use notify::{Config, Event, RecommendedWatcher};

use super::PeerConnection;

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
pub async fn watch(peer_connection: PeerConnection) -> Result<(), notify::Error> {
    let peer = peer_connection.get_peer().await.unwrap();
    let (mut watcher, mut rx) = async_watcher()?;

    let dirs = peer_connection.state.get_sync_directories().await;
    for dir in dirs {
        if peer_connection
            .state
            .is_directory_synced(dir.id.clone(), peer.id.unwrap())
            .await
        {
            if let Some(path) = &dir.path {
                watcher.watch(path, RecursiveMode::Recursive)?;
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
                    if let Err(e) = peer_connection.send_index().await {
                        log::error!(
                            "{}: Error when trying to send an updated index: {}",
                            peer_connection.get_name().await,
                            e
                        );
                    }
                }
            }
            Some(Some(Err(e))) => return Err(e),
            Some(None) => return Ok(()),
            None => return Ok(()),
        }
    }
}
