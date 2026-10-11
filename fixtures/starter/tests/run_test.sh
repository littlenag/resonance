#!/bin/sh
# Runs one named test; exit code 0 is a pass. Use as:
#   resonance test add tests/run_test.sh:7-10 --name normalize --run "sh tests/run_test.sh normalize"
set -u
name="${1:-}"
case "$name" in
  normalize)
    out=$(printf '  Big   Widget ' | tr -s ' ' | sed 's/^ //; s/ $//' | tr 'A-Z' 'a-z')
    [ "$out" = "big widget" ] && echo "normalize ok" && exit 0
    echo "normalize failed: '$out'"; exit 1 ;;
  table)
    python3 py/app/main.py widget=3 >/dev/null && echo "table ok" && exit 0
    exit 1 ;;
  always_fails)
    echo "this test always fails"; exit 1 ;;
esac
echo "running 0 tests (no test named '$name')"
exit 0
