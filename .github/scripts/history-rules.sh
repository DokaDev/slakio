#!/usr/bin/env bash
# Rules on the commits pushed (CI job `repo-rules`): for every commit in BASE..HEAD, merges
# left out,
#   1. the subject is a Conventional Commit with a lowercase type (`fix(tui): …`);
#   2. a `refactor` commit changes no screen snapshot (no behavior changed, so no screen did);
# and over the whole range,
#   3. the file-size allowlist was not loosened: no new entry, no entry raised.
# Without a usable BASE (a new branch, a force push, a shallow clone) the range falls back to
# the merge base with origin/main, else to the whole history, with a warning; it never passes
# silently.
# Usage: history-rules.sh [BASE] [HEAD]
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

base=${1:-}
head=${2:-HEAD}
allowlist=.github/scripts/file-size-allowlist.txt
types='feat|fix|refactor|test|ci|docs|chore|build|perf|style|revert'
failed=0

warn() { echo "::warning::$*"; }
usable() { [[ -n "$1" && ! "$1" =~ ^0+$ ]] && git cat-file -e "$1^{commit}" 2>/dev/null; }

if ! usable "$base"; then
    fallback=$(git merge-base origin/main "$head" 2>/dev/null || true)
    if usable "$fallback" && [[ "$(git rev-parse "$fallback")" != "$(git rev-parse "$head")" ]]; then
        warn "no usable base '${base}'; checking from the merge base with origin/main ($fallback)"
        base=$fallback
    else
        warn "no usable base '${base}'; checking the whole history"
        base=
    fi
fi
range=${base:+$base..}$head

for commit in $(git rev-list --no-merges "$range"); do
    subject=$(git log -1 --format=%s "$commit")
    short=$(git log -1 --format=%h "$commit")
    if [[ ! "$subject" =~ ^($types)(\([a-z0-9._/-]+\))?!?:\ [^\ ] ]]; then
        echo "$short \"$subject\": not a Conventional Commit subject (type: $types, lowercase)"
        failed=1
    fi
    if [[ "$subject" =~ ^refactor(\(|!|:) ]]; then
        changed=$(git diff-tree --no-commit-id --root --name-only -r "$commit" | grep '/snapshots/' || true)
        if [[ -n "$changed" ]]; then
            echo "$short \"$subject\": a refactor that changes snapshots:"
            while IFS= read -r file; do echo "  $file"; done <<<"$changed"
            failed=1
        fi
    fi
done

# The allowlist as `path size` lines, comments and blank lines left out.
entries() { grep -v -e '^#' -e '^[[:space:]]*$' || true; }
if [[ -n "$base" ]] && git cat-file -e "$base:$allowlist" 2>/dev/null; then
    before=$(git show "$base:$allowlist" | entries)
    while read -r path lines; do
        [[ -z "$path" ]] && continue
        was=$(awk -v f="$path" '$1 == f { print $2 }' <<<"$before")
        if [[ -z "$was" ]]; then
            echo "$allowlist: $path was added; split the file instead"
            failed=1
        elif ((lines > was)); then
            echo "$allowlist: $path raised from $was to $lines; it may only be lowered"
            failed=1
        fi
    done < <(git show "$head:$allowlist" | entries)
else
    warn "no allowlist at the base; its entries are not compared"
fi

if ((failed)); then
    exit 1
fi
echo "history rules ($range): ok"
