#!/usr/bin/env bash
# Rules on the commits pushed (CI job `repo-rules`): for every commit in BASE..HEAD, merges
# left out,
#   1. the subject is a Conventional Commit with a lowercase type (`fix(tui): …`);
#   2. a `refactor` commit changes no screen snapshot (no behavior changed, so no screen did);
#   3. a commit that touches the file-size allowlist adds no entry and raises none against its
#      parent (and the same holds over the whole range);
#   4. a change to a guard (the limit of file-size.sh, these scripts, clippy.toml, the CI
#      workflow) is named in the pull request's body (PR=true, PR_BODY), or, on a push,
#      flagged with a warning.
# Without a usable BASE (a new branch, a force push, a shallow clone) the range falls back to
# the merge base with origin/main, else to the whole history, with a warning; it never passes
# silently.
# Usage: [PR=true PR_BODY=…] history-rules.sh [BASE] [HEAD]
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

base=${1:-}
head=${2:-HEAD}
allowlist=.github/scripts/file-size-allowlist.txt
types='feat|fix|refactor|test|ci|docs|chore|build|perf|style|revert'
failed=0

warn() { echo "::warning::$*"; }
# The allowlist as `path size` lines, comments and blank lines left out.
entries() { grep -v -e '^#' -e '^[[:space:]]*$' || true; }
# Fails (and says why) when the allowlist at $2 adds an entry or raises one against $1.
loosened() {
    local before after path lines was out=0
    before=$(git show "$1:$allowlist" 2>/dev/null | entries)
    after=$(git show "$2:$allowlist" 2>/dev/null | entries)
    while read -r path lines; do
        [[ -z "$path" ]] && continue
        was=$(awk -v f="$path" '$1 == f { print $2 }' <<<"$before")
        if [[ -z "$was" ]]; then
            echo "$allowlist ($3): $path was added; split the file instead"
            out=1
        elif ((lines > was)); then
            echo "$allowlist ($3): $path raised from $was to $lines; it may only be lowered"
            out=1
        fi
    done <<<"$after"
    return "$out"
}
guards=(.github/scripts/file-size.sh .github/scripts/history-rules.sh
    .github/scripts/repo-rules.sh .github/scripts/dependency-direction.sh clippy.toml .github/workflows/ci.yml)
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
    parent=$(git rev-parse -q --verify "$commit^" || true)
    if [[ -n "$parent" ]] && git diff-tree --no-commit-id --name-only -r "$commit" | grep -qx "$allowlist" &&
        git cat-file -e "$parent:$allowlist" 2>/dev/null; then
        loosened "$parent" "$commit" "$short" || failed=1
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

if [[ -n "$base" ]] && git cat-file -e "$base:$allowlist" 2>/dev/null; then
    loosened "$base" "$head" "$range" || failed=1
else
    warn "no allowlist at the base; the range as a whole is not compared"
fi

# Guards changed in the range: named in the pull request's body, or flagged.
changed=$(git diff --name-only "${base:-$(git rev-list --max-parents=0 "$head" | tail -1)}" "$head" -- "${guards[@]}")
if [[ -n "$changed" ]] && git diff "${base:-$(git rev-list --max-parents=0 "$head" | tail -1)}" "$head" -- .github/scripts/file-size.sh |
    grep -q '^[-+]LIMIT='; then
    warn "the size limit of file-size.sh changed"
fi
while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    if [[ "${PR:-}" == true ]]; then
        if ! grep -qF "$file" <<<"$PR_BODY"; then
            echo "$file: a guard changed; name it in the pull request's body and say why"
            failed=1
        fi
    else
        warn "a guard changed: $file"
    fi
done <<<"$changed"

if ((failed)); then
    exit 1
fi
echo "history rules ($range): ok"
