use std::error::Error;

mod bencode;
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let torrent_bytes = std::fs::read(&args[1])?;

    match bencode::parse_bencode(&torrent_bytes) {
        Ok((_, val)) => {
            if let bencode::BencodeValue::Dictionary(dict) = val {
                println!("{:#?}", dict);
            } else {
                panic!("Invalid torrent file!");
            }
        },
        Err(e) => panic!("{}", e),
    }

    Ok(())
}
