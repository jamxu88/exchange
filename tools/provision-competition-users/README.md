# Provision competition users

Provisions 200 competition traders (`Team 1` .. `Team 200`) through the admin
REST endpoint and writes two batch files of 100 keys each:

- `allocated-api-keys-batch-1.txt` — `Team 1` .. `Team 100`
- `allocated-api-keys-batch-2.txt` — `Team 101` .. `Team 200`

Each file is comma-separated (CSV) with the header `identifier,api_key` and one
row per team. The identifier column uses the format `Team #<n>`, matching the
handout label; the `api_key` column is the 7-character alphanumeric key returned by the
exchange.

## Usage

```bash
EXCHANGE_URL="https://exchange.jamesxu.dev" \
ADMIN_API_TOKEN="<admin bearer token>" \
node tools/provision-competition-users/provision.mjs
```

Optional environment variables:

- `TEAM_COUNT` — total teams to provision. Defaults to `200`.
- `BATCH_SIZE` — teams per file. Defaults to `100`.
- `OUTPUT_DIR` — directory to write the batch files. Defaults to the repo root.
- `TEAM_PREFIX` — username prefix. Defaults to `Team `.
- `IDENTIFIER_PREFIX` — identifier prefix. Defaults to `Team #`.
