{
  description = "NEST workspace flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";

    # Only Linux is supported
    systems.url = "github:nix-systems/default-linux";

    rust-flake.url = "github:juspay/rust-flake";
    rust-flake.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = import inputs.systems;

      imports = [
        ./nix/modules/component-shells.nix
        ./nix/modules/devshell.nix
        ./nix/modules/rust.nix
      ];

      perSystem =
        { pkgs
        , self'
        , ...
        }:
        {
          formatter = pkgs.nixpkgs-fmt;
          packages.default = self'.packages.nest-server;
        };
    };
}
