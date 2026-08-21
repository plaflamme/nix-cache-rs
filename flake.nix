{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };
  outputs =
    { flake-parts, ... }@inputs:
    flake-parts.lib.mkFlake { inherit inputs; } (
      { ... }: {
        imports = [
        ];
        systems = [
          "x86_64-linux"
          "aarch64-darwin"
        ];
        perSystem = { pkgs, ... }: {
          devShells = {
            default = pkgs.mkShell {
              nativeBuildInputs = [
                pkgs.wrangler
                pkgs.worker-build
                pkgs.cachix
              ];
            };
          };
        };
      }
    );

}
