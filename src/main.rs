use anyhow::Result;
use rand::{
    prelude::{Distribution, SliceRandom},
    Rng,
};

use metainfo::Metainfo;
use peer_list::PeerList;

mod bencode;
mod metainfo;
mod peer_list;

struct Digits;

impl Distribution<char> for Digits {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> char {
        *b"0123456789".choose(rng).unwrap() as char
    }
}

pub type PeerID = [u8; 20];

pub struct ClientConfig {
    pub peer_id: PeerID,
    pub port: u16,
    pub timeout: std::time::Duration,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let random_digits_string: String = rand::thread_rng().sample_iter(&Digits).take(12).collect();
    let peer_id = "-DP0001-".to_string() + &random_digits_string;

    // TODO: CLI args, config file - some other abstraction
    let client_config = ClientConfig {
        peer_id: peer_id.as_bytes().try_into()?,
        port: 6881,
        timeout: std::time::Duration::from_secs(2)
    };

    let metainfo = Metainfo::from_file(&args[1])?;
    let peers = PeerList::fetch_peers_from_metainfo(&metainfo, &client_config).await;

    println!("{:#?}", peers);

    Ok(())
}
