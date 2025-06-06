{
  description = "rusty-bittorrent-client - flake for devShell and build";

  # 1. Inputs: where do we get Nixpkgs, other flakes, helpers, etc.?
  inputs = {
    # pin to a specific Nixpkgs commit or channel
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = { nixpkgs.follows = "nixpkgs"; };
    };
  };

  # 2. Outputs: what this flake exposes (packages, devShells, apps, etc.)
  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile
          ./rust-toolchain.toml;
        nativeBuildInputs = with pkgs; [ rustToolchain pkg-config ];
        buildInputs = with pkgs; [ openssl just ];

      in with pkgs; {
        devShells = {
          default = mkShell {
            inherit buildInputs nativeBuildInputs;
            name = "dev-shell";
          };
        };
      });
}

