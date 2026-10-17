"""Small, strict text-editing helpers for scripted spec edits.

Every helper fails loudly when its anchor text is missing or ambiguous, so a
scripted edit can never silently do nothing or touch the wrong place.

Usage from a script:  sys.path.insert(0, "scripts"); from spec_edit import *
"""


def load(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def save(path, text):
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def rep(text, old, new):
    """Replace the single occurrence of `old`."""
    count = text.count(old)
    if count != 1:
        raise ValueError("expected exactly one match, found %d: %r" % (count, old[:80]))
    return text.replace(old, new)


def repline(text, prefix, new):
    """Replace the single line that starts with `prefix`."""
    lines = text.split("\n")
    hits = [i for i, line in enumerate(lines) if line.startswith(prefix)]
    if len(hits) != 1:
        raise ValueError("expected exactly one line starting with %r, found %d" % (prefix, len(hits)))
    lines[hits[0]] = new
    return "\n".join(lines)


def repblock(text, start, end, new):
    """Replace from `start` (inclusive) up to `end` (exclusive)."""
    if text.count(start) != 1:
        raise ValueError("expected exactly one block start: %r" % start[:80])
    i = text.index(start)
    j = text.index(end, i)
    return text[:i] + new + text[j:]


def edit(path, fn):
    save(path, fn(load(path)))
