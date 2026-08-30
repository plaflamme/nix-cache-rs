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
        perSystem =
          { pkgs, ... }:
          let
            cachix-proxied = pkgs.writeShellScriptBin "cachix-proxied" ''
              export http_proxy="http://127.0.0.1:8080"
              export https_proxy="http://127.0.0.1:8080"
              export SSL_CERT_FILE="$HOME/.mitmproxy/mitmproxy-ca-cert.pem"
              exec ${pkgs.cachix}/bin/cachix "$@"
            '';

          in
          {
            devShells = {
              default = pkgs.mkShell {
                nativeBuildInputs = [
                  pkgs.wrangler
                  pkgs.worker-build
                  cachix-proxied
                  pkgs.cachix
                  pkgs.mitmproxy
                ];
              };
            };
          };
      }
    );

}
