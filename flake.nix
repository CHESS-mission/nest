{
  description = "NEST flake";

  inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url  = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
      in
      {
        devShells.default = with pkgs; mkShell {
          name = "NEST devShell";
          buildInputs = [
            llvmPackages.clang
            extism-cli
            (rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
          ];
          LIBCLANG_PATH = "${pkgs.llvmPackages_16.libclang.lib}/lib";
        };
      }
    );
}
