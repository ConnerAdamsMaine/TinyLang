---
title: TIDE Deployment
---

# TIDE Deployment

TIDE is deployed as part of the existing Web application, not as a separate service. It requires the same PostgreSQL database and static Web assets. All SQLx migrations, including `0031_tide_investigation_attachments.sql`, run during Web startup.

## Local

From `Web`, configure a local `.env` based on `.env.example` with a PostgreSQL `DATABASE_URL`. Configure `PUBLIC_BASE_URL` and any provider credentials you want to enable. X uses `X_OAUTH_CLIENT_ID` / `X_OAUTH_CLIENT_SECRET`; register `${PUBLIC_BASE_URL}/api/auth/x/callback` and enable only the `users.read` scope.

Run:

```powershell
cargo run --manifest-path Cargo.toml
```

The server serves TIDE at `/tide.html`; health checks are `/health`, `/live`, and `/ready`. TIDE authorship requires at least one linked GitHub, Discord, or X identity. Users can sign in with an already linked provider, but provider sign-in never creates or links accounts. GitHub OAuth remains necessary only if GitHub-linked repository contribution synchronization is desired.

## Docker Compose

From `Web`, create `.env` using `.env.example`, set a strong `POSTGRES_PASSWORD`, `PUBLIC_BASE_URL`, and credentials for at least one supported provider. Docker Compose forwards GitHub, Discord, and X client credentials to the web container. For X, set `X_OAUTH_CLIENT_ID` and `X_OAUTH_CLIENT_SECRET`, register `${PUBLIC_BASE_URL}/api/auth/x/callback`, and enable the `users.read` scope. Then run:

```powershell
docker compose up --build -d
docker compose logs -f web
```

The web container applies database migrations on startup, including the current TIDE migrations. Existing sessions are treated as local sessions by the provider migration and must sign in again through GitHub, Discord, or X to use TIDE write operations. Private ticket attachments are stored in PostgreSQL and purged 90 days after tickets remain resolved; account for their storage and backup footprint. Support and investigator access is not inherited from admin or moderator roles. Designate accounts by inserting individually approved existing user IDs into `tide_support_agents` or `tide_investigators`, with the grantor recorded, using a controlled database session. Protect and audit these grants. Do not publish `.env` or provider credentials.

## Encrypted offsite database backups

The optional `tide-backup` Compose profile performs an immediate full PostgreSQL custom-format dump, encrypts the stream to an age recipient before it is written to disk, signs a manifest with minisign, and uploads the encrypted dump, manifest, and signature as separate immutable objects. It then repeats at the configured interval (24 hours by default). The S3-compatible bucket must have versioning and Object Lock enabled; each uploaded object is explicitly retained in COMPLIANCE mode for the configured period (90 days by default). S3 server-side encryption is also requested. The full database includes private support, chat, and moderation data, so age encryption is mandatory. The backup container receives the signing key but never the age decryption identity.

1. Create a private S3-compatible bucket with versioning and Object Lock enabled. Configure lifecycle/cost expectations: COMPLIANCE-retained objects cannot be deleted early, even by an administrator. Give the backup identity only bucket configuration read permissions and object-write permissions; use a separate restore identity with object-read permissions.
2. Generate and retain keys outside the Web deployment. For example, `age-keygen -o secrets/tide-backup-age-identity` creates the decryption identity and prints its public recipient. `minisign -G -W -s secrets/tide-backup-signing.key -p secrets/tide-backup-signing.pub` creates an unencrypted signing key; protect its file with host permissions/secret management and retain the public key separately. Never put private keys in Git or the S3 bucket. Keep an independent copy of the age identity and minisign public key so loss of the host does not prevent decrypting/verifying retained backups.
3. Set the `AWS_*` and `TIDE_BACKUP_*` values shown in `Web/.env.example`, including the S3 endpoint for a non-AWS service, recipient, and signing-key file path. The file is ignored by Git. Confirm the key files exist before enabling the profile.
4. Start the scheduled backup worker with `docker compose --profile backup up --build -d tide-backup`. Check `docker compose logs tide-backup` for successful upload and the backup ID. A failed run exits visibly and Compose restarts the worker. The staging file is encrypted; the only plaintext dump is streamed directly from `pg_dump` into age.
5. Periodically test recovery on a separate PostgreSQL instance or an empty disposable database named `tide_restore_test` (or `tide_restore_test_<suffix>`). Set `TIDE_BACKUP_ID` from the successful upload log and `TIDE_RESTORE_DATABASE_URL`, mount the offline age identity and signing public key, and run `docker compose --profile restore run --rm tide-restore-test`. The restore utility verifies the manifest signature and encrypted-dump SHA-256, decrypts through a pipe, validates the archive, refuses non-test target names and nonempty targets, and performs a full `pg_restore`. Do not restore over production.

Backups are not a substitute for testing disaster recovery, monitoring failures, or retaining key material. Use an offsite bucket in a separate failure domain and periodically record/verify a successful restore before relying on it. A backup profile must be explicitly enabled; default Compose startup does not upload data.

For each grant, verify the recipient and grantor TIDE IDs before insertion. For example, grant investigator access to an existing account using its public ID while recording the authorizing account:

```sql
INSERT INTO tide_investigators (user_id, granted_by)
SELECT recipient.id, grantor.id
FROM users AS recipient
CROSS JOIN users AS grantor
WHERE recipient.tide_user_id = 'TIDE-U-XXXXXX'
  AND recipient.deleted_at IS NULL
  AND grantor.tide_user_id = 'TIDE-U-YYYYYY'
  AND grantor.deleted_at IS NULL;
```

Verify exactly one row was inserted. Use the same pattern with `tide_support_agents` to grant support access. Revoke access by deleting the individual grant row after a recorded authorization; preserve existing support and investigation data.

For automatic implementation attribution, configure a GitHub repository webhook over HTTPS at `${PUBLIC_BASE_URL}/api/v1/integrations/github/webhook`, select the `pull_request` event, and set the same high-entropy webhook secret in GitHub and `TIDE_GITHUB_WEBHOOK_SECRET` (for example, generate one with `openssl rand -hex 32`). The handler verifies GitHub's `X-Hub-Signature-256`, exact repository/base identity, merged state, and merge SHA. It scans the PR body for existing `TIDE-XXXXXX` forum IDs; each target receives an append-only `IMPLEMENT` event only if the PR author maps to an active linked GitHub identity with a developer or maintainer role. Configure the webhook only after TIDE is reachable at the public HTTPS URL. Other GitHub events and unmerged pull requests are acknowledged without ledger changes.

Production requires HTTPS, accurate `PUBLIC_BASE_URL`, secure cookies, appropriately configured trusted proxy hops, secret management, Postgres backups, and monitoring. The existing compose memory limit is a configuration value, not evidence of $20/month capacity. No TIDE load benchmark or live production-bucket recovery exercise has been performed.
