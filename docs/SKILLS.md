# Skills registry

Inventory made on 2026-11-19 on the P0 workstation (Windows 11). Re-check presence before use:
a skill listed here is not proof it is installed on *your* machine.

## External skills

| Name | Source | Commit | Installed | Role | Deps | Status |
|---|---|---|---|---|---|---|
| rust-best-practices | https://github.com/apollographql/skills (`skills/rust-best-practices`), skill metadata version 1.1.2 | `9dda1397730370aeb8df4ff8f2015c34c86b5003` | 2026-11-19, user-global (`~/.claude/skills`) via `npx skills add … -g -a claude-code` | Rust idioms, ownership, errors, clippy, tests, performance | Rust ≥ 1.70, cargo | ACTIVE (does not replace architecture review) |
| security-threat-model | https://github.com/openai/skills (`skills/.curated/security-threat-model`) | `49f948faa9258a0c61caceaf225e179651397431` | 2026-11-19, user-global | Repository-grounded threat modelling → `docs/SECURITY.md` | none | ACTIVE |
| uml-and-software-architecture-visualization | https://github.com/openai/plugins (`plugins/build-web-data-visualization/skills/…`) | `981b02c0c44058b595f3b4180002c3ae3c806d40` | 2026-11-19, user-global | Architecture / flow / state diagrams (Mermaid, versioned in `docs/`) | a Mermaid renderer to view | ACTIVE |

**Planning (mandate §35.1 D):** no dedicated skill installed. Workflow used instead: the mandate's
7-step checklist (problem, files, dependencies, risks, tests, GO/NO-GO, then implement), recorded in
the ADR or the PR description, with Claude Code plan mode for P0/P1/P2 work. Status: WORKFLOW.

External skills are installed per user, not vendored here. Any agent on another machine must
install the same commits (command and commits above).

## Project skills (`.skills/`)

| Name | Role | Version | Status |
|---|---|---|---|
| legacy2uefi-domain | Concepts, invariants, prohibitions — primary conceptual source | 0.1.0-draft | DRAFT, to review against mandate v1.0 + golden reference |
| legacy2uefi-hardware | Detection methods, "detect capabilities, not brands" | 0.1.0-draft | DRAFT |
| legacy2uefi-testing | Status rules, QEMU, physical evidence, regression | 0.1.0-draft | DRAFT |
| legacy2uefi-ui | TUI/GUI rules, semantic colours, fallback chain | 0.1.0-draft | DRAFT |

`.skills/` is agent-neutral. Claude Code discovers project skills in `.claude/skills/`; expose them
there with a symlink or a copy step, keeping `.skills/` as the only edited copy.
