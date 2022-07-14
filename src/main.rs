use std::{time::Duration, path::PathBuf};

use anyhow::Result;
use clap::Parser;
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

#[derive(Parser, Debug)]
#[clap(version, about)]
struct Args {
    /// Path to the metainfo of the torrent to be downloaded
    pub metainfo_file: PathBuf,

    /// The output directory for the downloaded torrent
    pub download_dir: std::path::PathBuf,
    
    /// Port reported to trackers as our incoming traffic port. Not currently used.
    #[clap(short, long, default_value_t=6881)]
    pub port: u16,

    /// Timeout (in seconds) for network-related operations
    #[clap(short, long, default_value_t=2.)]
    pub timeout: f32,

    /// The maximum number of active connections with peers held open simultaneously
    #[clap(short, long, default_value_t=8)]
    pub active_peers: usize,

    /// The interval (in seconds) at which new active peers are selected to fill any vacancies.
    #[clap(short='u', long, default_value_t=5.)]
    pub peer_update_interval: f32,
}

#[derive(Clone)]
pub struct ClientConfig {
    pub peer_id: PeerID,
    pub port: u16,
    pub timeout: std::time::Duration,
    pub active_peers: usize,
    pub peer_update_interval: std::time::Duration,
    pub download_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let random_digits_string: String = rand::thread_rng().sample_iter(&Digits).take(12).collect();
    let peer_id = "-DO0001-".to_string() + &random_digits_string;

    let args = Args::parse();

    // TODO: CLI args, config file - some other abstraction
    let client_config = ClientConfig {
        peer_id: peer_id.as_bytes().try_into()?,
        port: args.port,
        timeout: Duration::from_secs_f32(args.timeout),
        active_peers: args.active_peers,
        peer_update_interval: Duration::from_secs_f32(args.peer_update_interval),
        download_dir: args.download_dir.into(),
    };

    let metainfo = Metainfo::from_file(args.metainfo_file)?;
    let peers = PeerList::fetch_peers_from_metainfo(&metainfo, &client_config).await;

    if peers.0.is_empty() {
        eprintln!("Unable to source any peers; exiting.");
        return Ok(());
    }

    Downloader::new(metainfo, peers, client_config)
        .download()
        .await?;

    Ok(())
}
