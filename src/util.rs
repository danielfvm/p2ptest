use libp2p::{
    multiaddr::Protocol, Multiaddr, PeerId,
};
use std::{env, error::Error, fs, path::Path, str::FromStr};

/// Get the current ipfs repo path, either from the IPFS_PATH environment variable or
/// from the default $HOME/.ipfs
pub fn get_ipfs_path() -> Box<Path> {
    env::var("IPFS_PATH")
        .map(|ipfs_path| Path::new(&ipfs_path).into())
        .unwrap_or_else(|_| {
            env::var("HOME")
                .map(|home| Path::new(&home).join(".ipfs"))
                .expect("could not determine home directory")
                .into()
        })
}

/// Read the pre shared key file from the given ipfs directory
pub fn get_psk(path: &Path) -> std::io::Result<Option<String>> {
    let swarm_key_file = path.join("swarm.key");
    match fs::read_to_string(swarm_key_file) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// for a multiaddr that ends with a peer id, this strips this suffix. Rust-libp2p
/// only supports dialing to an address without providing the peer id.
pub fn strip_peer_id(addr: &mut Multiaddr) -> Option<PeerId> {
    let mut peer = None;
    let last = addr.pop();
    match last {
        Some(Protocol::P2p(peer_id)) => peer = Some(peer_id),
        Some(other) => addr.push(other),
        _ => {}
    }

    peer
}

/// parse a legacy multiaddr (replace ipfs with p2p), and strip the peer id
/// so it can be dialed by rust-libp2p
pub fn parse_legacy_multiaddr(text: &str) -> Result<(Multiaddr, Option<PeerId>), Box<dyn Error>> {
    let sanitized = text
        .split('/')
        .map(|part| if part == "ipfs" { "p2p" } else { part })
        .collect::<Vec<_>>()
        .join("/");
    let mut addr = Multiaddr::from_str(&sanitized)?;
    let peer_id = strip_peer_id(&mut addr);
    Ok((addr, peer_id))
}


pub fn pad_key(key: &Vec<u8>) -> [u8; 32] {
    let mut bytes = [0u8; 32];

    for i in 0..32.min(key.len()) {
        bytes[i] = key[i];
    }

    bytes
}

