use binread::BinRead;
use binwrite::BinWrite;

use reqwest::Url;
use tokio::net::UdpSocket;

use anyhow::{anyhow, Result};
use rand::Rng;
use std::{
    collections::HashSet,
    io::Cursor,
    net::{SocketAddr, SocketAddrV4},
};

use crate::{
    bencode,
    metainfo::{Metainfo, Sha1Hash},
    ClientConfig, PeerID,
};

#[derive(Debug)]
pub struct PeerList(pub HashSet<SocketAddr>);

async fn fetch_peers_http(
    mut url: Url,
    metainfo: &Metainfo,
    client_config: &ClientConfig,
) -> HashSet<SocketAddr> {
    // We need to build up the query manually like this as Reqwest's in-built
    // urlencoding doesn't support encoding u8 slices.
    let mut query = String::new();
    query += "info_hash=";
    query += &urlencoding::encode_binary(&metainfo.info_hash);

    url.set_query(Some(&query));

    let peer_list: Result<_> = async {
        url.query_pairs_mut()
            .append_pair("peer_id", std::str::from_utf8(&client_config.peer_id)?)
            .append_pair("port", &client_config.port.to_string())
            .append_pair("uploaded", "0")
            .append_pair("downloaded", "0")
            .append_pair("left", &metainfo.total_length.to_string());

        let res = reqwest::get(url).await?.bytes().await?;

        // TODO: compact format
        let (_, peers) =
            bencode::parse_bencode(&res).map_err(|_| anyhow!("Bencode parse error"))?;

        let peer_list = peers
            .as_dict()?
            .get("peers".as_bytes())
            .ok_or_else(|| anyhow!("Response contains no peers"))?
            .as_list()?
            .iter()
            .map(|peer| {
                let peer = peer.as_dict()?;
                let ip = peer
                    .get("ip".as_bytes())
                    .ok_or_else(|| anyhow!("Peer has no IP field"))?
                    .as_str()?;

                let port = peer
                    .get("port".as_bytes())
                    .ok_or_else(|| anyhow!("Peer has no port field"))?
                    .as_integer()?;

                Ok(SocketAddr::new(ip.parse()?, port.try_into()?))
            })
            .collect::<Result<HashSet<SocketAddr>>>()?;

        Ok(peer_list)
    }
    .await;

    match peer_list {
        Ok(peer_list) => peer_list,
        Err(e) => {
            eprintln!("Skipping tracker due to error: {}", e);
            HashSet::new()
        }
    }
}

#[derive(BinWrite, Debug)]
#[binwrite(big)]
struct UDPConnectRequest {
    pub magic: u64,
    pub action: u32,
    pub transaction_id: u32,
}

#[derive(BinRead, Debug)]
#[br(big)]
struct UDPConnectResponse {
    pub action: u32,
    pub transaction_id: u32,
    pub connection_id: u64,
}

#[derive(BinWrite, Debug)]
#[binwrite(big)]
struct UDPAnnounceRequest {
    pub connection_id: u64,
    pub action: u32,
    pub transaction_id: u32,
    pub info_hash: Sha1Hash,
    pub peer_id: PeerID,
    pub downloaded: u64,
    pub left: u64,
    pub uploaded: u64,
    pub event: u64,
    pub ip: u32,
    pub key: u32,
    pub num_want: i32,
    pub port: u16,
}

#[derive(BinRead, Debug)]
#[br(big)]
struct UDPPeer {
    pub ip: u32,
    pub port: u16,
}

#[derive(BinRead, Debug)]
#[br(import(len: usize), big)]
struct UDPAnnounceResponse {
    pub action: u32,
    pub transaction_id: u32,
    pub _interval: u32,
    pub _leechers: u32,
    pub _seeders: u32,

    #[br(count = len)]
    pub peers: Vec<UDPPeer>,
}

