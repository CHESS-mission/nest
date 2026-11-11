# Development shells for authoring custom NEST WebAssembly components in several languages.
# These shells are for WebAssembly component projects, which live outside this workspace.

{ ... }:
{
  perSystem =
    { pkgs
    , self'
    , ...
    }:
    let
      # The WASI SDK is a self-contained clang plus WASI libc, distributed as release archives.
      wasiSdk = pkgs.fetchzip {
        url = "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-27/wasi-sdk-27.0-x86_64-linux.tar.gz";
        hash = "sha256-jujoJW6pj5RLAUXyn8Sk3tQrm6qwB8zQERd1Sd+cdYA=";
        stripRoot = false;
      };

      # verify.sh reads the manifest of a component and talks WebSocket to the server
      verifyDeps = [ pkgs.uv ];
    in
    {
      devShells.component-rust = pkgs.mkShell {
        name = "nest-component-rust";

        inputsFrom = [
          self'.devShells.rust
        ];

        packages = [
          pkgs.wasm-tools
          pkgs.wit-bindgen
        ] ++ verifyDeps;

        shellHook = ''
          echo "nest-component-rust: rust ($(rustc --version))"
          echo "targets available: $(ls $(rustc --print sysroot)/lib/rustlib | grep wasm | tr '\n' ' ')"
        '';
      };

      devShells.component-c = pkgs.mkShell {
        name = "nest-component-c";

        packages = [
          pkgs.cmake
          pkgs.ninja
          pkgs.wasm-tools
          pkgs.wit-bindgen
        ] ++ verifyDeps;

        WASI_SDK_PATH = "${wasiSdk}/wasi-sdk-27.0-x86_64-linux";

        shellHook = ''
          echo "nest-component-c: wasi-sdk 27, wit-bindgen, wasm-tools, cmake, ninja"
          echo "build with -DCMAKE_TOOLCHAIN_FILE=\$WASI_SDK_PATH/share/cmake/wasi-sdk-p2.cmake"
        '';
      };

      devShells.component-js = pkgs.mkShell {
        name = "nest-component-js";

        packages = [
          pkgs.nodejs_22
          pkgs.wasm-tools
        ] ++ verifyDeps;

        shellHook = ''
          echo "nest-component-js: node $(node --version) with jco, which build-component.sh"
          echo "fetches once into the project's node_modules"
        '';
      };

      devShells.component-python = pkgs.mkShell {
        name = "nest-component-python";

        packages = [
          pkgs.wasm-tools
        ] ++ verifyDeps;

        shellHook = ''
          echo "nest-component-python: run the tool through uv, e.g."
          echo "  uv run --python 3.12 --with componentize-py componentize-py --help"
        '';
      };
    };
}
