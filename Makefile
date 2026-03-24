.PHONY: build wasm relay clean

build:
	nix develop --command cargo build --release -p boringtun-cli

wasm:
	nix develop --command cargo build --release --target wasm32-wasip1 -p boringtun-wasm

relay:
	nix develop --command cargo build --release -p wg-relay

serve: wasm relay
	cp target/wasm32-wasip1/release/boringtun_wasm.wasm web/
	cd web && python3 -m http.server 9720 &
	./target/release/wg-relay

clean:
	nix develop --command cargo clean
