#!/usr/bin/env bash
# A refactor changes no behavior, so it changes no screen snapshot: every commit in
# BASE..HEAD whose subject is `refactor: …` or `refactor(scope): …` must leave
# `**/snapshots/**` untouched. A commit that does change a snapshot is not a refactor.
# Usage: refactor-snapshots.sh BASE [HEAD]
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

base=${1:?usage: refactor-snapshots.sh BASE [HEAD]}
head=${2:-HEAD}
failed=0
for commit in $(git rev-list "$base..$head"); do
    subject=$(git log -1 --format=%s "$commit")
    if [[ "$subject" =~ ^refactor(\([^\)]*\))?!?: ]]; then
        changed=$(git diff-tree --no-commit-id --name-only -r "$commit" | grep '/snapshots/' || true)
        if [[ -n "$changed" ]]; then
            echo "$(git log -1 --format='%h %s' "$commit") changes snapshots:"
            while IFS= read -r file; do echo "  $file"; done <<<"$changed"
            failed=1
        fi
    fi
done
if ((failed)); then
    exit 1
fi
echo "refactors change no snapshot: ok"
