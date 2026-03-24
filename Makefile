.PHONY: build wasm clean

build:
	nix develop --command cargo build --release

wasm:
	nix develop --command cargo build --release --target wasm32-wasip1 -p boringtun --no-default-features

clean:
	nix develop --command cargo clean
