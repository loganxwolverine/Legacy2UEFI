#!/bin/sh
# Coherence check between agent.md and other sources of truth (mandate §36.4).
# POSIX sh, no dependencies beyond grep/sed. Exit 1 on divergence.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
fail=0
err() { printf 'agent.md check: %s\n' "$*" >&2; fail=1; }
key() { sed -n "s/^$1: //p" agent.md | head -n 1; }

[ -f agent.md ] || { err "agent.md missing"; exit 1; }

# 1. OpenCore version must match third_party/opencore/version
oc_agent=$(key OPENCORE_VERSION)
oc_file=$(head -n 1 third_party/opencore/version 2>/dev/null || true)
[ -n "$oc_agent" ] || err "OPENCORE_VERSION key missing"
[ "$oc_agent" = "$oc_file" ] || err "OpenCore version differs: agent.md='$oc_agent' third_party='$oc_file'"

# 2. Project phase must be declared
key PROJECT_PHASE | grep -Eq '^P[0-4] ' || err "PROJECT_PHASE missing or malformed"

# 3. Every project skill directory must be listed, and every listed project skill must exist
skills=$(key REQUIRED_SKILLS)
for d in .skills/*/; do
  n=$(basename "$d")
  [ -f "$d/SKILL.md" ] || err "$d has no SKILL.md"
  printf '%s\n' "$skills" | tr ',' '\n' | sed 's/^ *//' | grep -qx "$n" || err "skill $n not listed in REQUIRED_SKILLS"
done
for n in $(printf '%s\n' "$skills" | tr ',' '\n' | sed 's/^ *//' | grep '^legacy2uefi-' || true); do
  [ -f ".skills/$n/SKILL.md" ] || err "REQUIRED_SKILLS lists $n but .skills/$n/SKILL.md is missing"
done

# 4. Every skill must appear in docs/SKILLS.md
for n in $(printf '%s\n' "$skills" | tr ',' ' '); do
  grep -q "$n" docs/SKILLS.md || err "skill $n not documented in docs/SKILLS.md"
done

# 5. Pointer files must not carry project content
for p in AGENTS.md CLAUDE.md; do
  [ ! -f "$p" ] || [ "$(wc -l < "$p")" -le 8 ] || err "$p looks like it duplicates agent.md content"
done

# 6. VERIFIED hardware must have evidence
if sed -n '/^## Verified hardware/,/^## /p' agent.md | grep -q '^- '; then
  [ -d tests/evidence ] || err "agent.md lists VERIFIED hardware but tests/evidence/ is missing"
fi

[ "$fail" -eq 0 ] && echo "agent.md check: OK"
exit "$fail"
