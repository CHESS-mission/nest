{
  perSystem =
    { pkgs
    , self'
    , ...
    }:
    {
      devShells.default = pkgs.mkShell {
        name = "nest-devshell";

        inputsFrom = [
          self'.devShells.rust
        ];

        packages = with pkgs; [
          # Runtime and development tools
          qemu
          picocom
          socat

          # Documentation tooling
          uv
        ];
      };
    };
}
