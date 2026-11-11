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

VERSION = "0.33.1"
ARCHIVES = {
    "aarch64-apple-darwin": ("tar.xz", "b8bf041a38e9e54ecf41eee2574ca6644043796620bb0ee8340e9c0acbaaae8f"),
    "x86_64-apple-darwin": ("tar.xz", "e9dc303035fbb2f44295948e0a076f1d1d218a0328e9b84e16955863bba92bf4"),
    "aarch64-unknown-linux-gnu": ("tar.xz", "e883f4c6c17f2458d6ced950d978c04b45423ed3a5fdbebcb8f8de568389e1c1"),
    "x86_64-unknown-linux-gnu": ("tar.xz", "da68e159d5fed1ee61b00284da444e719927dbd0b7d0e2c34e57a04414453cf9"),
    "x86_64-pc-windows-msvc": ("zip", "79dfcce84e6a8e69210f11fc23d445c3aac7af6a29e0e981867f053c0a869316"),
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
