{ pkgs, nix-cache-rs, ... }:
let
  mock-auth-token = "mock-auth-token";
  bucket-name = "nix-cache";
  access-key-id = "mock-local-key";
  secret-access-key = "mock-local-secret";
  public-key = builtins.readFile ../../tests/cache.example.com-1.pk;
  tomlFormat = pkgs.formats.toml { };
  wrangler-toml = tomlFormat.generate "wrangler.toml" {
    name = "nix-cache-rs";
    main = "${nix-cache-rs}/worker/shim.mjs";
    compatibility_date = "2026-08-01";
    vars = {
      bucket_name = bucket-name;
      AUTH_TOKEN = mock-auth-token;
      R2_ACCESS_KEY_ID = access-key-id;
      R2_SECRET_ACCESS_KEY = secret-access-key;
      R2_ENDPOINT = "http://localhost:8787/cdn-cgi/local/r2/s3";
      SIGNING_PUBLIC_KEY = "${public-key}";
      SIGNING_PRIVATE_KEY = "${builtins.readFile ../../tests/cache.example.com-1.sk}";
    };
    r2_buckets = [
      {
        bucket_name = bucket-name;
        binding = "nix-cache-bucket";
        local_dev = {
          "experimental_s3_credentials" = {
            "accessKeyId" = access-key-id;
            "secretAccessKey" = secret-access-key;
          };
        };
      }
    ];
  };
in
pkgs.stdenv.mkDerivation {
  name = "simple";

  src = ./.;

  nativeBuildInputs = [
    pkgs.wrangler
    pkgs.curl
    pkgs.cachix
    pkgs.hello
    pkgs.nix
  ];

  buildPhase = ''
    export HOME=\$TMPDIR
    export CARGO_HOME=\$TMPDIR/.cargo
    export CACHIX_AUTH_TOKEN="${mock-auth-token}"

    export NIX_DATA_DIR=$TMPDIR/nix/share
    export NIX_LOG_DIR=$TMPDIR/nix/var/log/nix
    export NIX_STATE_DIR=$TMPDIR/nix/var/nix
    # https://github.com/cachix/cachix/pull/723
    # export NIX_STORE_DIR=$TMPDIR//nix/store
    export NIX_STORE_DIR=/nix/store

    STORE_PATH=$(nix-store --add ${pkgs.hello})

    cp ${wrangler-toml} wrangler.toml
    wrangler dev --local --log-level=debug &
    WRANGLER_PID=$!
    trap "kill $WRANGLER_PID 2>/dev/null || true" EXIT
    sleep 1

    curl http://:${mock-auth-token}@localhost:8787/nix-cache-info

    cachix -v --hostname http://localhost:8787 push default $STORE_PATH

    nix --extra-experimental-features nix-command \
      --offline \
      copy \
        --option extra-substituters "https://example.com" \
        --option extra-trusted-public-keys "${public-key}" \
        --verbose \
        --from http://:${mock-auth-token}@localhost:8787 \
        --to /tmp/store \
        $STORE_PATH
  '';

  installPhase = "touch \$out";
}
