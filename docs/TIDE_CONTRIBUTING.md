---
title: TIDE Contributing
---

# TIDE Contributing

1. Create or use a website account and sign in.
2. Link a GitHub, Discord, or X account under Account. Linking does not create a session authenticated by that provider.
3. Sign in through the linked provider, then open the [TIDE page](https://tl.404connernotfound.dev/tide.html) and read the category descriptions. Provider sign-in does not create accounts or link identities automatically.
4. Submit a record and use its permanent ID when discussing it. Do not choose an ID; the server allocates it.
5. Keep private credentials and unpatched vulnerability details out of public records.
6. Use the [private support page](https://tl.404connernotfound.dev/support.html) for account, authorship, abuse, appeal, and private security concerns.

The interface supports nested discussion replies and append-only amendments, relationships, maintainer decisions, implementation evidence, verification events, and policy events. A TIDE resolution is a separate forum record; a maintainer can select it in a decision event. Original records and authors remain unchanged. Requirements have append-only lifecycle status history.

Maintainers may grant or revoke the `developer`, `tester`, and `reviewer` contributor roles through `POST /api/v1/contributors/{TIDE_USER_ID}/roles`, with JSON fields `role`, `action` (`granted` or `revoked`), and a nonempty `rationale`. Role changes are append-only and public: current roles are visible from `GET /api/v1/contributors/{TIDE_USER_ID}/roles`, their latest 100 grant/revoke events from `GET /api/v1/contributors/{TIDE_USER_ID}/role-events`, and an authenticated account can inspect its own current roles at `GET /api/v1/contributors/me/roles`. Role-change rationales are public. An active developer role permits implementation events; tester and reviewer roles permit verification events. Maintainers retain those permissions, and their contributor role is recorded on implementation and verification events. TIDE forum, ticket, investigation, and chat write actions require a session established by a supported OAuth provider linked to that same TIDE identity. Existing legacy forum records remain separate and retain their previous identifiers.

Forum replies and TIDE workflow updates appear in the existing account inbox. The inbox links to the relevant record, ticket, investigation, or appeal. Private support messages and investigator evidence are never included in notification text.
