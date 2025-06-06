init:
    mkdir -p local

download:
    cargo run download-file sample.torrent --output-path local/result.txt
