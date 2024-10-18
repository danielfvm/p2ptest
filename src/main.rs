mod file_exchange;
mod node;
mod store;
mod util;

use base64::prelude::*;
use futures::prelude::*;
use libp2p::{
    core::transport::upgrade::Version,
    gossipsub, identify, identity, kad, noise, ping,
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, StreamProtocol, Transport,
};
use std::{env, error::Error, sync::Arc, time::Duration};
use tokio::{
    io::{self, AsyncBufReadExt},
    select, spawn,
};
use tracing_subscriber::EnvFilter;

use file_exchange::{FileRequest, FileResponse};
use util::{pad_key, parse_legacy_multiaddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let myfile = Arc::new(store::File {
        name: "test.txt".to_string(),
        data: include_bytes!("../Cargo.toml").to_vec(),
    });

    let mut test = store::Store::open("cache.db")
        .await
        .map_err(|e| format!("{:?}", e))?;
    // test.insert(myfile.clone()).await.map_err(|e| format!("{:?}", e))?;
    println!("{}", test.get(myfile.hash()).await.unwrap().name);

    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // Retrieve secret key for generating the environment variable
    let secret_key_seed = match env::var("SECRET_KEY") {
        Ok(seed) => Some(BASE64_STANDARD.decode(seed)?),
        Err(_) => None,
    };

    // Create a public/private key pair, either random or based on a seed.
    let secret_key = match secret_key_seed {
        Some(key) => identity::Keypair::ed25519_from_bytes(pad_key(&key)).unwrap(),
        None => identity::Keypair::generate_ed25519(),
    };
    let local_id = secret_key.public().to_peer_id();

    let (mut network_client, mut network_events, network_event_loop) = node::new(secret_key)
        .await
        .map_err(|e| format!("{:?}", e))?;

    // Spawn the network task for it to run in the background.
    spawn(network_event_loop.run());

    let known_peers = include_str!("../known_peers.txt").split("\n");
    for peer in known_peers {
        if let Ok((addr, peer_id)) = parse_legacy_multiaddr(peer) {
            if let Some(peer_id) = peer_id {
                if local_id != peer_id {
                    network_client
                        .dial(peer_id, addr)
                        .await
                        .map_err(|e| format!("{:?}", e))?;
                }
            }
        }
    }

    match env::var("LISTEN_ON") {
        // Listen on the given address
        Ok(addr) => network_client.start_listening(addr.parse()?),
        // Listen on all interfaces and whatever port the OS assigns
        Err(_) => network_client.start_listening("/ip4/0.0.0.0/tcp/0".parse()?),
    }
    .await
    .map_err(|e| format!("{:?}", e))?;

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();

    while let Ok(Some(line)) = stdin.next_line().await {
        //let cmd = line.split(' ').collect::<Vec<_>>();

        if line == "q" || line == "quit" {
            break;
        }
    }

    Ok(())
}
