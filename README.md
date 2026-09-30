# nix-cache-rs

A nix binary cache backed by Cloudflare Workers and R2.

## Introduction

This project allows you to host a (mostly) cachix-compatible binary cache at low cost by relying on Cloudflare's generous free tier.
Typical use-case for this project is hosting binaries for personal nixos configurations that you'd like to keep private.
It is not designed for large binary caches. Consider using [Cachix](https://www.cachix.org/) for anything else.

The project relies on only 2 Cloudflare products: Workers (compute) and R2 (S3-compatible storage).

The free tier gives you:

* 100k requests / day
* 10GB-month
* 1M class-A operations (mostly write operations)
* 10M class-B operations (mostly read operations)

Depending on your usage, you are likely to only pay for storage which is US$0.015 / GB-month above the 10GB mark.
So a cache holding 50GB of binaries for a month would cost you US$0.60.

Refer to Cloudflare's [pricing for more details](https://www.cloudflare.com/plans/) and use their [R2 calculator](https://r2-calculator.cloudflare.com/) to estimate your own costs.

⚠️ **Cloudflare Cost Disclaimer** ⚠️

This tool relies on paid cloud infrastructure resources provided by Cloudflare.
You are solely responsible for any fees, bills, or charges incurred by running this project.
The authors and contributors are not responsible for any unexpected expenses.

## Getting Started

### Pre-requisites

You'll need a Cloudflare account, if you don't already have one, you can create one [here](https://www.cloudflare.com/sign-up).

Once available, login to the account using the provided cli:

```
nix run github:plaflamme/nix-cache-rs -- login
```

This will execute an OAuth dance to allow using the `wrangler` command line locally.

### Provision

Use the script provided to provision the required R2 bucket and worker

```
nix run github:plaflamme/nix-cache-rs -- provision -w nix-cache-worker -b nix-cache-bucket
```

The authentication token and the public signing key will be printed on the console, these are required to actually use the cache.
See the sections below to learn where to place these values.

### Deploy

Once provisioning is complete, you may deploy the latest worker code with the following command:

```
nix run github:plaflamme/nix-cache-rs -- deploy -w nix-cache-worker -b nix-cache-bucket
```

This will print the `workers.dev` url you may use to invoke the worker.

Updating the worker can be done using the same command.

### Test

To confirm the setup is complete

```
curl https://:${AUTH_TOKEN}@nix-cache-rs-worker.foo-bar.workers.dev/nix-cache-info
```

## using `cachix`

### Locally

Cachix supports multiple caches, but this project only supports a single one, so use `default` as its name.

```
CACHIX_AUTH_TOKEN=${AUTH_TOKEN} cachix --hostname=https://nix-cache-rs-worker.foo-bar.workers.dev push default $(nix path-info nixpkgs#hello)
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

## dependabot

If you are using Dependabot, the secrets will also have to be stored under `Secrets & Variables / Dependabot` to make them visible to workflows ran from PRs it opens.

Alternatively, you may use `on: pull_request_target` instead of `pull_request`.

## using with NixOS

Create a `/etc/nix/netrc` file with the following contents:

```
machine <worker_hostname>
  login ""
  password <auth_token>
```

Add the cache as a substituter to your nix settings:

```
{ ... }: {
  nix = {
    settings = {
      extra-substituters = [
        "https://nix-cache-rs-worker.foo-bar.workers.dev"
      ];
      extra-trusted-public-keys = [
        "nix-cache-rs-worker-1:NnFFOMVXxfXu7hA/WiSPk+6Dt3KOnqqD0Y1vWZgWP2s="
      ];
    };
  }
}
```

Similarly in your `flake.nix`:

```
{
  nixConfig = {
    extra-substituters = [
        "https://nix-cache-rs-worker.foo-bar.workers.dev"
    ];
    extra-trusted-public-keys = [
      "nix-cache-rs-worker-1:NnFFOMVXxfXu7hA/WiSPk+6Dt3KOnqqD0Y1vWZgWP2s="
    ];
  };

  inputs = {
    ...
  };
}

```