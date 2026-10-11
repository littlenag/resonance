"""Command-line tool: prints the inventory as a table."""

import sys


def render(items):
    """Return the inventory as a fixed-width table."""
    width = max((len(name) for name, _ in items), default=4)
    lines = [f"{'name'.ljust(width)}  qty"]
    for name, qty in items:
        lines.append(f"{name.ljust(width)}  {qty:>3}")
    return "\n".join(lines)


def main(argv):
    items = [tuple(arg.split("=", 1)) for arg in argv[1:]]
    print(render([(n, int(q)) for n, q in items]))


if __name__ == "__main__":
    main(sys.argv)
