#!/usr/bin/env python3
"""Install the reviewed cargo-dist binary; never execute a downloaded installer."""
import argparse
import hashlib
import io
import os
from pathlib import Path
import tarfile
import urllib.request
import zipfile

VERSION = "0.34.0"
ARCHIVES = {
    "aarch64-apple-darwin": ("tar.xz", "d1eec30e63eed23e5a5bd48fe96eae314799811bffca254ec1d2264638ba1890"),
    "x86_64-apple-darwin": ("tar.xz", "6aa89bb1e7c18f51ae8b148ce2fc6ab00ac46ed8173d4f2ed9afa12101218773"),
    "aarch64-unknown-linux-gnu": ("tar.xz", "58efb33edf01342875bfd0c3ce2befa9d73075579ca1b3e5dd75b98c88f92e83"),
    "x86_64-unknown-linux-gnu": ("tar.xz", "7fee44ff3ff71ed0064b32d4ec62fee6b58bd37c5887b12a86aedddc5af0c7b5"),
    "x86_64-pc-windows-msvc": ("zip", "7bb53fbc998321f77a3bbb044084a52747c8ed041cf6d6a604802aa57b9c80c6"),
}

def install(target, directory):
    extension, digest = ARCHIVES[target]
    name = f"cargo-dist-{target}.{extension}"
    url = f"https://github.com/axodotdev/cargo-dist/releases/download/v{VERSION}/{name}"
    with urllib.request.urlopen(url, timeout=120) as response:
        archive = response.read()
    if hashlib.sha256(archive).hexdigest() != digest:
        raise ValueError("cargo-dist checksum mismatch")
    binary = "dist.exe" if extension == "zip" else "dist"
    expected = binary if extension == "zip" else f"cargo-dist-{target}/{binary}"
    if extension == "zip":
        with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
            data = bundle.read(expected)
    else:
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:xz") as bundle:
            member = bundle.getmember(expected)
            if not member.isfile():
                raise ValueError("cargo-dist executable is not a regular file")
            data = bundle.extractfile(member).read()
    directory.mkdir(parents=True, exist_ok=True)
    executable = directory / binary
    executable.write_bytes(data)
    executable.chmod(0o755)
    return executable

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", choices=ARCHIVES)
    args = parser.parse_args()
    directory = Path(os.environ["RUNNER_TEMP"]) / "reviewed-dist"
    print(install(args.target, directory))
    with open(os.environ["GITHUB_PATH"], "a", encoding="utf-8") as output:
        print(directory, file=output)
