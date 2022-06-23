use std::path::Path;
use sha1::{Sha1, Digest};
use reqwest::Url;
use anyhow::{anyhow, Result};

use crate::bencode::{BencodeValue, self};

pub type Sha1Hash = [u8; 20];

#[derive(Debug)]
pub struct SingleFileInfo {
    pub name: String,
    pub length: u64,
}

#[derive(Debug)]
pub struct DirectoryFileInfo {
    pub path: Vec<String>,
    pub length: u64,
}

#[derive(Debug)]
pub struct DirectoryInfo {
    pub name: String,
    pub files: Vec<DirectoryFileInfo>,
}

#[derive(Debug)]
pub enum Info {
    SingleFile(SingleFileInfo),
    Directory(DirectoryInfo),
}

#[derive(Debug)]
pub struct Metainfo {
    pub announce_list: Vec<Url>,
    pub piece_length: u64,
    pub pieces: Vec<Sha1Hash>,
    pub info: Info,
    pub info_hash: Sha1Hash,
}

impl Metainfo {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let (_, val) = bencode::parse_bencode(&bytes)
                                            .map_err(|_| { anyhow!("Bencode parse error") })?;

        // The root-level element of a metainfo file needs to be a dictionary
        let root = val.as_dict()?;

        let announce_list = if let Some(BencodeValue::List(announce_list)) = root.get("announce-list".as_bytes()) {
            // TODO: rewrite this using iter chains
            let mut announce_urls = Vec::new();
            
            for tier_list in announce_list {
                // TODO: the trackers in a tier are meant to be shuffled randomly.
                for announce_val in tier_list.as_list()? {
                    announce_urls.push(Url::parse(announce_val.as_str()?)?);
                };
            };

            announce_urls
        } else {
            let announce_string = root.get("announce".as_bytes())
                .ok_or_else(|| anyhow!("Invalid metainfo file: no announce URL"))?
                .as_str()?;
            
            vec![Url::parse(announce_string)?]
        };

        let info_dict = root.get("info".as_bytes())
            .ok_or_else(|| anyhow!("Invalid metainfo file: no info dict"))?
            .as_dict()?;

        let name = info_dict.get("name".as_bytes())
            .ok_or_else(|| anyhow!("Invalid info dict: no name"))?
            .as_str()?;

        let piece_length: u64 = info_dict.get("piece length".as_bytes())
            .ok_or_else(|| anyhow!("Invalid info dict: no piece length"))?
            .as_integer()?
            .try_into()?;
        
        let pieces_bytestring  = info_dict.get("pieces".as_bytes())
            .ok_or_else(|| anyhow!("Invalid info dict: no pieces bytestring"))?
            .as_bytes()?;

        let pieces_slices: Vec<&[u8]> = pieces_bytestring.chunks(20).collect();

        let pieces = pieces_slices.iter()
            .cloned()
            .map(|x| x.try_into().map_err(|_| anyhow!("Failed to convert slice to array")))
            .collect::<Result<Vec<Sha1Hash>>>()?;
        
        // Is this a single file, or are we dealing with a whole-directory torrent?
        let info = if let Some(BencodeValue::List(files_list)) = info_dict.get("files".as_bytes()) {
            // Whole-directory torrent
            let files: Vec<DirectoryFileInfo> = files_list.iter()
                .map(|file_val| {
                    let file_dict = file_val.as_dict()?;
                    let file_length: u64 = file_dict.get("length".as_bytes())
                        .ok_or_else(|| anyhow!("Invalid file entry: no length"))?
                        .as_integer()?
                        .try_into()?;

                    let file_path = file_dict.get("path".as_bytes())
                        .ok_or_else(|| anyhow!("Invalid file entry: no path"))?
                        .as_list()?
                        .iter()
                        .map(|x| Ok(x.as_str()?.to_string()))
                        .collect::<Result<Vec<String>>>()?;

                    Ok(DirectoryFileInfo {
                        path: file_path,
                        length: file_length,
                    })
                })
                .collect::<Result<Vec<DirectoryFileInfo>>>()?;
            
                Info::Directory(DirectoryInfo {
                    name: name.to_string(),
                    files,
                })

        } else {
            // Single file torrent
            let length: u64 = info_dict.get("length".as_bytes())
                .ok_or_else(|| anyhow!("Invalid single-file torrent: no length"))?
                .as_integer()?
                .try_into()?;

            Info::SingleFile(SingleFileInfo {
                name: name.to_string(),
                length,
            })
        };

        // Calculate the torrent's info_hash as the SHA1 hash of the raw bytes of the info dictionary
        let (_, info_dict_bytes) = bencode::parse_info_dict_raw(&bytes)
            .map_err(|_| anyhow!("Could not get the raw bytes of the info dict"))?;

        let mut hasher = Sha1::new();
        hasher.update(info_dict_bytes);
        let info_hash: Sha1Hash = hasher.finalize()[..].try_into()?;

        Ok(Self {
            announce_list,
            piece_length,
            pieces,
            info,
            info_hash
        })
    }
}