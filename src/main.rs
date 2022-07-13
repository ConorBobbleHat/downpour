use std::collections::HashSet;

use anyhow::Result;
use downloader::Downloader;
use rand::{
    prelude::{Distribution, SliceRandom},
    Rng,
};

use metainfo::Metainfo;
use peer_list::PeerList;

mod bencode;
mod metainfo;
mod peer_list;
mod downloader;

struct Digits;

impl Distribution<char> for Digits {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> char {
        *b"0123456789".choose(rng).unwrap() as char
    }
}

pub type PeerID = [u8; 20];

#[derive(Clone)]
pub struct ClientConfig {
    pub peer_id: PeerID,
    pub port: u16,
    pub timeout: std::time::Duration,
    pub active_peers: usize,
    pub peer_update_interval: std::time::Duration,
    pub download_dir: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let random_digits_string: String = rand::thread_rng().sample_iter(&Digits).take(12).collect();
    let peer_id = "-DO0001-".to_string() + &random_digits_string;

    // TODO: CLI args, config file - some other abstraction
    let client_config = ClientConfig {
        peer_id: peer_id.as_bytes().try_into()?,
        port: 7881,
        timeout: std::time::Duration::from_secs(2),
        active_peers: 80,
        peer_update_interval: std::time::Duration::from_secs(10),
        download_dir: "./downloads".into(),
    };

    let metainfo = Metainfo::from_file(&args[1])?;
    //let peers = PeerList::fetch_peers_from_metainfo(&metainfo, &client_config).await;

    let peers = PeerList(HashSet::from_iter(
            vec![
                "192.168.68.107:60234",
            ]
            .into_iter()
            .map(|s| s.parse().unwrap())
    ));

    if peers.0.len() == 0 {
        eprintln!("Unable to source any peers; exiting.");
        return Ok(());
    }

    Downloader::new(metainfo, peers, client_config)
        .download()
        .await?;

    Ok(())
}
