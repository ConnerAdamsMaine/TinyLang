---
title: TIDE Community and Governance
---

# TIDE: TinyLang Intent & Decision Engine

TIDE is the official destination for new TinyLang development requests and
engineering records. It replaces the previously planned TLR/TLP/TLIN proposal
workflow. Categories identify the affected area; record types identify the
purpose of a contribution.

The implementation is being introduced alongside the pre-existing forum. Old
forum records remain in that legacy surface and are not assigned TIDE IDs or
rewritten as TIDE history. The old document submission endpoint is retired;
legacy editing, discussion, and moderation behavior remains separate and is
not covered by TIDE's append-only guarantees.

## Current participation

Open [TIDE on the TinyLang website](https://tl.404connernotfound.dev/tide.html).
Reading categories and records is public. To submit or participate, establish
your current session by signing in with a GitHub, Discord, or X identity already
linked to your TIDE account. Linking a provider while signed in with a local
password does not change that session; sign out and use provider sign-in.
Provider sign-in does not create accounts or link identities automatically.

The server assigns permanent `TIDE-U-XXXXXX` user identifiers and
`TIDE-XXXXXX` record identifiers. The original record content and its
SHA-256 content hash are preserved in an append-only database record. A
`REQUEST` event records the provider-authenticated session, linked-provider
context, and author. The
database rejects updates and deletions to TIDE original records and events.

## Official categories

| Name | Slug | Purpose |
| --- | --- | --- |
| TIDE Announcements | `announcements` | Official project-wide announcements. Maintainer posting only. |
| TIDE Compiler | `compiler` | Compiler architecture and behavior. |
| TIDE Language Design | `language-design` | Language syntax, semantics, and type-system design. |
| TIDE Runtime | `runtime` | Runtime execution infrastructure. |
| TIDE Standard Library | `standard-library` | Standard and extended library development. |
| TIDE ABI & FFI | `abi-ffi` | Binary interfaces and foreign-language interoperability. |
| TIDE Tooling | `tooling` | Tools used to build, debug, and develop TinyLang programs. |
| TIDE Platforms & Compatibility | `platforms-compatibility` | Operating systems, architectures, and compatibility. |
| TIDE Performance | `performance` | Cross-subsystem performance analysis. |
| TIDE Documentation | `documentation` | Documentation improvements. |
| TIDE Bugs & Regressions | `bugs-regressions` | General bug triage and cross-component failures. |
| TIDE Governance | `governance` | Governance and community policy. |
| TIDE General Discussion | `general-discussion` | General TinyLang community discussions. |
| TIDE General Support | `general-support` | Public technical assistance; not private tickets. |
| TIDE Security | `security` | Public defensive security discussion and published advisories. |

Every category is currently publicly readable and accepts member posts except
TIDE Announcements. Public categories are not a safe place for credentials,
unpatched vulnerabilities, or private account evidence.

## Record types

The API recognizes `request`, `bug_report`, `engineering_proposal`,
`resolution`, `discussion`, `announcement`, `documentation_request`, and
`verification_report`. At present, the TIDE page submits requests, bug reports,
engineering proposals, discussions, and documentation requests. Announcements
are restricted to maintainers.

The original-content table and request event are immutable. Requirement values
are retained as submitted. Nested discussion replies and append-only amendment,
relationship, decision, implementation, verification, and policy events are
available in the TIDE interface. Resolutions are separate records that
maintainers may select in a decision event; the original request is not
overwritten. The latest per-requirement status is projected from append-only
lifecycle events; each immutable reference starts as `Active`. Maintainers can
grant or revoke developer, tester, and reviewer roles, and authorized
implementation and verification events record the applicable role. New
records start as `Active` until a decision event changes the overall derived
status.

Private account, identity, abuse, appeal, and security support is separate from
the public `general-support` category. The [private support page](https://tl.404connernotfound.dev/support.html)
provides owner-scoped tickets with private messages, staff-only notes,
assignment, status history, and bounded evidence attachments. Account recovery
codes can restore access after losing linked providers and replace an
inaccessible email address; recovery without a valid recovery code is not
implemented. The separate [appeals page](https://tl.404connernotfound.dev/appeals.html)
supports email-verified appeals for active TIDE restrictions, including
suspensions.

The [private investigation page](https://tl.404connernotfound.dev/investigations.html)
accepts reports and provides restricted investigator review, append-only
evidence, and bounded private evidence attachments. Permanent bans require two
distinct human reviewers. Independent appeals and reversals are handled by a
separate email-verified appeal flow. Investigators can append personal,
financial, prior-involvement, or other conflict declarations, which recuse them
from further case participation. Independent investigators can adjudicate a
declaration using two matching votes; this records an assessment but never
deletes the disclosure or lifts the declarant's recusal. Automated duplicate,
rapid-submission-burst, and content-only ban-evasion signals create private,
non-punitive investigation leads. They are not findings. Ban-evasion detection
is limited to new accounts reposting an actively restricted account's content;
broader account-relationship and coordinated-abuse detection is not
implemented.

The [TIDE chat page](https://tl.404connernotfound.dev/chat.html) provides
authenticated channels for ordinary conversation. Chat messages expire after
90 days; use TIDE records, not chat, for official requests and decisions.

## Unpublished proposals

TLP-0001, TLP-0002, and TLP-0003 refer to previously discussed TinyLang v2
proposal ideas that remain unpublished working drafts. They have not been
migrated, posted, assigned TIDE identifiers, or recorded as accepted or
implemented contributions. Their authors should submit them through TIDE when
the required workflows are operational.

See [TIDE overview](TIDE_OVERVIEW.md), [TIDE forum](TIDE_FORUM.md), and
[TIDE requests](TIDE_REQUESTS.md) for implementation details and known limits.
