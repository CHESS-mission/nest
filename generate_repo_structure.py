#!/usr/bin/env python3
"""
Script to generate the file-folder structure of the repository for easier inspection.
Usage:
    python3 generate_repo_structure.py

Optional arguments:
    -d, --depth <N>   limit recursion depth (default: unlimited)
    -i, --ignore <pattern>  comma-separated substrings to ignore (e.g., '.git,__pycache__,target')
"""

import os
import argparse
from io import StringIO

# Global counters
file_count = 0
dir_count = 0


def print_tree(root: str, prefix: str = "", depth_limit: int | None = None,
               ignore_patterns: list[str] | None = None, output_buffer=None) -> None:
    """Recursively print the directory tree and update global counters."""
    global file_count, dir_count

    if depth_limit is not None and depth_limit < 0:
        return

    try:
        entries = sorted(os.listdir(root))
    except PermissionError:
        return

    entries = [e for e in entries if not any(p in e for p in (ignore_patterns or []))]
    total = len(entries)

    for idx, name in enumerate(entries):
        path = os.path.join(root, name)
        connector = "└── " if idx == total - 1 else "├── "
        line = prefix + connector + name
        print(line)
        if output_buffer is not None:
            output_buffer.write(line + "\n")

        if os.path.isdir(path):
            dir_count += 1
            extension = "    " if idx == total - 1 else "│   "
            new_prefix = prefix + extension
            print_tree(
                path,
                prefix=new_prefix,
                depth_limit=None if depth_limit is None else depth_limit - 1,
                ignore_patterns=ignore_patterns,
                output_buffer=output_buffer,
            )
        else:
            file_count += 1


def main():
    parser = argparse.ArgumentParser(description="Generate hierarchical repo structure.")
    parser.add_argument(
        "-d", "--depth", type=int, default=None,
        help="Maximum depth to recurse (default: unlimited)"
    )
    parser.add_argument(
        "-i", "--ignore", type=str,
        default=".git,__pycache__,venv,dist,build,target,.mypy_cache",
        help="Comma-separated substrings of folder/file names to ignore"
    )
    parser.add_argument(
        "-o", "--output", type=str, default="repo_structure.txt",
        help="Output file name (default: repo_structure.txt)"
    )
    args = parser.parse_args()

    ignore_patterns = [x.strip() for x in args.ignore.split(",") if x.strip()]
    output_path = os.path.join(os.getcwd(), args.output)

    buffer = StringIO()
    root = os.getcwd()

    print(".")
    buffer.write(".\n")
    print_tree(root, depth_limit=args.depth, ignore_patterns=ignore_patterns, output_buffer=buffer)

    summary = f"\nTotal: {dir_count} directories, {file_count} files"
    print(summary)
    buffer.write(summary + "\n")

    # Write to file
    with open(output_path, "w", encoding="utf-8") as f:
        f.write(buffer.getvalue())

    print(f"\nRepository structure saved to: {output_path}")


if __name__ == "__main__":
    main()
