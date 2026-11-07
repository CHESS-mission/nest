{ pkgs, ... }:
{
  autoWire = [
    "crate"
    "clippy"
    "doc"
  ];

  crane = {
    args = {
      nativeBuildInputs = with pkgs; [
        cargo-leptos
        wasm-bindgen-cli_0_2_130
        tailwindcss_4
        dart-sass
        binaryen

        # Install phase dependencies
        makeWrapper
      ];

      # Tests can be run manually with `nix flake check`
      doCheck = false;
    };

    extraBuildArgs = {
      buildPhaseCargoCommand = "cargo leptos build --release -vvv";

      # cargo-leptos does not produce the Cargo JSON build log expected by
      # Crane's automatic binary installation hook.
      doNotPostBuildInstallCargoBinaries = true;
      installPhaseCommand = ''
        runHook preInstall

        mkdir -p "$out/bin"
        mkdir -p "$out/share/nest"

        cp target/release/nest-server "$out/bin/.nest-server-unwrapped"
        cp -r target/site "$out/share/nest/site"

        makeWrapper "$out/bin/.nest-server-unwrapped" "$out/bin/nest-server" \
          --set LEPTOS_SITE_ROOT "$out/share/nest/site"

        runHook postInstall
      '';
    };
  };
}
