# nix-cache-rs

A nix binary cache backed by Cloudflare Workers and R2.

## Getting Started

* cloudflare account
* cloudflare api token
* wrangler install
* create R2 bucket
* create R2 object lifecycle
* create `wrangler.toml`
    * variables
        * `cache_hostname`
        * `bucket_name`
        * `github_username`
        * `public_key`
    * secrets: `["R2_ENDPOINT", "R2_ACCESS_KEY_ID", "R2_SECRET_ACCESS_KEY", "AUTH_TOKEN", "SIGNING_PRIVATE_KEY"]`
    * bucket binding
* create `.env` file
    * `R2_ACCESS_KEY_ID`
    * `R2_SECRET_ACCESS_KEY`
    * `R2_ENDPOINT`
    * `AUTH_TOKEN`
    * `SINGNING_PRIVATE_KEY`
*  `wrangler deploy --secrets-file .env`
*  update `cache_hostname`
*  `wrangler deploy --secrets-file .env`

That's it!

## using `cachix`

Add the following secrets:

* `NIX_CACHE_RS_CACHE_HOSTNAME`
* `NIX_CACHE_RS_AUTH_TOKEN`

```yaml
jobs:
  build:
    steps:
      - name: Create netrc file
        run: |
          echo 'machine ${{ secrets.NIX_CACHE_RS_CACHE_HOSTNAME }} login "" password ${{ secrets.NIX_CACHE_RS_AUTH_TOKEN }}' > /home/runner/.netrc
          chmod 600 /home/runner/.netrc
      - uses: cachix/install-nix-action@v31
        with:
          github_access_token: ${{ secrets.GITHUB_TOKEN }}
          nix_path: nixpkgs=channel:nixos-unstable
          extra_nix_config: |
            experimental-features = nix-command flakes
            extra-trusted-public-keys = ${{ secrets.NIX_CACHE_RS_CACHE_HOSTNAME }}:<public key>
            extra-substituters = https://${{ secrets.NIX_CACHE_RS_CACHE_HOSTNAME }}
            netrc-file = /home/runner/.netrc
      - uses: cachix/cachix-action@v17
        with:
          name: default
          authToken: ${{ secrets.NIX_CACHE_RS_AUTH_TOKEN }}
          skipAddingSubstituter: true
          cachixArgs: "--hostname ${{ secrets.NIX_CACHE_RS_CACHE_HOSTNAME }}"
      - run: nix flake check
```

## using with nix

Create a `/etc/nix/netrc` file with the following contents:

```
machine <cache_hostname>
  login ""
  password <auth_token>
```
