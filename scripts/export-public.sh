#!/usr/bin/env bash
# The public repository's history: this repository's tree at REV, committed onto the local
# branch `public` with none of this repository's own history behind it. The first export is
# a root commit and each later one stacks on the last; the `public` remote's push refspec
# (refs/heads/public:refs/heads/main) sends it. This repository stays private, since commits
# before 2026-10-19 carry a test unit's Bluetooth address and network secret.
#
# The DROP paths are left out, public/ is copied over the root (the README that the public
# repository's readers get), and paths under this user's home directory are written from ~.
# Before committing, the tree is refused if it names a test unit or this computer: a fixed
# string from .public-deny (gitignored: serials, network names, account names), this user's
# home directory, a tailnet address, or a MAC address other than a zeroed one.
#
# Usage: scripts/export-public.sh [-n DIR] [-m MESSAGE] [REV]
#   -n DIR   write the tree to DIR after the checks and commit nothing

set -euo pipefail
cd "$(dirname "$0")/.."
DRY=
MSG=
while getopts n:m: o; do
	case $o in
	n) DRY=$OPTARG ;;
	m) MSG=$OPTARG ;;
	*) exit 2 ;;
	esac
done
shift $((OPTIND - 1))
# Kept private:
DROP=(
	AGENTS.md # notes for the agents working in this repository
	docs/sensme.md # the SensMe research notes stay private (user decision 2026-10-24)
	docs/sensme-analyzer.md
	scripts/re
	public    # copied over the root instead
)
REV=$(git rev-parse --verify "${1:-HEAD}^{commit}")
SHORT=$(git rev-parse --short "$REV")
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

export GIT_INDEX_FILE="$WORK/index"
git read-tree "$REV"
git rm -r -q --cached --ignore-unmatch -- "${DROP[@]}"
TREE=$(git write-tree)
unset GIT_INDEX_FILE

mkdir "$WORK/tree"
git archive "$TREE" | tar -x -C "$WORK/tree"
if git cat-file -e "$REV:public" 2>/dev/null; then
	git archive "$REV" public | tar -x -C "$WORK"
	cp -a "$WORK/public/." "$WORK/tree/"
fi
{ grep -rlIF "$HOME/" "$WORK/tree" || true; } | while IFS= read -r f; do
	sed -i "s|$HOME/|~/|g" "$f"
done
found="$WORK/found"
(
	cd "$WORK/tree"
	if [ -s "$OLDPWD/.public-deny" ]; then
		sed 's/#.*//; s/[[:space:]]*$//' "$OLDPWD/.public-deny" | grep . >"$WORK/deny" || true
		[ ! -s "$WORK/deny" ] || grep -rnaF -f "$WORK/deny" . || true
	fi
	grep -rnIF "$HOME/" . || true
	grep -rnIE '(^|[^0-9.])100\.(6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])\.[0-9]{1,3}\.[0-9]{1,3}([^0-9]|$)' . || true
	grep -rnIoE '([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}' . | grep -vE ':0[02](:00){5}$' || true
) >"$found"
if [ -s "$found" ]; then
	echo "export-public: $SHORT names a test unit or this computer:" >&2
	cut -c1-200 "$found" >&2
	exit 1
fi

if [ -n "$DRY" ]; then
	mkdir -p "$DRY"
	cp -a "$WORK/tree/." "$DRY/"
	echo "$DRY: the public tree of $SHORT ($(find "$DRY" -type f | wc -l) files)"
	exit 0
fi

COMMON=$(git rev-parse --path-format=absolute --git-common-dir)
TREE=$(
	export GIT_DIR="$COMMON" GIT_WORK_TREE="$WORK/tree" GIT_INDEX_FILE="$WORK/public-index"
	cd "$WORK/tree"
	git -c core.autocrlf=false add -A -f -- .
	git write-tree
)

PARENT=$(git rev-parse -q --verify refs/heads/public || true)
if [ -n "$PARENT" ] && [ "$(git rev-parse "$PARENT^{tree}")" = "$TREE" ]; then
	echo "public already holds the tree of $SHORT"
	exit 0
fi
COMMIT=$(git commit-tree "$TREE" ${PARENT:+-p "$PARENT"} -m "${MSG:-Walkman Manager at $SHORT}

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>")
git update-ref refs/heads/public "$COMMIT" $PARENT
echo "public: $(git rev-parse --short "$COMMIT") with the tree of $SHORT"
