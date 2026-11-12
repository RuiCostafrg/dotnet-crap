"""The reviewed cargo-dist downloader is pinned, checksummed and archive-safe."""
import hashlib
import importlib.util
import io
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    """Load a tooling script without invoking its command-line entry point."""
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


install_dist = module("install_dist", "scripts/install-dist.py")

TARGETS = {
    "aarch64-apple-darwin": "tar.xz",
    "x86_64-apple-darwin": "tar.xz",
    "aarch64-unknown-linux-gnu": "tar.xz",
    "x86_64-unknown-linux-gnu": "tar.xz",
    "x86_64-pc-windows-msvc": "zip",
}


def tarball(member_name, data, kind=tarfile.REGTYPE):
    """Build an in-memory tar.xz with one member of the requested kind."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:xz") as bundle:
        info = tarfile.TarInfo(member_name)
        if kind == tarfile.REGTYPE:
            info.size = len(data)
            bundle.addfile(info, io.BytesIO(data))
        else:
            info.type = kind
            info.linkname = "target"
            bundle.addfile(info)
    return buffer.getvalue()


class InstallDistTests(unittest.TestCase):
    def test_archive_table_is_pinned_and_well_formed(self):
        """Every supported host maps to one extension and a 64-digit SHA-256."""
        self.assertEqual(install_dist.VERSION, "0.33.1")
        self.assertEqual(set(install_dist.ARCHIVES), set(TARGETS))
        for target, extension in TARGETS.items():
            with self.subTest(target=target):
                actual_extension, digest = install_dist.ARCHIVES[target]
                self.assertEqual(actual_extension, extension)
                self.assertRegex(digest, r"^[0-9a-f]{64}$")

    @mock.patch("urllib.request.urlopen")
    def test_archive_with_wrong_digest_is_rejected(self, urlopen):
        """A downloaded archive that does not match the pinned digest is never unpacked."""
        payload = tarball("cargo-dist-test-host/dist", b"binary")
        urlopen.return_value = io.BytesIO(payload)
        with mock.patch.dict(install_dist.ARCHIVES, {"test-host": ("tar.xz", "0" * 64)}):
            with tempfile.TemporaryDirectory() as temporary:
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    install_dist.install("test-host", Path(temporary))
                self.assertEqual(list(Path(temporary).iterdir()), [])

    @mock.patch("urllib.request.urlopen")
    def test_non_regular_member_is_rejected(self, urlopen):
        """A symlink in place of the dist executable is refused."""
        payload = tarball("cargo-dist-test-host/dist", b"", kind=tarfile.SYMTYPE)
        urlopen.return_value = io.BytesIO(payload)
        digest = hashlib.sha256(payload).hexdigest()
        with mock.patch.dict(install_dist.ARCHIVES, {"test-host": ("tar.xz", digest)}):
            with tempfile.TemporaryDirectory() as temporary:
                with self.assertRaisesRegex(ValueError, "not a regular file"):
                    install_dist.install("test-host", Path(temporary))
                self.assertEqual(list(Path(temporary).iterdir()), [])

    @mock.patch("urllib.request.urlopen")
    def test_verified_archive_is_extracted_executable(self, urlopen):
        """A matching archive yields an executable with the expected contents."""
        payload = tarball("cargo-dist-test-host/dist", b"binary")
        urlopen.return_value = io.BytesIO(payload)
        digest = hashlib.sha256(payload).hexdigest()
        with mock.patch.dict(install_dist.ARCHIVES, {"test-host": ("tar.xz", digest)}):
            with tempfile.TemporaryDirectory() as temporary:
                executable = install_dist.install("test-host", Path(temporary))
                self.assertEqual(executable.name, "dist")
                self.assertEqual(executable.read_bytes(), b"binary")
                self.assertEqual(executable.stat().st_mode & 0o111, 0o111)
        self.assertEqual(urlopen.call_args.args[0],
                         "https://github.com/axodotdev/cargo-dist/releases/download/"
                         "v0.33.1/cargo-dist-test-host.tar.xz")


if __name__ == "__main__":
    unittest.main()
