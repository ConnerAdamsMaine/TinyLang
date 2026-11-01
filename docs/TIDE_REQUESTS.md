---
title: TIDE Requests
---

# TIDE Requests

Submit a new record from the [TIDE page](https://tl.404connernotfound.dev/tide.html). The current session must be established by GitHub, Discord, or X OAuth, and that provider must be linked to the account. A local-password session is insufficient even when a provider is linked. Choose one official category and a record type; the server assigns a permanent `TIDE-XXXXXX` identifier.

The original title, body, rationale, requirements, category, record type, author, timestamp, and SHA-256 content hash are stored in `tide_records`. Database triggers reject update and delete operations. Creation writes a `REQUEST` event in the same transaction. An optional UUID `Idempotency-Key` prevents a retry with the same author/key from creating a duplicate; reusing the key with different content conflicts. The event records the provider that established the current session and the account's linked provider types. It is a server-side attestation, not a personal cryptographic signature.

Requirements are stored as separate text array elements and displayed with stable references such as `TIDE-000142/R1`. Submit later governance events to `POST /api/v1/forum/requests/{public_id}/events` using an `event_type` and typed `payload`:

- `AMEND` adds content and optional rationale without changing the original.
- `RELATE` adds one of the supported record relationships.
- `DECIDE` records a maintainer decision and rationale. `Selected` requires a resolution record; `Satisfied` requires every request requirement reference.
- `IMPLEMENT` records a verified commit or pull-request reference and notes. Supply a full 40-character commit SHA or canonical TinyOne GitHub commit URL, and/or a positive pull-request number or canonical TinyOne GitHub pull-request URL. TIDE verifies each manually submitted reference against the configured TinyOne repository before appending the event and stores it as a canonical URL; an unavailable GitHub API fails the request rather than accepting unverified evidence. When configured, the signed GitHub pull-request webhook also appends an implementation event for each `TIDE-XXXXXX` marker in the body of a merged pull request, attributed to its linked author only if that account currently has developer or maintainer authorization. A delivery UUID and payload hash make webhook retries idempotent.
- `VERIFY` records a passed, failed, or partial result and public evidence references.
- `POLICY` records a maintainer-authored policy entry.
- `TAG` and `UNTAG` add or remove a classification tag without rewriting the original request. Only the record author or a maintainer may change tags, up to 20 may be active at once, and every change remains in the hashed event history.
- `MODERATE` locks or unlocks replies with a public rationale. It is available only to a maintainer or an assigned moderator for that record's category; category reply-disabled policy cannot be overridden.

The server supplies the event author, TIDE identity, authentication context, and timestamp; event-specific fields are stored under `payload.data`. Implementation events require an active developer role or maintainer authorization, while verification events require an active tester/reviewer role or maintainer authorization. The role used is recorded with the event. Each event links to its predecessor, is hashed with SHA-256, and is appended in a transaction that serializes concurrent changes to that record. `GET /api/v1/forum/requests/{public_id}/integrity` recomputes the canonical hash of the immutable original content and checks event hashes and chain links. A decision status is projected from the latest decision event; it does not rewrite the request. Keep secrets and private evidence out of public event payloads. Discussion replies use `GET` and `POST /api/v1/forum/requests/{public_id}/replies`; they support nested responses via an optional `parent_reply_id` and remain separate from the authoritative event ledger. Reply editing/deletion is not implemented.

The list endpoint supports full-text search over titles, original content, and rationale, as well as category, record-type, current-tag, newest-first, and oldest-first filters. Maintainers may assign or revoke category moderators through a separate append-only assignment log. The legacy forum remains available for existing records under its previous editing and moderation behavior; old submissions are not migrated or mapped into TIDE. No historical TIDE number is reserved for an unpublished idea.
