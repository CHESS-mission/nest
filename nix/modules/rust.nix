{ inputs, ... }:
{
  imports = [
    inputs.rust-flake.flakeModules.default
    inputs.rust-flake.flakeModules.nixpkgs
  ];

  perSystem =
    { config
    , self'
    , lib
    , ...
    }:
    {
      rust-project = {
        # Crate-specific Nix configuration
        crateNixFile = "crate.nix";

        src =
          let
            root = inputs.self;
            cargoSources = config.rust-project.crane-lib.filterCargoSources;
          in
          lib.cleanSourceWith {
            src = root;

            filter = path: type:
              cargoSources path type
              || lib.hasSuffix "/crate.nix" path
              || lib.hasPrefix "${root}/public/" path
              || lib.hasPrefix "${root}/style/" path
              || lib.hasPrefix "${root}/wit/" path;
          };
      };
    };
}
