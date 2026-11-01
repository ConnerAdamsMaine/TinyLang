---
title: TIDE Testing
---

# TIDE Testing

From `Web`, the JavaScript checks and static-site tests run with:

```powershell
npm test
```

Rust formatting, compilation, and unit tests run with:

```powershell
cargo fmt --check
cargo test
```

Legacy suspension coverage includes unit tests for the pending-state, distinct-reviewer, and TIDE-action boundaries. An opt-in PostgreSQL API test verifies that an unsuspended account cannot be confirmed, the initial suspender cannot be the confirming reviewer, a distinct superadmin confirmation is audited, and legacy suspension/reinstatement endpoints preserve TIDE-linked restrictions. Run it only against a disposable database whose name includes `test` or `integration`:

```powershell
$env:TIDE_TEST_DATABASE_URL = "******localhost:55432/tinyone_integration"
cargo test legacy_suspension_confirmation_requires_a_distinct_second_reviewer -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

Discard that database after the test. Never point this variable at production, shared development, or any database containing data that must be preserved.

Ticket privacy API coverage is opt-in and requires a disposable integration
database. It verifies that owners see public messages but not staff-only notes,
an administrator without a support grant cannot read or list another user's
ticket, and designated support agents can access the queue and internal notes:

```powershell
$env:TIDE_TEST_DATABASE_URL = "******localhost:55432/tinyone_integration"
cargo test investigation_visibility_excludes_targets_and_private_evidence_from_reporters -- --ignored
cargo test ticket_visibility_and_internal_notes_are_scoped_to_owner_and_support_agents -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

These tests apply migrations and insert persistent fixtures; discard the test
database afterward. Owner/staff access, internal-note privacy, and attachment
API authorization/audit are covered. The retention-worker database test verifies
only bytes on tickets continuously resolved for at least 90 days are purged,
while recent and reopened-ticket contents remain and the digest is retained:

```powershell
$env:TIDE_TEST_DATABASE_URL = "******localhost:55432/tinyone_integration"
cargo test ticket_attachment_retention_purges_only_still_resolved_expired_bytes -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

Broader ticket message workflows still need database-backed coverage. The
focused ticket workflow integration test
does verify designated-agent assignment, support-role enforcement, permitted
status transitions, owner reopening, cleared resolution timestamps, and
append-only transition/assignment events:

```powershell
$env:TIDE_TEST_DATABASE_URL = "******localhost:55432/tinyone_integration"
cargo test ticket_assignment_status_and_owner_reopen_are_audited -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

The investigator API and grant-concurrency tests are opt-in and must only run against a disposable PostgreSQL database whose name contains `test` or `integration`. They apply the Web migrations and insert uniquely named fixtures. Because investigation history is append-only, discard the database after these tests rather than reusing a database with data that must be preserved:

```powershell
$env:TIDE_TEST_DATABASE_URL = "postgres://tinylang:test-only-password@localhost:55432/tinyone_integration"
cargo test evidence_write_requires_active_grant_and_records_audit_event -- --ignored
cargo test investigator_revocation_waits_for_in_flight_write_authorization -- --ignored
cargo test permanent_ban_requires_two_distinct_human_reviews -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

The visibility test verifies that reporters receive only the case summary, targets cannot list or read a case even when they have investigator access, and unrelated investigators can read private evidence. The API test verifies that a designated investigator can append evidence and an audit event, upload and download a private evidence attachment with the expected response headers, and loses access after grant revocation. The concurrency test verifies that revoking an investigator grant waits for an in-flight authorized transaction. The review test proves that one permanent-ban recommendation does not enforce the restriction, while two distinct reviewers trigger the recorded enforcement and revoke sessions. Do not point `TIDE_TEST_DATABASE_URL` at production, shared development, or any database containing data that must be preserved. These tests do not read or use `DATABASE_URL`.

Investigation evidence retention is also database-backed and opt-in. It verifies that bytes from closed or appealed cases are purged only after 90 days, while recent and open cases retain content, and that digests remain and repeated cleanup is idempotent:

```powershell
$env:TIDE_TEST_DATABASE_URL = "******localhost:55432/tinyone_integration"
cargo test investigation_attachment_retention_purges_only_expired_closed_cases -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

Independent conflict adjudication is opt-in and database-backed. It verifies that declarants, case targets, and prior reviewers cannot adjudicate; a split remains pending; two matching votes finalize an assessment without removing the original declaration or lifting recusal; and finalized assessments reject additional votes:

```powershell
$env:TIDE_TEST_DATABASE_URL = "******localhost:55432/tinyone_integration"
cargo test conflict_declarations_require_independent_matching_adjudication_and_never_unrecuse -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

The ban-evasion detection test is opt-in and database-backed. It verifies that an established account posting identical content never signals, a brand-new account reposting an actively restricted account's content creates exactly one unreviewed private investigation, a second identical submission is suppressed while the case is open, the similarity score is recorded, and the new account is never restricted by the detector:

```powershell
$env:TIDE_TEST_DATABASE_URL = "******localhost:55432/tinyone_integration"
cargo test possible_ban_evasion_signals_are_unreviewed_and_non_punitive -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

The recovery workflow test is also opt-in and requires a disposable integration database. It verifies that issuance requires the current password, replaces a prior set with ten codes, races two recovery attempts using the same code and confirms exactly one succeeds, revokes sessions and old account tokens, consumes every issued code, leaves the replacement email unverified, confirms the new password is usable, and rejects a later retry:

