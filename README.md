# nix-cache-rs

A nix binary cache backed by Cloudflare Workers and R2.

## Introduction

Cachix provides nix binary caches for free (up to 5GB of storage), but the cache will be publicly available.

This project allows you to host a (mostly) cachix-compatible binary cache for almost nothing by relying on Cloudflare's generous free tier.
Common use-cases for this project are personal nixos configurations that you'd like to keep private.
It is not designed for very large binary caches.

The project relies on only 2 Cloudflare products: Workers (compute) and R2 (S3-compatible storage).

The free tier gives you:

* 100k requests / day
* 10GB-month
* 1M class-A operations (mostly write operations)
* 10M class-B operations (mostly read operations)

Depending on your usage, you are likely to only pay for storage which is $0.015 / GB-month above the 10GB mark.
So a 50GB cache (the entrylevel cachix plan) would cost you US$2 per month.

Refer to Cloudflare's [pricing for more details](https://www.cloudflare.com/plans/) and use their [R2 calculator](https://r2-calculator.cloudflare.com/) to estimate your own costs.

NOTE: the author and this project are not responsible for the user's Cloudflare fees.
Using this will likely cause you to incur Cloudflare fees, these are your own responsbility.
This project makes no guarantees. Use at your own risks.

## Getting Started

### Pre-requisites

You'll need a cloudflare acount. You can crate one [here](https://www.cloudflare.com/sign-up).

1. Install [`wrangler`](https://developers.cloudflare.com/workers/wrangler/install-and-update/).

2. Login to Cloudflare

```
wrangler login
```

3. Create an R2 bucket

We'll name the bucket `nix-cache`, but you can use a different name. We'll need to refer to this later on.

```
wrangler r2 bucket create nix-cache
```

4. Set object lifecycle (recommended)

This step is optional, but is recommended to have evict cached binaries after a certain time.
Without this, the cahe will grow unbounded.

```
wrangler r2 bucket lifecycle add nix-cache cache-eviction --expire-days 30 --abort-multipart-days 1
```

This policy will automatically delete cached files after 30 days.
It will also automatically unfinished multipart uploads (used by the cachix client) after 1 day.

5. Create `wrangler.toml`

TODO

  * variables
      * `bucket_name`
  * secrets: `["R2_ENDPOINT", "R2_ACCESS_KEY_ID", "R2_SECRET_ACCESS_KEY", "AUTH_TOKEN", "SIGNING_PUBLIC_KEY", "SIGNING_PRIVATE_KEY"]`
  * bucket binding

6. Create Secrets in `.env`

TODO

  * `R2_ACCESS_KEY_ID`
  * `R2_SECRET_ACCESS_KEY`
  * `R2_ENDPOINT`
  * `AUTH_TOKEN`
  * `SIGNING_PUBLIC_KEY`
  * `SINGNING_PRIVATE_KEY`

7. Deploy!

```
wrangler deploy --secrets-file .env`
```

8. Confirm it is working

```
set -a && source .env && curl https://:${AUTH_TOKEN}@worker-url.dev/nix-cache-info
```

## using `cachix`

### Locally

Cachix supports multiple caches, but this project only supports a single one, so use `default` as its name.

```
set -a && source .env && CACHIX_AUTH_TOKEN=${AUTH_TOKEN} cachix --hostname=https://worker-url.dev push default $(nix path-info nixpkgs#hello)
```

### In Github Actions

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

## dependabot

Put the auth token in both Secrets & Variables / Actions and Secrets & Variables / Dependabot

Or use `pull_request_target`