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

            wrangler-dev = pkgs.writeShellScriptBin "wrangler-dev" ''
              ${pkgs.wrangler}/bin/wrangler dev --var public_key:$(cat tests/cache.example.com-1.pk)
            '';

            mock-auth-token = "mock-auth-token";
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
                  wrangler-dev
                ];

                MOCK_AUTH_TOKEN = "${mock-auth-token}";

                shellHook = ''
                  TARGET_FILE=".env.local"
                  cat << 'EOF' > "$TARGET_FILE"
                  # auto-generated, see flake.nix
                  AUTH_TOKEN=${mock-auth-token}
                  R2_ACCESS_KEY_ID=mock-local-key
                  R2_SECRET_ACCESS_KEY=mock-local-secret
                  R2_ENDPOINT=http://localhost:8787/cdn-cgi/local/r2/s3
                  SIGNING_PRIVATE_KEY=$(cat tests/cache.example.com-1.sk)
                  EOF
                '';
              };
            };
          };
      }
    );

}