```powershell
$env:TIDE_TEST_DATABASE_URL = "postgres://tinylang:test-only-password@localhost:55432/tinyone_integration"
cargo test recovery_consumes_codes_and_revokes_sessions_atomically -- --ignored
cargo test recovery_rejects_deleted_accounts_and_email_conflicts -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

These tests insert unique accounts and recovery fixtures, so discard their database afterward. They exercise persistence and response behavior with email delivery unconfigured; actual verification-email delivery remains untested.

The appeal workflow integration test is opt-in and uses the same disposable PostgreSQL requirement. It rejects an expired token, submits a verified appellant's valid token concurrently and confirms single-use behavior, rejects a reviewer who participated in the original case, confirms one vote and a split vote leave the appeal open without changing the restriction, then records two matching independent reversal votes and checks the restriction and immutable appeal events:

```powershell
$env:TIDE_TEST_DATABASE_URL = "postgres://tinylang:test-only-password@localhost:55432/tinyone_integration"
cargo test appeal_token_is_single_use_and_requires_matching_independent_reviews -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

This test inserts a complete unique moderation/appeal fixture and requires discarding the test database afterward. It does not test appeal-email delivery.

The chat API integration test is opt-in and requires the same disposable database. It verifies block-aware creation for one-to-one and group conversations, idempotency for one-to-one channel creation, canonical participant ordering for private fixed-membership groups, member-only history, recipient notifications that omit message content, outsider denial, report submission by another group member, moderator-only report listing and message removal, and that revoking a session invalidates the WebSocket access check:

```powershell
$env:TIDE_TEST_DATABASE_URL = "postgres://tinylang:test-only-password@localhost:55432/tinyone_integration"
cargo test group_direct_messages_are_idempotent_private_and_block_aware -- --ignored
Remove-Item Env:TIDE_TEST_DATABASE_URL
```

Discard the database after the test because its group membership records are persistent.

The TIDE unit tests cover public ID shape validation, deterministic SHA-256 output, UUIDv7 generation, PostgreSQL timestamp precision, support-ticket and appeal-token hash stability, bounded pagination, ticket status transitions, private investigation visibility, permanent-ban reviewer thresholds, canonical GitHub evidence references, PKCE challenge encoding, browser-bound provider sign-in state, rejection of local sessions for TIDE participation, actual provider authentication attestation, attachment filename/type/signature/size validation, category-definition slug/role/rationale validation, and canonical group-DM participant keys. The Web static checks cover GitHub, Discord, and X PKCE flows, provider-authenticated TIDE session enforcement, existing-linked-identity sign-in without automatic account creation or linking, OAuth-start rate limits, account recovery routes and UI, X account linking, permanent identity display, TIDE participation gates, public chat channel-ID resolution, nested replies, bounded WebSocket send buffering and reconnect behavior, chat search/mentions/one-to-one and group direct messages/cross-instance fan-out, append-only category-definition administration, forum full-text search and category/type/order filters, contributor roles, private-ticket attachment routes/storage/retention/UI, private investigation evidence attachment routes/storage/retention/UI and transactional investigator-grant locking, appeals, notification API/UI contracts, and static checks for encrypted/signed offsite backup and guarded restore configuration, in addition to static HTML safety, root-relative references, TPack rendering constraints, and Markdown escaping. Localhost-mocked GitHub, Discord, and X callback integration tests exercise PKCE verifier exchange, identity linking, existing-linked-identity sign-in for GitHub, and one-time callback state handling. They do not contact providers or validate production credentials, provider availability, or live callback registrations.

The latest complete regression sweep passed formatting, 76 ordinary Rust tests, all 29 ignored PostgreSQL integrations against disposable PostgreSQL 16, 31 Node tests, and diff checks. The complete ordered migration chain through `0042_tide_ban_evasion_signals.sql` applied. Database coverage includes independently adjudicated conflict declarations, split and matching votes, persistent recusal after adjudication, ticket assignment/status/reopen audit, both attachment retention workers, content-only ban-evasion signaling with established-account and duplicate-signal negative controls, and the previous OAuth, forum, moderation, appeal, chat, recovery, category, and webhook coverage. The parallel chat notification test also passed after being made robust to unrelated concurrent PostgreSQL notifications. A real multi-replica end-to-end run also passed: `tests/chat-multi-replica/run.ps1` builds the production image, starts two web replicas against one disposable PostgreSQL via `docker-compose.multi.yml`, connects a WebSocket client to each, posts through replica A, and confirms the message arrives on replica B (and locally on A) before verified teardown. A live browser pass drove the real chat UI on both replicas: an account registered through the UI connected to `#general` on replica A and replica B simultaneously, and a message sent from replica A's browser rendered on replica B's browser without a reload. That browser pass surfaced and fixed a real client bug (`chat.js` read a `user` wrapper that the flattened `/api/auth/me` response does not return); a static assertion now guards it. Compose configuration validates, including assertion that dummy X OAuth credentials are passed to the web container. The deterministic backup E2E harness passes against a local S3 mock, not a live external bucket. Remaining deployment-only work includes live OAuth registration/provider verification, capacity/load and reconnect testing at scale on staging, and live external-bucket backup/restore verification. Broader governance-manipulation and coordinated-abuse detection remains incomplete.