async fn fetch_peers_udp(
    url: Url,
    metainfo: &Metainfo,
    client_config: &ClientConfig,
) -> HashSet<SocketAddr> {
    let res: Result<_> = async {
        let ip = url
            .host()
            .ok_or_else(|| anyhow!("URL has no host"))?
            .to_string();
        let port = url.port().ok_or_else(|| anyhow!("URL has no port"))?;
        let addr = format!("{}:{}", ip, port);

        // STEP ONE: request a connection ID from the tracker
        // This is so the tracker knows that we do in fact have control over the IP in our
        // UDP header (as UDP has no handshake process)
        let transaction_id = rand::thread_rng().gen();
        let connect_packet = UDPConnectRequest {
            magic: 0x41727101980,
            action: 0,
            transaction_id,
        };

        let mut bytes = vec![];
        connect_packet.write(&mut bytes)?;

        // TODO: IPv6 support
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect(addr).await?;
        socket.send(&bytes).await?;

        let mut res = [0u8; 1024];
        socket.recv(&mut res).await?;
        let connect_reponse = UDPConnectResponse::read(&mut Cursor::new(res))?;

        if connect_reponse.action != 0 || connect_reponse.transaction_id != transaction_id {
            return Err(anyhow!(
                "Invalid connection reponse packet received from {}: {:?}",
                url,
                connect_reponse
            ));
        };

        // STEP 2: make the actual announce request that results in us receiving a list of peers
        let transaction_id = rand::thread_rng().gen();
        let announce_packet = UDPAnnounceRequest {
            connection_id: connect_reponse.connection_id,
            action: 1, // announce
            transaction_id,
            info_hash: metainfo.info_hash,
            peer_id: client_config.peer_id,
            downloaded: 0,
            left: metainfo.total_length as u64,
            uploaded: 0,
            event: 2, // started
            ip: 0,
            key: 0,
            num_want: -1,
            port: client_config.port,
        };

        let mut bytes = vec![];
        announce_packet.write(&mut bytes)?;
        socket.send(&bytes).await?;

        let mut res = [0u8; 1024];
        let res_length = socket.recv(&mut res).await?;
        let num_peers = (res_length - 20) / 6;
        let announce_reponse = UDPAnnounceResponse::read_args(&mut Cursor::new(res), (num_peers,))?;

        if announce_reponse.action != 1 || announce_reponse.transaction_id != transaction_id {
            return Err(anyhow!(
                "Invalid connection reponse packet received from {}: {:?}",
                url,
                announce_reponse
            ));
        };

        Ok(announce_reponse
            .peers
            .into_iter()
            .map(|peer| SocketAddr::V4(SocketAddrV4::new(peer.ip.into(), peer.port)))
            .collect())
    }
    .await;

    match res {
        Ok(set) => set,
        Err(e) => {
            eprintln!("Failed to retrive peers from tracker {}: {}", url, e);
            HashSet::new()
        }
    }
}

async fn fetch_peers(
    url: &Url,
    metainfo: &Metainfo,
    client_config: &ClientConfig,
) -> HashSet<SocketAddr> {
    match url.scheme() {
        "http" | "https" => fetch_peers_http(url.clone(), metainfo, client_config).await,
        "udp" => fetch_peers_udp(url.clone(), metainfo, client_config).await,
        _ => {
            async {
                eprintln!(
                    "Unknown protocol {} in tracker URL {}; skipping.",
                    url.scheme(),
                    url.as_str()
                );
                HashSet::new()
            }
            .await
        }
    }
}

impl PeerList {
    pub async fn fetch_peers_from_metainfo(
        metainfo: &Metainfo,
        client_config: &ClientConfig,
    ) -> Self {
        let mut tracker_peer_futures = Vec::new();

        for url in &metainfo.announce_list {
            // TODO: retry connection instead of just giving up after one failed attempt
            tracker_peer_futures.push(tokio::time::timeout(
                client_config.timeout,
                fetch_peers(url, metainfo, client_config),
            ));
        }

        let tracker_peer_sets: Vec<_> = futures::future::join_all(tracker_peer_futures)
            .await
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();

        let mut peers = HashSet::new();

        for tracker_peer_set in tracker_peer_sets {
            peers.extend(tracker_peer_set);
        }

        Self(peers)
    }
}
