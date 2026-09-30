{
  nixConfig = {
    extra-substituters = [ "https://nix-cache-rs.cachix.org" ];
    extra-trusted-public-keys = [
      "nix-cache-rs.cachix.org-1:97m0A/0Fm2/8TeaytfiiGQIbQdfmCtdN+niDjX+ogtE="
    ];
  };
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs =
    {
      self,
      nixpkgs,
      flake-parts,
      rust-overlay,
      crane,
      ...
    }@inputs:
    flake-parts.lib.mkFlake { inherit inputs; } (
      { ... }: {
        imports = [
        ];
        systems = [
          "x86_64-linux"
          "aarch64-linux"
          "aarch64-darwin"
        ];
        perSystem =
          {
            self',
            pkgs,
            system,
            lib,
            ...
          }:
          {
            packages =
              let
                localPkgs = import nixpkgs {
                  inherit system;
                  overlays = [ (import rust-overlay) ];
                };
                rustToolchain = localPkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
                craneLib = (crane.mkLib localPkgs).overrideToolchain rustToolchain;

                src = pkgs.lib.fileset.toSource {
                  root = ./.;
                  fileset = pkgs.lib.fileset.unions [
                    (craneLib.fileset.commonCargoSources ./.)
                    # keep all files under ./tests
                    ./tests
                  ];
                };
                nix-cache-rs = self'.packages.worker;
              in
              {
                worker = craneLib.buildPackage {
                  inherit src;

                  cargoArtifacts = craneLib.buildDepsOnly {
                    inherit src;
                    CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
                  };

                  doCheck = false;

                  nativeBuildInputs = [
                    pkgs.worker-build
                    pkgs.wasm-bindgen-cli
                    pkgs.binaryen
                    pkgs.esbuild
                  ];

                  HOME = "\$TMPDIR";
                  CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
                  WASM_BINDGEN_BIN = "${pkgs.wasm-bindgen-cli}/bin/wasm-bindgen";
                  WASM_OPT_BIN = "${pkgs.binaryen}/bin/wasm-opt";
                  ESBUILD_BIN = "${pkgs.esbuild}/bin/esbuild";

                  cargoBuildCommand = "worker-build --release";

                  installPhase = ''
                    mkdir -p $out
                    cp -r build/* $out/
                  '';
                };
                default = import ./nix/cli.nix { inherit pkgs nix-cache-rs; };
              };

            checks =
              let
                nix-cache-rs = self'.packages.worker;
              in
              { }
              // lib.optionalAttrs (system == "x86_64-linux") {
                simple = import ./nix/checks/simple.nix { inherit pkgs nix-cache-rs; };
              };

            devShells = {
              default =
                let
                  cachix-proxied = pkgs.writeShellScriptBin "cachix-proxied" ''
                    export http_proxy="http://127.0.0.1:8080"
                    export https_proxy="http://127.0.0.1:8080"
                    export SSL_CERT_FILE="$HOME/.mitmproxy/mitmproxy-ca-cert.pem"
                    exec ${pkgs.cachix}/bin/cachix "$@"
                  '';

                  mock-auth-token = "mock-auth-token";
                in
                pkgs.mkShell {
                  nativeBuildInputs = [
                    pkgs.wrangler
                    pkgs.worker-build
                    cachix-proxied
                    pkgs.cachix
                    pkgs.mitmproxy
                  ];

                  MOCK_AUTH_TOKEN = "${mock-auth-token}";

                  shellHook = ''
                    TARGET_FILE=".env.local"
                    cat << EOF > "$TARGET_FILE"
                    # auto-generated, see flake.nix
                    AUTH_TOKEN=${mock-auth-token}
                    R2_ACCESS_KEY_ID=mock-local-key
                    R2_SECRET_ACCESS_KEY=mock-local-secret
                    R2_ENDPOINT=http://localhost:8787/cdn-cgi/local/r2/s3
                    SIGNING_PUBLIC_KEY=''$(cat tests/cache.example.com-1.pk)
                    SIGNING_PRIVATE_KEY=''$(cat tests/cache.example.com-1.sk)
                    EOF
                  '';
                };
            };
          };
      }
    );

}
