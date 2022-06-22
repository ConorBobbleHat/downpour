use anyhow::Result;
use metainfo::Metainfo;

mod bencode;
mod metainfo;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    
    let metainfo = Metainfo::from_file(&args[1])?;
    println!("{:x?}", metainfo.info_hash);

    Ok(())
}
