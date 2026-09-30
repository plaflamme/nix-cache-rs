{ pkgs, nix-cache-rs, ... }:
pkgs.writeShellApplication {
  name = "nix-cache-rs";
  runtimeInputs = [
    pkgs.wrangler
    pkgs.pwgen
  ];
  text = ''
    show_help() {
        echo "Usage: nix-cache-rs <command> [options]"
        echo ""
        echo "Commands:"
        echo "  login     Authenticate to your Cloudflare account"
        echo "  provision Provision the R2 bucket"
        echo "  deploy    Deploy or update the worker"
        echo "  help      Show this help message"
    }

    # If no arguments are provided, show help
    if [ $# -eq 0 ]; then
        show_help
        exit 1
    fi

    open_browser() {
      local url="$1"
      case "$(uname -s)" in
        Darwin)  open "$url" ;;
        Linux)   xdg-open "$url" ;;
        *)       echo "Unsupported OS" >&2; exit 1 ;;
      esac
    }

    write_wrangler_toml() {
      WORKER_NAME=$1
      BUCKET_NAME=$2
      wrangler_toml_dir=$(mktemp -d)

      cat << EOF > "$wrangler_toml_dir/wrangler.toml"
      name = "$WORKER_NAME"
      main = "${nix-cache-rs}/worker/shim.mjs"
      compatibility_date = "2026-08-01"
      workers_dev = true
      preview_urls = true

      [vars]
      bucket_name="$BUCKET_NAME"

      [secrets]
      required = [
          "R2_ENDPOINT",
          "R2_ACCESS_KEY_ID",
          "R2_SECRET_ACCESS_KEY",
          "AUTH_TOKEN",
          "SIGNING_PUBLIC_KEY",
          "SIGNING_PRIVATE_KEY",
      ]

      [[r2_buckets]]
      bucket_name = "$BUCKET_NAME"
      binding = "nix-cache-bucket"
    EOF
      echo "$wrangler_toml_dir"
    }

    provision() {
      WORKER_NAME="nix-cache-rs"
      BUCKET_NAME="nix-cache"
      RETENTION_DAYS="30"

      while getopts "w:b:r:h" opt; do
          case "''${opt}" in
              w) WORKER_NAME="''${OPTARG}" ;;
              b) BUCKET_NAME="''${OPTARG}" ;;
              r) RETENTION_DAYS="''${OPTARG}" ;;
              h)
                echo "Usage: nix-cache-rs provision [-w <worker name>] [-b <bucket name>] [-r <retention days>] [-h]"
                echo ""
                echo "Provisions the R2 bucket using <bucket name> (default 'nix-cache') and applies a lifecycle rule on the objects to evict them after <retention days> (default 30)."
                exit 1
                ;;
              *) exit 1 ;;
          esac
      done

      read -rp "Confirm you with to provision an R2 bucket named $BUCKET_NAME and a worker $WORKER_NAME"

      wrangler_toml_dir=$(write_wrangler_toml "$WORKER_NAME" "$BUCKET_NAME")

      # TODO: verify only one account
      cf_account_id=$(wrangler whoami --json | jq -r '.accounts[0].id')

      wrangler r2 bucket create "$BUCKET_NAME"
      wrangler r2 bucket lifecycle add "$BUCKET_NAME" --id "binary-cache-retention" --expire-days="$RETENTION_DAYS" --abort-multipart-days 1 --force

      api_key_url="https://dash.cloudflare.com/$cf_account_id/r2/api-tokens"

      echo ""
      echo "An R2 API token must be provisioned manually."
      echo "Use 'Create Account API Token' and select 'Object Read & Write'. Limit the token to the bucket $BUCKET_NAME."
      echo "Your browser should have opened $api_key_url to create the token."
      echo "Once created, leave the page opened to copy the S3 crentials and paste them here."
      echo ""
      
      open_browser "$api_key_url"

      read -rsp "Paste the 'Access Key ID': " access_key_id
      echo ""
      read -rsp "Paste the 'Secret Access Key': " secret_access_key
      echo ""

      private_key=$(nix key generate-secret --key-name "$WORKER_NAME")
      public_key=$(echo "$private_key" | nix key convert-secret-to-public)
      auth_token=$(pwgen -1 -n 32)

      cat << EOF | wrangler --cwd "$wrangler_toml_dir" secret bulk --name "$WORKER_NAME"
      R2_ENDPOINT=https://$cf_account_id.r2.cloudflarestorage.com
      R2_ACCESS_KEY_ID=$access_key_id
      R2_SECRET_ACCESS_KEY=$secret_access_key
      AUTH_TOKEN=$auth_token
      SIGNING_PUBLIC_KEY=$public_key
      SIGNING_PRIVATE_KEY=$private_key
    EOF

      echo "Your nix binary cache Cloudflare infrastructure has been provisioned."
      echo ""
      echo "Public key: $public_key"
      echo "Auth token: $auth_token"
      echo ""
      echo "You may replace the auth token with the following command:"
      echo "wrangler secret put --name $WORKER_NAME AUTH_TOKEN"
      echo ""
      echo "Run the following command to finalize the configuration and obtain the cache's URL."
      echo ""
      echo "deploy -w $WORKER_NAME -b $BUCKET_NAME
    }

    deploy() {
      WORKER_NAME="nix-cache-rs"
      BUCKET_NAME="nix-cache"

      while getopts "w:b:r:h" opt; do
          case "''${opt}" in
              w) WORKER_NAME="''${OPTARG}" ;;
              b) BUCKET_NAME="''${OPTARG}" ;;
              h)
                echo "Usage: nix-cache-rs deploy [-w <worker name>] [-b <bucket name>] [-h]"
                echo ""
                echo "Deploy (or update) the worker."
                exit 1
                ;;
              *) exit 1 ;;
          esac
      done

      # TODO: validate the existence of WORKER_NAME and BUCKET_NAME

      wrangler_toml_dir=$(write_wrangler_toml "$WORKER_NAME" "$BUCKET_NAME")
      wrangler --cwd "$wrangler_toml_dir" deploy
    }

    COMMAND="$1"
    shift

    case "$COMMAND" in
        login)
            wrangler login
            ;;
            
        provision)
            provision "$@"
            ;;
        deploy)
            deploy "$@"
            ;;

        help|-h|--help)
            show_help
            ;;
            
        *)
            echo "Error: Unknown command '$COMMAND'"
            show_help
            exit 1
            ;;
    esac
  '';
}
