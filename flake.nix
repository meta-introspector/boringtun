{
  description = "meta-introspector boringtun — WireGuard in Rust + WASM";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
    rust = pkgs.rust-bin.stable.latest.default.override {
      targets = [ "wasm32-wasip1" ];
    };
  in {
    devShells.${system}.default = pkgs.mkShell {
      buildInputs = [ rust pkgs.pkg-config pkgs.openssl ];
    };
  };
}
