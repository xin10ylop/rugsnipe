# alpenglow_oracle

Generates `fixtures/alpenglow/solana_entry_4_3_1.json` with the real `solana-entry =4.3.1`
serializer, so `src/alpenglow.rs` is tested against Agave's own bytes without pulling Agave
crates into the workspace. Not a workspace member (rename `Cargo.toml.txt` to `Cargo.toml`
in a scratch copy):

    cp -r crates/pump-codec/scripts/alpenglow_oracle /tmp/ao && cd /tmp/ao && mv Cargo.toml.txt Cargo.toml
    RUSTUP_TOOLCHAIN=1.97.1 cargo run --release -- /path/to/crates/pump-codec/fixtures/tx

and convert the printed `name hex` lines into the JSON `vectors` map.
