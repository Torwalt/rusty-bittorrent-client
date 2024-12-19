# Bittorrent implementation (in parts) as per codecrafters

This project followed the steps from the codecrafters Bittorrent challenge
<https://app.codecrafters.io/courses/bittorrent/overview>

A full file can be downloaded with  
```bash
cargo run -- download -o $OUTPUT_PATH sample.torrent
```

The resulting file can be diff'd against the `golden-result` file.

I've implemented the download to run over all available Peers. Each downloaded
piece is streamed into the File at the correct index.

Run  
```bash
cargo run -- help
```

to see the other available commands!

The program will work so long as the codecrafters bittorrent is online.

## Thoughts

This project was insanely fun. Not only did I explore the bittorrent protocol
and bencode but also touched lower level topics such as working directly on a
TCP connection.

By doing the challenge with Rust I learned and understood a lot of concepts of the lang:

- ownership, borrowing, moving
- enums
- tokio async - tasks and channels
- testing
- lifetimes
- modules

Rust is such an awesome language. Once you are past a certain point, the
compiler becomes liberating and you are not working against it but with it.
I am looking forward building more stuff with Rust.

🦀 Rust <3 🦀

## Codecrafters

The branch codecrafters contains the additional files that are needed to run
this repo in the codecrafters test suite, if ever needed.

