#!/usr/bin/env python3
"""Validate published verification coordinates before using them in commands."""
import argparse
import re

VERSION = re.compile(r'(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?\Z')
SHA = re.compile(r'[0-9a-f]{40}\Z')


def validate(version: str, sha: str) -> None:
    match = VERSION.fullmatch(version)
    if not match or (match.group(1) and any(
        part.isdigit() and len(part) > 1 and part[0] == '0'
        for part in match.group(1).split('.')
    )):
        raise ValueError('version must be a canonical SemVer without v or build metadata')
    if not SHA.fullmatch(sha):
        raise ValueError('source SHA must be 40 lowercase hexadecimal characters')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('version')
    parser.add_argument('sha')
    args = parser.parse_args()
    try:
        validate(args.version, args.sha)
    except ValueError as error:
        parser.error(str(error))


if __name__ == '__main__':
    main()
