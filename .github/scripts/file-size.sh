#!/usr/bin/env bash
# A ratchet on the size of source files (run by repo-rules.sh, so CI and the local gate both
# check it): a tracked Rust file that is not a test may have at most LIMIT lines. The few that
# are longer are listed in file-size-allowlist.txt with their size; a listed file may not
# grow, and once it shrinks its entry must be lowered to match (or removed below LIMIT), so the
# list only ever gets shorter.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

LIMIT=600
allowlist=.github/scripts/file-size-allowlist.txt
failed=0

# The lines `path size` of the allowlist, comments and blank lines left out.
entries() { grep -v -e '^#' -e '^[[:space:]]*$' "$allowlist" || true; }

# Every entry is a path and a whole number of lines, nothing else, one entry per path.
for path in $(entries | awk '{ print $1 }' | sort | uniq -d); do
    echo "$allowlist: $path is listed more than once"
    failed=1
done
while read -r path lines rest; do
    if [[ ! "$lines" =~ ^[0-9]+$ || -n "$rest" ]]; then
        echo "$allowlist: \"$path $lines${rest:+ $rest}\": an entry is a path and a whole number of lines"
        failed=1
    fi
done < <(entries)

while IFS= read -r -d '' file; do
    case "$file" in
    */tests/* | */tests.rs) continue ;;
    esac
    n=$(wc -l <"$file" | tr -d ' ')
    max=$(entries | awk -v f="$file" '$1 == f { print $2 }')
    [[ -n "$max" && ! "$max" =~ ^[0-9]+$ ]] && continue
    if [[ -z "$max" ]]; then
        if ((n > LIMIT)); then
            echo "$file: $n lines (at most $LIMIT; split it)"
            failed=1
        fi
    elif ((n > max)); then
        echo "$file: $n lines, more than the $max it is allowed (it may only shrink)"
        failed=1
    elif ((n < max)); then
        if ((n > LIMIT)); then
            echo "$file: $n lines; lower its entry in $allowlist from $max to $n"
        else
            echo "$file: $n lines, under $LIMIT; remove its entry from $allowlist"
        fi
        failed=1
    fi
done < <(git ls-files -z -- '*.rs')

for path in $(entries | awk '{ print $1 }'); do
    if ! git ls-files --error-unmatch -- "$path" >/dev/null 2>&1; then
        echo "$allowlist: $path is not a tracked file; remove its entry"
        failed=1
    fi
done

if ((failed)); then
    exit 1
fi
echo "file sizes: ok"
