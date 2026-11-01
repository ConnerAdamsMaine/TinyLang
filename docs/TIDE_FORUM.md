---
title: TIDE Forum
---

# TIDE Forum

TIDE separates the affected-area category from the purpose of a record. All category reads are public. Member posting is enabled for every category except announcements, where only maintainers may post. Public discussion supports nested replies tied to the same record; categories can disable replies.

| Official category | Slug | Description and example topics | Posting |
| --- | --- | --- | --- |
| TIDE Announcements | `announcements` | Official project-wide communications: releases, milestones, maintenance, roadmaps, and governance updates. Example: a release announcement. | Maintainers only |
| TIDE Compiler | `compiler` | Compiler architecture and behavior: lexing, parsing, IR, optimization, code generation, diagnostics, native compilation, and backends. Example: a parser diagnostic proposal. | Members |
| TIDE Language Design | `language-design` | Language syntax, semantics, type systems, ergonomics, safety, and compile-time behavior. Example: a type-inference proposal. | Members |
| TIDE Runtime | `runtime` | Execution infrastructure: VM, bytecode, JIT/AOT, memory management, scheduling, and runtime optimization. Example: a VM execution issue. | Members |
| TIDE Standard Library | `standard-library` | Standard and extended library APIs, correctness, performance, deprecations, and library documentation. Example: a new standard-library module. | Members |
| TIDE ABI & FFI | `abi-ffi` | Binary interfaces, calling conventions, C interoperability, data layout, native calls, and foreign callbacks. Example: an ABI compatibility report. | Members |
| TIDE Tooling | `tooling` | CLI, debugging, formatting, linting, builds, IDE integration, tests, and development automation. Example: a formatter enhancement. | Members |
| TIDE Platforms & Compatibility | `platforms-compatibility` | Operating systems, architectures, embedded targets, platform behavior, and backward/migration compatibility. Example: a Windows compatibility report. | Members |
| TIDE Performance | `performance` | Cross-subsystem benchmarking, profiling, memory efficiency, overhead, regressions, and allocation behavior. Example: a reproducible cross-component benchmark. | Members |
| TIDE Documentation | `documentation` | Tutorials, developer guides, API and language references, examples, and corrections. Example: a request to clarify a language-reference section. | Members |
| TIDE Bugs & Regressions | `bugs-regressions` | Unclassified bugs, cross-system regressions, crashes, reproduction cases, and unexpected compatibility problems. Example: a crash report spanning compiler and runtime. | Members |
| TIDE Governance | `governance` | TIDE governance, moderation policy, contributor attribution, development procedures, and decision standards. Example: a proposed governance-policy amendment. | Members |
| TIDE General Discussion | `general-discussion` | General TinyLang conversation, introductions, project showcases, feedback, and topics without a better subsystem fit. Example: a community project showcase. | Members |
| TIDE General Support | `general-support` | Public installation, configuration, build, usage, and troubleshooting assistance. Example: a build-configuration question. This is not private support. | Members |
| TIDE Security | `security` | Public defensive security engineering, hardening, and coordinated published advisories. Example: discussion of a published advisory. Never post an unpatched vulnerability or private evidence here. | Members |

The API record types are Request, Bug report, Engineering proposal, Resolution, Discussion, Announcement, Documentation request, and Verification report. A category describes the affected area; a record type describes why the contribution is being made. Tags add a third, independent classification dimension. The record author or a maintainer can append tag and untag events; current tags are derived from the immutable event ledger, changes remain auditable, and each record can have up to 20 active lowercase slug tags (2-32 characters). The API and browser page support tag filtering. The submission form exposes the record types; publishing an Announcement requires maintainer authorization. A new record starts as Active. Maintainers can append decision events that derive its current status, while amendments, relationships, implementation evidence, verification reports, tag changes, and policy events remain separate history entries.

All seeded categories are publicly readable and member-postable except TIDE Announcements. Reply availability is controlled by the category record. Linked OAuth maintainers can create a category or update its name, description, posting role, reply availability, and ordering through the TIDE category API. Slugs are stable, categories cannot be deleted, and each change requires a rationale and appends immutable before/after state. Category definition history is publicly readable; the migration does not invent change events for categories that existed before history was installed. Maintainers can assign or revoke category moderators using an append-only, reasoned assignment history. The current moderator list is public. Assigned moderators may append an auditable `MODERATE` event to lock or unlock replies on records in their category; the event ledger preserves every action and the API serializes it with new replies. A category configured without replies cannot be overridden by a moderator.

The category index supports filtering records by category and record type, full-text search across titles, original content, and rationale, and newest/oldest ordering. Filtered records are paginated by the versioned API.

Do not post credentials, private account evidence, or unpatched vulnerability reports in public categories.
