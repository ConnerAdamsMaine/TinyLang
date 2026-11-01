---
title: TIDE Support
---

# TIDE Support

TIDE has a private ticket interface at `/support.html` and authenticated APIs under `/api/v1/tickets`. TIDE General Support (`general-support`) is a public forum category and must not be used for account recovery details, identity disputes, moderation appeals, private evidence, or sensitive security reports.

Ticket categories are Technical support, Account recovery, Authentication problems, Identity disputes, Authorship disputes, Abuse reports, Moderation appeals, Security disclosures, and General support. Tickets have permanent `TIDE-S-XXXXXX` IDs, private owner/support-agent conversations, staff-only notes, assignment, status history, and idempotent creation when an `Idempotency-Key` UUID is supplied. Owners can list and read only their tickets; designated support agents can work across the support queue. Being an administrator, maintainer, or moderator does not itself grant ticket access. Ticket status and assignment history is append-only; private messages are stored separately and are not made part of the public immutable ledger.

Supported states are Open, Assigned, Investigating, Awaiting Response, Resolved, and Reopened. Support agents may move tickets through the supported workflow; ticket owners may reopen a resolved ticket or reply to it. Adding an owner reply to a resolved ticket also reopens it. Invalid transitions are rejected by the API.

The initial support-agent set is controlled by explicit rows in `tide_support_agents`; it is not granted through the general site role hierarchy. A database administrator must review and authorize grants. Use a controlled database session to insert the specific recipient and grantor identities:

```sql
INSERT INTO tide_support_agents (user_id, granted_by)
SELECT recipient.id, grantor.id
FROM users AS recipient
CROSS JOIN users AS grantor
WHERE recipient.tide_user_id = 'TIDE-U-XXXXXX'
  AND recipient.deleted_at IS NULL
  AND grantor.tide_user_id = 'TIDE-U-YYYYYY'
  AND grantor.deleted_at IS NULL;
```

Confirm exactly one row was inserted. Support agents can add internal notes, assign tickets to other support agents using their TIDE User ID, and make permitted status transitions. Ticket users can reopen resolved tickets. To revoke staff access, remove the user's grant row; historical ticket messages and assignments continue to reference the stable user account, while the revoked user loses staff API access.

Tickets accept up to 10 simultaneous attachments, each at most 2 MiB, in PDF, PNG, JPEG, or UTF-8 plain-text format. The server checks the filename, declared media type, and basic file signature; downloads are authorized through the same ticket-owner/support-agent access check and are sent as attachments with `nosniff` and `private, no-store` headers. PostgreSQL stores the bytes, so include the database volume in capacity planning and verified backups. An uploader or support agent may delete an attachment; agents must provide a reason. Deletion removes the bytes while retaining the attachment metadata and an append-only ticket event. A scheduled cleanup purges bytes 90 days after a ticket is resolved, but only while it remains resolved. Reopening clears the resolution timestamp; resolving again starts a new 90-day period. Attachment metadata remains available after deletion or expiry. This is an application retention policy, not a substitute for database backup expiration or legal-retention controls.

Creating or participating in a support ticket requires a session established by a linked GitHub, Discord, or X identity. For local-password accounts, loss of linked providers can be handled by resetting the password through the verified email, signing in, and linking a replacement; recovery when the verified email is also inaccessible is not implemented. Suspended accounts cannot use the authenticated ticket API, but TIDE moderation appeals use a separate verified-email link flow documented in [TIDE Moderation](TIDE_MODERATION.md). A dedicated staff queue for investigation and security-disclosure workflows remains unavailable; appeal reviewers can use the appeals page. Disposable-PostgreSQL integration coverage verifies owner/staff visibility and internal-note isolation. Attachment API flows, retention-worker behavior, assignment and status transitions, and broader message-flow integration tests remain outstanding.
