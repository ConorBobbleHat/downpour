use std::{io::{Cursor, SeekFrom}, net::SocketAddr, collections::HashMap, f32::consts::E, path::{PathBuf, Path}};

use anyhow::{anyhow, Result};
use binread::BinRead;
use binwrite::BinWrite;
use boolvec::BoolVec;
use futures::{stream::FuturesUnordered, StreamExt};
use rand::prelude::IteratorRandom;
use reqwest::Request;
use tokio::{io::{AsyncWriteExt, AsyncSeekExt}, net::TcpStream, sync::mpsc, fs::File};

use crate::{
    metainfo::{Metainfo, Sha1Hash, Info},
    peer_list::PeerList,
    ClientConfig, PeerID,
};

const BLOCK_LENGTH: u32 = 1 << 14; // in bytes. 1 << 14 == 16KB.

#[derive(BinRead, BinWrite, Debug)]
#[br(big)]
#[binwrite(big)]
struct Handshake {
    pstrlen: u8,
    #[br(count=pstrlen)]
    pstr: Vec<u8>,
    reserved: [u8; 8],
    info_hash: Sha1Hash,
    peer_id: PeerID,
}

#[derive(BinRead, BinWrite, Debug)]
#[br(big)]
#[binwrite(big)]
struct PacketHeader {
    len: u32,
    id: u8,
}

#[derive(BinRead, BinWrite, Debug)]
#[br(big)]
#[binwrite(big)]
struct HavePacket {
    header: PacketHeader,
    index: u32,
}

#[derive(BinRead, BinWrite, Debug)]
#[br(big)]
#[binwrite(big)]
struct BitfieldPacket {
    header: PacketHeader,
    #[br(count = header.len - 1)]
    bitfield: Vec<u8>,
}

#[derive(BinRead, BinWrite, Debug)]
#[br(big)]
#[binwrite(big)]
struct RequestPacket {
    header: PacketHeader,
    index: u32,
    begin: u32,
    length: u32,
}

#[derive(BinRead, BinWrite, Debug)]
#[br(big)]
#[binwrite(big)]
struct PiecePacket {
    header: PacketHeader,
    index: u32,
    begin: u32,
    #[br(count = header.len - 9)]
    block: Vec<u8>,
}

#[derive(BinRead, BinWrite, Debug)]
#[br(big)]
#[binwrite(big)]
struct CancelPacket {
    header: PacketHeader,
    index: u32,
    begin: u32,
    length: u32,
}

#[derive(Debug)]
enum Packet {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(HavePacket),
    Bitfield(BitfieldPacket),
    Request(RequestPacket),
    Piece(PiecePacket),
    Cancel(CancelPacket),
}

fn parse_packet(packet_buf: &[u8]) -> Result<Packet> {
    // What kind of packet is this?
    let packet_header = PacketHeader::read(&mut Cursor::new(packet_buf))?;

    if packet_header.len == 0 {
        return Ok(Packet::KeepAlive);
    }

    match packet_header.id {
        0 => Ok(Packet::Choke),
        1 => Ok(Packet::Unchoke),
        2 => Ok(Packet::Interested),
        3 => Ok(Packet::NotInterested),
        4 => Ok(Packet::Have(HavePacket::read(&mut Cursor::new(
            packet_buf,
        ))?)),
        5 => Ok(Packet::Bitfield(BitfieldPacket::read(&mut Cursor::new(
            packet_buf,
        ))?)),
        6 => Ok(Packet::Request(RequestPacket::read(&mut Cursor::new(
            packet_buf,
        ))?)),
        7 => Ok(Packet::Piece(PiecePacket::read(&mut Cursor::new(
            packet_buf,
        ))?)),
        8 => Ok(Packet::Cancel(CancelPacket::read(&mut Cursor::new(
            packet_buf,
        ))?)),
        _ => Err(anyhow!("Unknown packet with ID {}", packet_header.id)),
    }
}

fn read_packets(data_buf: &mut Vec<u8>) -> Result<Vec<Packet>> {
    let mut packets = Vec::new();

    // Do we have a full packet present in the buffer?
    loop {
        if data_buf.len() >= 4 {
            // Unwrap is safe here due to the length check above
            let packet_len = u32::from_be_bytes(data_buf[..4].try_into().unwrap()) as usize + 4;
            if data_buf.len() >= packet_len {
                let packet_slice: Vec<u8> = data_buf.drain(..packet_len).collect();
                let packet = parse_packet(&packet_slice)?;
                packets.push(packet);
            } else {
                // We don't have a full packet, continue
                break;
            }
        } else {
            // We don't even have a full packet length field; continue
            break;
        }
    }

    Ok(packets)
}

