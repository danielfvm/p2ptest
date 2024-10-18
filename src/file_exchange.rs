use either::Either;
use futures::prelude::*;
use libp2p::{
    core::transport::upgrade::Version, gossipsub, identify, identity, kad, noise, ping, pnet::PnetConfig, request_response::{self, ProtocolSupport, ResponseChannel}, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux, Multiaddr, PeerId, StreamProtocol, Swarm, Transport
};
use std::{collections::HashSet, env, error::Error, sync::Arc, time::Duration};
use tokio::{io, io::AsyncBufReadExt, select};
use tracing_subscriber::EnvFilter;
use serde::{Deserialize, Serialize};

use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use futures::StreamExt;

use crate::store::{File, FileHash};

#[derive(Clone)]
pub struct Client {
    pub sender: mpsc::Sender<Command>,
}

impl Client {
    /// Listen for incoming connections on the given address.
    pub async fn start_listening(
        &mut self,
        addr: Multiaddr,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::StartListening { addr, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Dial the given peer at the given address.
    pub async fn dial(
        &mut self,
        peer_id: PeerId,
        peer_addr: Multiaddr,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::Dial {
                peer_id,
                peer_addr,
                sender,
            })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Advertise the local node as the provider of the given file on the DHT.
    pub async fn start_providing(&mut self, file_hash: FileHash) {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::StartProviding { file_hash, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.");
    }

    /// Find the providers for the given file on the DHT.
    pub async fn get_providers(&mut self, file_hash: FileHash) -> HashSet<PeerId> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::GetProviders { file_hash, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Request the content of the given file from the given peer.
    pub async fn request_file(
        &mut self,
        peer: PeerId,
        file_hash: FileHash,
    ) -> Result<File, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::RequestFile {
                file_hash,
                peer,
                sender,
            })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not be dropped.")
    }

    /// Respond with the provided file content to the given request.
    pub async fn respond_file(
        &mut self,
        file: File,
        channel: ResponseChannel<FileResponse>,
    ) {
        self.sender
            .send(Command::RespondFile { file, channel })
            .await
            .expect("Command receiver not to be dropped.");
    }
}

#[derive(Debug)]
pub enum Command {
    StartListening {
        addr: Multiaddr,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    Dial {
        peer_id: PeerId,
        peer_addr: Multiaddr,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    StartProviding {
        file_hash: FileHash,
        sender: oneshot::Sender<()>,
    },
    GetProviders {
        file_hash: FileHash,
        sender: oneshot::Sender<HashSet<PeerId>>,
    },
    RequestFile {
        file_hash: FileHash,
        peer: PeerId,
        sender: oneshot::Sender<Result<File, Box<dyn Error + Send>>>,
    },
    RespondFile {
        file: File,
        channel: ResponseChannel<FileResponse>,
    },
}

// Simple file exchange protocol
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRequest(pub FileHash);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileResponse(pub File);
