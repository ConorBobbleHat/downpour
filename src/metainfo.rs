use std::{path::Path};
use url::Url;
use anyhow::{anyhow, Result};

use crate::bencode::{BencodeValue, self};

type Sha1Hash = [u8; 20];

#[derive(Debug)]
pub struct SingleFileInfo {
    name: String,
    length: u64,
}

#[derive(Debug)]
pub struct DirectoryFileInfo {
    path: Vec<String>,
    length: u64,
}

#[derive(Debug)]
pub struct DirectoryInfo {
    name: String,
    files: Vec<DirectoryFileInfo>,
}

#[derive(Debug)]
pub enum Info {
    SingleFile(SingleFileInfo),
    Directory(DirectoryInfo),
}

#[derive(Debug)]
pub struct Metainfo {
    announce: Url,
    piece_length: u64,
    pieces: Vec<Sha1Hash>,
    info: Info,
}

impl Metainfo {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let (_, val) = bencode::parse_bencode(&bytes)
                                            .map_err(|x| { anyhow!("Bencode parse error") })?;
        Self::from_bencode(val)
    }

    pub fn from_bencode(val: BencodeValue) -> Result<Self> {
        // The root-level element of a metainfo file needs to be a dictionary
        let root = val.as_dict()?;

        let announce_string = root.get("announce".as_bytes())
            .ok_or(anyhow!("Invalid metainfo file: no announce URL"))?
            .as_str()?;

        let announce = Url::parse(announce_string)?;

        let info_dict = root.get("info".as_bytes())
            .ok_or(anyhow!("Invalid metainfo file: no info dict"))?
            .as_dict()?;

        let name = info_dict.get("name".as_bytes())
            .ok_or(anyhow!("Invalid info dict: no name"))?
            .as_str()?;

        let piece_length: u64 = info_dict.get("piece length".as_bytes())
            .ok_or(anyhow!("Invalid info dict: no piece length"))?
            .as_integer()?
            .try_into()?;
        
        let pieces_bytestring  = info_dict.get("pieces".as_bytes())
            .ok_or(anyhow!("Invalid info dict: no pieces bytestring"))?
            .as_bytes()?;

        let pieces_slices: Vec<&[u8]> = pieces_bytestring.chunks(20).collect();

        let pieces = pieces_slices.iter()
            .cloned()
            .map(|x| x.try_into().map_err(|y| anyhow!("Failed to convert slice to array")))
            .collect::<Result<Vec<Sha1Hash>>>()?;
        
        // Is this a single file, or are we dealing with a whole-directory torrent?
        let info = if let Some(BencodeValue::List(files_list)) = info_dict.get("files".as_bytes()) {
            // Whole-directory torrent
            let files: Vec<DirectoryFileInfo> = files_list.iter()
                .map(|file_val| {
                    let file_dict = file_val.as_dict()?;
                    let file_length: u64 = file_dict.get("length".as_bytes())
                        .ok_or(anyhow!("Invalid file entry: no length"))?
                        .as_integer()?
                        .try_into()?;

                    let file_path = file_dict.get("path".as_bytes())
                        .ok_or(anyhow!("Invalid file entry: no path"))?
                        .as_list()?
                        .iter()
                        .map(|x| Ok(x.as_str()?.to_string()))
                        .collect::<Result<Vec<String>>>()?;

                    Ok((DirectoryFileInfo {
                        path: file_path,
                        length: file_length,
                    }))
                })
                .collect::<Result<Vec<DirectoryFileInfo>>>()?;
            
                Info::Directory(DirectoryInfo {
                    name: name.to_string(),
                    files: files,
                })

        } else {
            // Single file torrent
            let length: u64 = info_dict.get("length".as_bytes())
                .ok_or(anyhow!("Invalid single-file torrent: no length"))?
                .as_integer()?
                .try_into()?;

            Info::SingleFile(SingleFileInfo {
                name: name.to_string(),
                length,
            })
        };

        Ok(Self {
            announce,
            piece_length,
            pieces,
            info,
        })
    }
}