use std::{net::SocketAddr, collections::HashSet};
use reqwest::Url;
use anyhow::{anyhow, Result};

use crate::{metainfo::Metainfo, ClientConfig, bencode};

#[derive(Debug)]
pub struct PeerList(HashSet<SocketAddr>);

async fn fetch_peers_http(mut url: Url, metainfo: &Metainfo, client_config: &ClientConfig) -> HashSet<SocketAddr> {
    // We need to build up the query manually like this as Reqwest's in-built
    // urlencoding doesn't support encoding u8 slices.
    let mut query = String::new();
    query += "info_hash=";
    query += &urlencoding::encode_binary(&metainfo.info_hash);

    url.set_query(Some(&query));

    url.query_pairs_mut()
        .append_pair("peer_id", &client_config.peer_id)
        .append_pair("port", &client_config.port.to_string())
        // TODO: do any trackers require accurate values for these fields?
        .append_pair("uploaded", "0")
        .append_pair("downloaded","0")
        .append_pair("left", "0");

    let peer_list: Result<_> = async {
        let res = reqwest::get(url)
            .await?
            .bytes()
            .await?;

        // TODO: compact format
        let (_, peers) = bencode::parse_bencode(&res)
            .map_err(|_| anyhow!("Bencode parse error"))?;
        
        let peer_list = peers.as_dict()?
            .get("peers".as_bytes())
            .ok_or_else(|| anyhow!("Response contains no peers"))?
            .as_list()?
            .iter()
            .map(|peer| {
                let peer = peer.as_dict()?;
                let ip = peer.get("ip".as_bytes())
                    .ok_or_else(|| anyhow!("Peer has no IP field"))?
                    .as_str()?;
                
                let port = peer.get("port".as_bytes())
                    .ok_or_else(|| anyhow!("Peer has no port field"))?
                    .as_integer()?;

                Ok(SocketAddr::new(ip.parse()?, port.try_into()?))
            })
            .collect::<Result<HashSet<SocketAddr>>>()?;

        Ok(peer_list)

    }.await;

    match peer_list {
        Ok(peer_list) => peer_list,
        Err(e) => {
            eprintln!("Skipping tracker due to error: {}", e);
            HashSet::new()
        }
    }
}


impl PeerList {
    pub async fn fetch_peers_from_metainfo(metainfo: &Metainfo, client_config: &ClientConfig) -> Self {
        let mut peers = HashSet::new();

        for tracker in &metainfo.announce_list {
            let tracker_peers = match tracker.scheme() {
                "http" | "https" => fetch_peers_http(tracker.clone(), metainfo, client_config).await,
                 _ =>  {
                    eprintln!("Unknown protocol {} in tracker URL {}; skipping.", tracker.scheme(), tracker.as_str());
                    HashSet::new()
                }
            };

            peers.extend(tracker_peers);
        }

        Self(peers)
    }
}