#[derive(Debug)]
struct PeerPacket {
    packet: Packet,
    peer: SocketAddr,
}

#[derive(Debug)]
enum PeerOutgoingMessage {
    Have { index: u32 },
    RequestBlock { index: u32, begin: u32, length: u32},
}

async fn peer_thread(
    peer: SocketAddr,
    client_config: ClientConfig,
    metainfo: Metainfo,
    manager_tx: mpsc::Sender<PeerPacket>,
    mut manager_rx: mpsc::Receiver<PeerOutgoingMessage>,
) -> Result<()> {
    {
        let mut stream =
            tokio::time::timeout(client_config.timeout, TcpStream::connect(peer)).await??;

        // Let's be polite, and handshake!
        let mut bytes = vec![];
        Handshake {
            pstrlen: 19,
            pstr: b"BitTorrent protocol".to_vec(),
            reserved: [0u8; 8],
            info_hash: metainfo.info_hash,
            peer_id: client_config.peer_id,
        }
        .write(&mut bytes)?;
        stream.write_all(&bytes).await?;

        stream.readable().await?;
        let mut buf = [0u8; 4096];
        let buf_len = stream.try_read(&mut buf)?;

        let mut handshake_cursor = Cursor::new(buf);
        let handshake_reply = Handshake::read(&mut handshake_cursor)?;

        if handshake_reply.pstrlen != 19
            || handshake_reply.pstr != b"BitTorrent protocol"
            || handshake_reply.info_hash != metainfo.info_hash
        {
            return Err(anyhow!("Invalid handshake received from peer"));
        }

        println!(
            "Connection established to {}",
            std::str::from_utf8(&handshake_reply.peer_id)?
        );

        // Immediately unchoke and register our interest in this peer
        let mut bytes = vec![];

        PacketHeader {
            len: 1,
            id: 1, // unchoke
        }
        .write(&mut bytes)?;

        PacketHeader {
            len: 1,
            id: 2, // interested
        }
        .write(&mut bytes)?;

        stream.write_all(&bytes).await?;

        let mut data_buf = buf[..buf_len][handshake_cursor.position() as usize..].to_vec();

        loop {
            let packets = read_packets(&mut data_buf)?;

            for packet in packets {
                manager_tx.send(PeerPacket { packet, peer }).await?;
            }

            tokio::select! {
                _ = stream.readable() => {
                    let mut buf = [0u8; 4096];
                    let buf_len = match stream.try_read(&mut buf) {
                        Ok(ok) => ok,
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            // False positive; turns out the stream wasn't readable.
                            continue;
                        }
                        Err(e) => return Err(e.into()),
                    };

                    data_buf.extend(&buf[..buf_len]);
                },

                msg = manager_rx.recv() => {
                    if let Some(msg) = msg {
                        match msg {
                            PeerOutgoingMessage::Have {index} => {
                                let mut bytes = vec![];
                                HavePacket {
                                    header: PacketHeader { len: 5, id: 4 },
                                    index,
                                }.write(&mut bytes)?;
                                stream.write_all(&bytes).await?;

                            },
                            PeerOutgoingMessage::RequestBlock { index, begin, length } => {
                                let mut bytes = vec![];
                                RequestPacket {
                                    header: PacketHeader { len: 13, id: 6 },
                                    index,
                                    begin,
                                    length
                                }.write(&mut bytes)?;
                                stream.write_all(&bytes).await?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

struct PeerState {
    choking_us: bool,
    interested_in_us: bool,
    bitfield: BoolVec,
    tx: mpsc::Sender<PeerOutgoingMessage>
}

#[derive(Debug, Clone)]
enum PieceState {
    Unstarted,
    Downloading { block_index: usize },
    Stalled { block_index: usize },
    Finished,
}

async fn preallocate_file(path: &Path, length: usize) -> Result<File> {
    // TODO: This definitely isn't the most efficient way to preallocate large files
    let mut f = tokio::fs::File::create(path).await?;

    // ~100MB buffer
    let buf = vec![0u8; 1 << 27];

    let mut remaining_bytes = length;

    while remaining_bytes > 0 {
        let bytes_to_write = std::cmp::min(buf.len(), remaining_bytes);
        f.write_all(&buf[..bytes_to_write]).await?;
        remaining_bytes -= bytes_to_write;
    }
    
    Ok(f)
}

fn flag_next_piece(metainfo: &Metainfo, peer_state: &PeerState, pieces_state: &mut Vec<PieceState>) -> Option<usize> {
    for piece_index in 0..metainfo.pieces.len() {
        if peer_state.bitfield.get(piece_index).unwrap_or(false) &&
            matches!(pieces_state[piece_index], PieceState::Unstarted | PieceState::Stalled { block_index: _ })
        {
            let block_index = if let PieceState::Stalled {block_index: b} = pieces_state[piece_index] {b} else {0};
            pieces_state[piece_index] = PieceState::Downloading { block_index };
            return Some(piece_index);
        }
    };

    return None;
}

async fn request_next_block(piece_index: usize, metainfo: &Metainfo, peer_state: &PeerState, pieces_state: &mut Vec<PieceState>) -> Result<()> {
    if let PieceState::Downloading { block_index } = pieces_state[piece_index] {
        let piece_len = if piece_index == metainfo.pieces.len() - 1 {
            metainfo.total_length % metainfo.piece_length as usize
        } else {
            metainfo.piece_length as usize
        };

        let num_blocks = (piece_len - 1) / (BLOCK_LENGTH as usize) + 1;
        let block_length = if piece_len % BLOCK_LENGTH as usize == 0 {
            BLOCK_LENGTH
        } else {
            if block_index + 1 == num_blocks {
                piece_len as u32 % BLOCK_LENGTH
            } else {
                BLOCK_LENGTH
            }
        };

        peer_state.tx.send(PeerOutgoingMessage::RequestBlock {
            index: piece_index as u32,
            begin: block_index as u32 * BLOCK_LENGTH,
            length: block_length
        }).await?;

        pieces_state[piece_index] = PieceState::Downloading { block_index: block_index + 1 };
        
        Ok(())
    } else {
        Err(anyhow!("request_next_block called on piece with a PieceState other than Downloading"))
    }
}

pub struct Downloader {
    metainfo: Metainfo,
    peers: PeerList,
    client_config: ClientConfig,
}

impl Downloader {
    pub fn new(metainfo: Metainfo, peers: PeerList, client_config: ClientConfig) -> Self {
        Self {
            metainfo,
            peers,
            client_config,
        }
    }

    pub async fn download(self) -> Result<()> {
        // First, preallocate space for all our files
        #[derive(Debug)]
        struct FileSpan {
            handle: File,
            start: usize,
            length: usize,
        }

        let mut file_handles = Vec::new();

        match self.metainfo.info {
            Info::SingleFile(ref file_info) => {
                let file_path = self.client_config.download_dir.join(&file_info.name);
                let f = preallocate_file(&file_path, file_info.length as usize).await?;
                file_handles.push(FileSpan {
                    handle: f,
                    start: 0,
                    length: file_info.length as usize,
                });
            },
            Info::Directory(ref files_info) => {
                tokio::fs::create_dir_all(self.client_config.download_dir.join(&files_info.name)).await?;

                let mut start = 0;
                for file in &files_info.files {
                    let file_path = self.client_config.download_dir.join(&files_info.name).join(file.path.iter().collect::<PathBuf>());
                    let f = preallocate_file(&file_path, file.length as usize).await?;
                    file_handles.push(FileSpan {
                        handle: f,
                        start,
                        length: file.length as usize,
                    });
                    start += file.length as usize;
                }
            },
        }

        // When we start downloading, we have no idea which peers have the best download speed
        // As such, just pick our starting set at random
        let mut rng = rand::thread_rng();
        let starting_peer_addrs = self
            .peers
            .0
            .into_iter()
            .choose_multiple(&mut rng, self.client_config.active_peers);

        let mut peer_update_interval =
            tokio::time::interval(self.client_config.peer_update_interval);

        let (tx, mut rx) = mpsc::channel(32);
        let mut peer_thread_futures = FuturesUnordered::new();
        let mut peer_states = HashMap::new();

        for peer in starting_peer_addrs {
            let (thread_tx, thread_rx) = mpsc::channel(32);

            let handle = tokio::spawn(peer_thread(
                peer,
                self.client_config.clone(),
                self.metainfo.clone(),
                tx.clone(),
                thread_rx,
            ));

            peer_thread_futures.push(handle);
            peer_states.insert(peer, PeerState {
                choking_us: true,
                interested_in_us: false,
                bitfield: BoolVec::filled_with(self.metainfo.pieces.len(), false),
                tx: thread_tx,
            });
        }

        let mut pieces_state = Vec::new();
        pieces_state.resize(self.metainfo.pieces.len(), PieceState::Unstarted);

        loop {
            tokio::select! {
                _ = peer_update_interval.tick() => {
                    println!("TODO: update peers");
                },

                peer_packet = rx.recv() => {
                    if let Some(peer_packet) = peer_packet {
                        let packet = peer_packet.packet;
                        let peer = peer_packet.peer;
                        let peer_state = peer_states.get_mut(&peer).unwrap();

                        match packet {
                            Packet::KeepAlive => todo!(),
                            Packet::Choke => peer_state.choking_us = true,
                            Packet::Unchoke => {
                                if peer_state.choking_us {
                                    peer_state.choking_us = false;
                                    
                                    // Now that we're able to download from this peer,
                                    // find the first unstarted / stalled piece we need that this peer has
                                    if let Some(piece_index) = flag_next_piece(&self.metainfo, &peer_state, &mut pieces_state) {
                                        request_next_block(piece_index, &self.metainfo, &peer_state, &mut pieces_state).await?;
                                    } else {
                                        // TODO: keep this peer open to see if they have a piece instead of closing immediately
                                        // if they don't?
                                        return Err(anyhow!("No pieces available to download from peer."));
                                    }
                                }
                            },
                            Packet::Interested => peer_state.interested_in_us = true,
                            Packet::NotInterested => peer_state.choking_us = false,
                            Packet::Have(_) => todo!(),
                            Packet::Bitfield(bitfield_packet) => {
                                peer_state.bitfield = BoolVec::from_vec(bitfield_packet.bitfield);
                            },
                            Packet::Request(_) => todo!(),
                            Packet::Piece(piece_packet) => {
                                let piece_index = piece_packet.index as usize;
                                if let PieceState::Downloading { block_index } = pieces_state[piece_index] {
                                    // request_next_block increments the block index in preparation for it 
                                    // downloading the next block.
                                    // As such, the block index we just received is the block index saved less one.
                                    let block_index = block_index - 1;
                                    
                                    // Write this piece out to disk
                                    // First, what file is this piece from?
                                    let block_torrent_offset = piece_index * (self.metainfo.piece_length as usize) + block_index * BLOCK_LENGTH as usize;

                                    let file_index = file_handles.iter_mut()
                                        .position(|f| (f.start + f.length) > block_torrent_offset)
                                        .ok_or(anyhow!("Piece index out of range for files provided (?)"))?;

                                    let f = &mut file_handles[file_index];
                                    let write_length = std::cmp::min(f.start + f.length - block_torrent_offset, piece_packet.block.len());

                                    f.handle.seek(SeekFrom::Start((block_torrent_offset - f.start) as u64)).await?;
                                    f.handle.write_all(&piece_packet.block[..write_length]).await?;

                                    if write_length < piece_packet.block.len() as usize && file_index + 1 < file_handles.len() {
                                        // This block stretches past the end of this file, and into the next
                                        let next_file = &mut file_handles[file_index + 1];
                                        next_file.handle.seek(SeekFrom::Start(0)).await?;
                                        next_file.handle.write_all(&piece_packet.block[write_length..]).await?;
                                    }

                                    // Is this piece finished?
                                    let piece_len = if piece_index == self.metainfo.pieces.len() - 1 {
                                        self.metainfo.total_length % self.metainfo.piece_length as usize
                                    } else {
                                        self.metainfo.piece_length as usize
                                    };

                                    let num_blocks = (piece_len - 1) / (BLOCK_LENGTH as usize) + 1;
                                    let next_piece_index = if block_index + 1 >= num_blocks {
                                        if let Some(next_piece_index) = flag_next_piece(&self.metainfo, &peer_state, &mut pieces_state) {
                                            next_piece_index
                                        } else {
                                            // TODO: keep this peer open to see if they have a piece instead of closing immediately
                                            // if they don't?
                                            return Err(anyhow!("No more pieces available to download from peer."));
                                        }
                                    } else {
                                        piece_index
                                    };

                                    request_next_block(next_piece_index, &self.metainfo, peer_state, &mut pieces_state).await?;

                                 } else {
                                    eprintln!("WARNING: received piece data for a block not currently being downloaded.");
                                }
                            },
                            Packet::Cancel(_) => todo!(),
                        };
                    };
                },

                peer_fut = peer_thread_futures.next() => {
                    match peer_fut {
                        Some(e) => eprintln!("{:?}", e),
                        None => {
                            println!("All peer threads ended, exiting (TODO: more peers!)");
                            break;
                        },
                    }
                }
            }
        };

        Ok(())
    }
}
