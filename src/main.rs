use anyhow::Result;
use rand::{prelude::{SliceRandom, Distribution}, Rng};

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

pub struct ClientConfig {
    pub peer_id: String,
    pub port: u64,
}

#[tokio::main]  
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    
    let random_digits_string: String = rand::thread_rng()
        .sample_iter(&Digits)
        .take(12)
        .collect();

    let peer_id = "-DP0001-".to_string() + &random_digits_string;

    // TODO: CLI args, config file - some other abstraction
    let client_config = ClientConfig {
        peer_id: peer_id,
        port: 6881,
    };

    let metainfo = Metainfo::from_file(&args[1])?;
    let peers = PeerList::fetch_peers_from_metainfo(&metainfo, &client_config).await;

    println!("{:?}", peers);

    Ok(())
}
