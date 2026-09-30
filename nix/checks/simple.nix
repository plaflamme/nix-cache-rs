{
  pkgs,
  nix-cache-rs,
  ...
}:
let
  lib = pkgs.lib;
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
  test-store-paths = [
    pkgs.bash
    pkgs.curl
  ];
in
pkgs.testers.runNixOSTest {
  imports = [
    {
      name = "simple";
      nodes = {
        node1 =
          { ... }:
          {
            nix = {
              settings = {
                extra-substituters = [ "http://localhost:8787" ];
                extra-trusted-public-keys = [ public-key ];
              };

              extraOptions = ''
                experimental-features = nix-command flakes
              '';
            };

            environment.etc."wrangler.toml".source = wrangler-toml;

            system.extraDependencies = test-store-paths;

            environment.variables = {
              CACHIX_AUTH_TOKEN = mock-auth-token;
            };

            systemd.services = {
              "nix-cache-rs" = {
                wants = [ "network-online.target" ];
                wantedBy = [ "multi-user.target" ];
                serviceConfig = {
                  ExecStart = "${pkgs.wrangler}/bin/wrangler dev --local --log-level=debug -c /etc/wrangler.toml";
                };
              };
            };
          };
      };

      testScript =
        { ... }:
        ''
          start_all()
          node1.wait_for_unit("nix-cache-rs")
        ''
        + lib.concatLines (
          lib.map (pkg: ''
            node1.succeed("${pkgs.cachix}/bin/cachix --hostname=http://localhost:8787 push default ${pkg}")
            node1.succeed("nix copy --from http://:${mock-auth-token}@localhost:8787 --to /tmp/store ${pkg}")
          '') test-store-paths
        );
    }
  ];
  node = {
    pkgsReadOnly = false;
  };
  defaults = {
    imports = [ ];
    nixpkgs.overlays = [
    ];
  };
}
