# downpour
### A toy BitTorrent client written in Rust

Downpour is a very bare-bones BitTorrent client, built as an excuse to get to grips with everyone's favourite distributed file sharing protocol. It currently contains enough functionality to complete downloading a torrent.

## Installation & Usage
```
git clone https://github.com/ConorBobbleHat/downpour
cd downpour
```
```
$ cargo run -- --help
downpour 0.1.0
A toy BitTorrent client written in Rust

USAGE:
    downpour.exe [OPTIONS] <METAINFO_FILE> <DOWNLOAD_DIR>

ARGS:
    <METAINFO_FILE>    Path to the metainfo of the torrent to be downloaded
    <DOWNLOAD_DIR>     The output directory for the downloaded torrent

OPTIONS:
    -a, --active-peers <ACTIVE_PEERS>
            The maximum number of active connections with peers held open simultaneously [default: 8]

    -h, --help
            Print help information

    -p, --port <PORT>
            Port reported to trackers as our incoming traffic port. Not currently used [default: 6881]

    -t, --timeout <TIMEOUT>
            Timeout (in seconds) for network-related operations [default: 2]

    -u, --peer-update-interval <PEER_UPDATE_INTERVAL>
            The interval (in seconds) at which new active peers are selected to fill any vacancies
            [default: 5]

    -V, --version
            Print version information
```

## TODO
* Check the SHA-1 hash of pieces we receive to ensure the torrent's integrity
* Respond to requests for pieces from other peers
* Reannounce ourselves to trackers periodically & refresh the peer list

## License
[MIT](https://github.com/ConorBobbleHat/downpour/blob/main/LICENSE.md)