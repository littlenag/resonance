#!/usr/bin/env bash
# Create a fresh git repository from the starter fixtures, for field-testing Resonance on
# something other than its own repository. Prints the repository's path.
#
# Usage: scripts/fixture_repo.sh [DIR]   (DIR must not exist; default: a new temporary directory)
#
# Besides the committed fixture files, it generates the edge cases that should not be kept
# in Resonance's own repository: build output and dependency directories (never ingested),
# a file ignored by a nested .gitignore, an editor swap file, a binary file and a file larger
# than ingest.maxBlobBytes (recorded by hash and size only).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src="$here/fixtures/starter"

if [[ $# -ge 1 && -n "$1" ]]; then
    dest="$1"
    if [[ -e "$dest" ]]; then
        echo "fixture-repo: $dest already exists; give a new path" >&2
        exit 1
    fi
    mkdir -p "$dest"
else
    dest="$(mktemp -d "${TMPDIR:-/tmp}/resonance-fixture.XXXXXX")"
fi
dest="$(cd "$dest" && pwd)"

cp -R "$src/." "$dest/"
cd "$dest"
git init -q -b main
git config user.name "Fixture Developer"
git config user.email "fixture@example.com"
git config commit.gpgsign false

# Edge cases that are generated rather than stored.
mkdir -p target/debug web/node_modules/left-pad py/app/__pycache__ web/dist assets
printf 'compiled\n' > target/debug/inventory
printf 'module.exports = 1;\n' > web/node_modules/left-pad/index.js
printf 'bytecode\n' > py/app/__pycache__/main.cpython-312.pyc
printf 'bundle\n' > web/dist/bundle.js
printf 'export const apiKey = "local";\n' > web/src/config.local.ts
printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR' > assets/logo.png
head -c 6291456 /dev/zero | tr '\0' 'x' > assets/large.log
printf 'swap\n' > src/.lib.rs.swp

cat > .gitignore <<'IGNORE'
target/
web/node_modules/
web/dist/
__pycache__/
*.swp
IGNORE

git add -A
git commit -q -m "Starter fixture"

cat <<MSG
$dest

A fresh fixture repository. Try:
  cd $dest
  resonance init          # mirrors the repository, installs hooks and the agent kit
  claude                  # then, in the agent session:
    > Give me the Resonance orientation.
    > Bring spec/requirements.md into Resonance.
    > Declare record (src/lib.rs) and the normalize test, link them to FR-001 and FR-003,
      and run the tests.
    > What implements FR-001?
  resonance tui           # browse and edit the spec yourself
  resonance status
MSG
