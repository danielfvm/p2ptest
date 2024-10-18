mod util;
mod store;
mod file_exchange;
mod node;

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
use tokio::{io, io::AsyncBufReadExt, select};
use tracing_subscriber::EnvFilter;

use util::{pad_key, parse_legacy_multiaddr};
use file_exchange::{FileRequest, FileResponse};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let myfile = Arc::new(store::File {
        name: "test.txt".to_string(),
        data: include_bytes!("../Cargo.toml").to_vec()
    });


    let myfile2 = Arc::new(store::File {
        name: "test2.txt".to_string(),
        data: include_bytes!("../Cargo.toml").to_vec()
    });

    let mut test = store::Store::open("cache.db").await.map_err(|e| format!("{:?}", e))?;
    //test.insert(myfile.clone()).await.map_err(|e| format!("{:?}", e))?;
    println!("{}", test.get(myfile.hash()).await.unwrap().name);

    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // Create a Gosspipsub topic
    let gossipsub_topic = gossipsub::IdentTopic::new("chat");

    // We create a custom network behaviour that combines gossipsub, ping and identify.
    #[derive(NetworkBehaviour)]
    struct MyBehaviour {
        gossipsub: gossipsub::Behaviour,
        identify: identify::Behaviour,
        ping: ping::Behaviour,
        kademlia: kad::Behaviour<kad::store::MemoryStore>,
        request_response: request_response::cbor::Behaviour<FileRequest, FileResponse>,
    }

    // Retrieve secret key for generating the environment variable
    let secret_key = match env::var("SECRET_KEY") {
        Ok(seed) => Some(BASE64_STANDARD.decode(seed)?),
        Err(_) => None,
    };

    // Create a public/private key pair, either random or based on a seed.
    let id_keys = match secret_key {
        Some(key) => identity::Keypair::ed25519_from_bytes(pad_key(&key)).unwrap(),
        None => identity::Keypair::generate_ed25519(),
    };
    let peer_id = id_keys.public().to_peer_id();

    println!("{}", peer_id.to_base58());

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_other_transport(|key| {
            let noise_config = noise::Config::new(key).unwrap();
            let yamux_config = yamux::Config::default();

            let base_transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true));
            base_transport
                .upgrade(Version::V1Lazy)
                .authenticate(noise_config)
                .multiplex(yamux_config)
        })?
        .with_dns()?
        .with_behaviour(|key| {
            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .max_transmit_size(262144)
                .build()
                .map_err(|msg| io::Error::new(io::ErrorKind::Other, msg))?; // Temporary hack because `build` does not return a proper `std::error::Error`.
            Ok(MyBehaviour {
                kademlia: kad::Behaviour::new(
                    peer_id,
                    kad::store::MemoryStore::new(key.public().to_peer_id()),
                ),
                gossipsub: gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key.clone()),
                    gossipsub_config,
                )
                .expect("Valid configuration"),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/ipfs/0.1.0".into(),
                    key.public(),
                )),
                ping: ping::Behaviour::new(ping::Config::new()),
                request_response: request_response::cbor::Behaviour::new(
                    [(
                        StreamProtocol::new("/file-exchange/1"),
                        ProtocolSupport::Full,
                    )],
                    request_response::Config::default(),
                ),
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    println!("Subscribing to {gossipsub_topic:?}");
    swarm
        .behaviour_mut()
        .gossipsub
        .subscribe(&gossipsub_topic)
        .unwrap();

    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(kad::Mode::Server));

    // Reach out to other nodes if specified
    for to_dial in std::env::args().skip(1) {
        let addr: Multiaddr = parse_legacy_multiaddr(&to_dial)?;
        swarm.dial(addr)?;
        println!("Dialed {to_dial:?}")
    }

    let known_peers = include_str!("../known_peers.txt").split("\n");

    for peer in known_peers {
        if let Ok(addr) = parse_legacy_multiaddr(peer) {
            swarm.dial(addr)?;
            println!("Dialed {peer:?}")
        }
    }

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();

    match env::var("LISTEN_ON") {
        // Listen on the given address
        Ok(addr) => swarm.listen_on(addr.parse()?)?,
        // Listen on all interfaces and whatever port the OS assigns
        Err(_) => swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?,
    };

    // Kick it off
    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {
                if let Err(e) = swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(gossipsub_topic.clone(), line.as_bytes())
                {
                    println!("Publish error: {e:?}");
                }
            },
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        println!("Listening on {address:?}");
                    }
                    SwarmEvent::Behaviour(MyBehaviourEvent::Identify(event)) => {
                        println!("identify: {event:?}");
                    }
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                        propagation_source: peer_id,
                        message_id: id,
                        message,
                    })) => {
                        println!(
                            "Got message: {} with id: {} from peer: {:?}",
                            String::from_utf8_lossy(&message.data),
                            id,
                            peer_id
                        )
                    }
                    SwarmEvent::Behaviour(MyBehaviourEvent::Ping(event)) => {
                        match event {
                            ping::Event {
                                peer,
                                result: Result::Ok(rtt),
                                ..
                            } => {
                                println!(
                                    "ping: rtt to {} is {} ms",
                                    peer.to_base58(),
                                    rtt.as_millis()
                                );
                            }
                            ping::Event {
                                peer,
                                result: Result::Err(ping::Failure::Timeout),
                                ..
                            } => {
                                println!("ping: timeout to {}", peer.to_base58());
                            }
                            ping::Event {
                                peer,
                                result: Result::Err(ping::Failure::Unsupported),
                                ..
                            } => {
                                println!("ping: {} does not support ping protocol", peer.to_base58());
                            }
                            ping::Event {
                                peer,
                                result: Result::Err(ping::Failure::Other { error }),
                                ..
                            } => {
                                println!("ping: ping::Failure with {}: {error}", peer.to_base58());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
