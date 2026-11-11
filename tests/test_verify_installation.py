"""Offline unit tests for the pure parts of scripts/verify-installation.py."""
import hashlib
import importlib.util
import io
import sys
import tarfile
import unittest
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    'verify_installation',
    Path(__file__).resolve().parents[1] / 'scripts' / 'verify-installation.py')
vi = importlib.util.module_from_spec(_spec)
sys.modules['verify_installation'] = vi
_spec.loader.exec_module(vi)

FAKE_INSTALLER = '''#!/bin/sh
APP_NAME="dotnet-crap"
APP_VERSION="0.1.0"
if [ -n "${DOTNET_CRAP_DOWNLOAD_URL:-}" ]; then
    ARTIFACT_DOWNLOAD_URLS="$DOTNET_CRAP_DOWNLOAD_URL"
elif [ -n "${INSTALLER_DOWNLOAD_URL:-}" ]; then
    ARTIFACT_DOWNLOAD_URLS="$INSTALLER_DOWNLOAD_URL"
fi
'''

FAKE_INSTALLER_NO_PRODUCT_OVERRIDE = FAKE_INSTALLER.replace(
    'if [ -n "${DOTNET_CRAP_DOWNLOAD_URL:-}" ]; then\n'
    '    ARTIFACT_DOWNLOAD_URLS="$DOTNET_CRAP_DOWNLOAD_URL"\n'
    'elif', 'if false; then\n    :\nelif')

FAKE_FORMULA = '''class DotnetCrap < Formula
  desc "Score C# callables"
  homepage "https://github.com/RuiCostafrg/dotnet-crap"
  version "0.1.0"
  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/RuiCostafrg/dotnet-crap/releases/download/v0.1.0/dotnet-crap-aarch64-apple-darwin.tar.xz"
      sha256 "1111111111111111111111111111111111111111111111111111111111111111"
    end
  end
  if OS.linux?
    if Hardware::CPU.intel?
      url "https://github.com/RuiCostafrg/dotnet-crap/releases/download/v0.1.0/dotnet-crap-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "2222222222222222222222222222222222222222222222222222222222222222"
    end
  end
  license "MIT"

  def install
    bin.install "dotnet-crap"
  end
end
'''


def make_archive(members):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w:xz') as bundle:
        for name, data in members:
            info = tarfile.TarInfo(name)
            info.size = len(data)
            bundle.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


def tar_with_binary(binary_name='dotnet-crap', data=b'\x7fELFbinary', extra=(('LICENSE', b'MIT'),)):
    return make_archive([(f'dotnet-crap-0.1.0-x86_64-unknown-linux-gnu/{binary_name}', data), *extra])


class VersionValidationTests(unittest.TestCase):
    def test_accepts_semver_and_prerelease(self):
        self.assertEqual(vi.validate_version('0.1.0'), '0.1.0')
        self.assertEqual(vi.validate_version('1.2.3-rc.1'), '1.2.3-rc.1')

    def test_rejects_non_semver(self):
        for bad in ('0.1', 'latest', 'v0.1.0', '0.1.0.0', ''):
            with self.assertRaises(ValueError):
                vi.validate_version(bad)

    def test_target_choices(self):
        self.assertEqual(sorted(vi.TARGETS), sorted({
            'aarch64-apple-darwin', 'x86_64-apple-darwin',
            'aarch64-unknown-linux-gnu', 'x86_64-unknown-linux-gnu',
            'aarch64-unknown-linux-musl', 'x86_64-unknown-linux-musl'}))
        self.assertNotIn('x86_64-pc-windows-msvc', vi.TARGETS)


class ChecksumTests(unittest.TestCase):
    def test_parse_valid(self):
        digest = hashlib.sha256(b'x').hexdigest()
        self.assertEqual(vi.parse_checksum(f'{digest}  dotnet-crap-x.tar.xz\n', 'dotnet-crap-x.tar.xz'), digest)

    def test_parse_binary_marker(self):
        digest = hashlib.sha256(b'x').hexdigest()
        self.assertEqual(vi.parse_checksum(f'{digest} *dotnet-crap-x.tar.xz\n', 'dotnet-crap-x.tar.xz'), digest)

    def test_parse_rejects_malformed(self):
        good = hashlib.sha256(b'x').hexdigest()
        good = hashlib.sha256(b'x').hexdigest()
        bad_name = 'dotnet-crap-x.tar.xz'
        for text, name in [
            (f'{good} other-name\n', bad_name),
            (f'{good}\n', bad_name),
            (f'ZZZ{bad_name} {good}\n', bad_name),
            (f'{good} {bad_name} extra\n', bad_name),
        ]:
            with self.assertRaises(ValueError):
                vi.parse_checksum(text, name)

    def test_verify_checksum_mismatch(self):
        digest = hashlib.sha256(b'expected').hexdigest()
        with self.assertRaises(ValueError):
            vi.verify_checksum(b'actual', 'dotnet-crap-x.tar.xz', digest)
        vi.verify_checksum(b'expected', 'dotnet-crap-x.tar.xz', digest)


class ArchiveTests(unittest.TestCase):
    def test_extract_binary(self):
        data = tar_with_binary()
        self.assertEqual(vi.extract_binary(data), b'\x7fELFbinary')

    def test_extract_rejects_missing_binary(self):
        with self.assertRaises(ValueError):
            vi.extract_binary(tar_with_binary(binary_name='other'))

    def test_extract_rejects_ambiguous_binary(self):
        with self.assertRaises(ValueError):
            vi.extract_binary(tar_with_binary(extra=(
                ('nested/dotnet-crap', b'\x7fELFother'),
                ('LICENSE', b'MIT'))))

    def test_archive_name(self):
        self.assertEqual(vi.archive_name('aarch64-apple-darwin'),
                         'dotnet-crap-aarch64-apple-darwin.tar.xz')
        with self.assertRaises(ValueError):
            vi.archive_name('x86_64-pc-windows-msvc')


class InstallerTests(unittest.TestCase):
    def test_metadata_and_prefix(self):
        prefix = vi.installer_metadata(FAKE_INSTALLER, '0.1.0')
        self.assertEqual(prefix, 'DOTNET_CRAP')

    def test_metadata_rejects_version_mismatch(self):
        with self.assertRaises(ValueError):
            vi.installer_metadata(FAKE_INSTALLER, '0.2.0')

    def test_metadata_rejects_other_product(self):
        with self.assertRaises(ValueError):
            vi.installer_metadata(FAKE_INSTALLER.replace('dotnet-crap', 'ts-crap'), '0.1.0')

    def test_download_override_prefers_product_variable(self):
        self.assertEqual(vi.download_override('DOTNET_CRAP', FAKE_INSTALLER), 'DOTNET_CRAP_DOWNLOAD_URL')

    def test_download_override_falls_back_to_generic(self):
        prefix = vi.installer_metadata(FAKE_INSTALLER_NO_PRODUCT_OVERRIDE, '0.1.0')
        self.assertEqual(vi.download_override(prefix, FAKE_INSTALLER_NO_PRODUCT_OVERRIDE),
                         'INSTALLER_DOWNLOAD_URL')


class FormulaTests(unittest.TestCase):
    def test_version(self):
        self.assertEqual(vi.formula_version(FAKE_FORMULA), '0.1.0')

    def test_rewrite_to_file_urls(self):
        rewritten = vi.rewrite_formula(FAKE_FORMULA, '0.1.0', 'file:///tmp/staged')
        self.assertIn('url "file:///tmp/staged/dotnet-crap-aarch64-apple-darwin.tar.xz"', rewritten)
        self.assertIn('url "file:///tmp/staged/dotnet-crap-x86_64-unknown-linux-gnu.tar.xz"', rewritten)
        self.assertNotIn('releases/download', rewritten)
        # sha256 lines and everything else are unchanged.
        self.assertEqual(vi.formula_targets(rewritten),
                         {'aarch64-apple-darwin', 'x86_64-unknown-linux-gnu'})
        self.assertEqual(
            [line for line in rewritten.splitlines() if 'sha256' in line],
            [line for line in FAKE_FORMULA.splitlines() if 'sha256' in line])
        self.assertIn('version "0.1.0"', rewritten)

    def test_rewrite_rejects_version_mismatch(self):
        with self.assertRaises(ValueError):
            vi.rewrite_formula(FAKE_FORMULA, '0.2.0', 'file:///tmp/staged')

    def test_rewrite_rejects_unexpected_url(self):
        with self.assertRaises(ValueError):
            vi.rewrite_formula(FAKE_FORMULA.replace('RuiCostafrg/dotnet-crap/releases',
                                                    'RuiCostafrg/other/releases'), '0.1.0', 'file:///tmp/x')

    def test_rewrite_rejects_missing_checksums(self):
        stripped = '\n'.join(line for line in FAKE_FORMULA.splitlines()
                             if 'sha256' not in line) + '\n'
        with self.assertRaises(ValueError):
            vi.rewrite_formula(stripped, '0.1.0', 'file:///tmp/x')

    def test_formula_targets(self):
        self.assertEqual(vi.formula_targets(FAKE_FORMULA),
                         {'aarch64-apple-darwin', 'x86_64-unknown-linux-gnu'})

    def test_formula_targets_after_rewrite_into_a_dotnet_crap_named_directory(self):
        # The staging directory itself is named dotnet-crap-install-*.
        rewritten = vi.rewrite_formula(FAKE_FORMULA, '0.1.0', 'file:///tmp/dotnet-crap-install-x1/assets')
        self.assertEqual(vi.formula_targets(rewritten),
                         {'aarch64-apple-darwin', 'x86_64-unknown-linux-gnu'})


class InstalledTargetTests(unittest.TestCase):
    def setUp(self):
        self.gnu = b'\x7fELFgnu'
        self.musl = b'\x7fELFmusl'
        self.digests = {
            'x86_64-unknown-linux-gnu': hashlib.sha256(self.gnu).hexdigest(),
            'x86_64-unknown-linux-musl': hashlib.sha256(self.musl).hexdigest(),
            'aarch64-unknown-linux-gnu': hashlib.sha256(b'\x7fELFarm-gnu').hexdigest(),
            'aarch64-apple-darwin': hashlib.sha256(b'\x7fMACHO').hexdigest(),
        }

    def test_candidates_share_cpu_and_platform(self):
        self.assertEqual(vi.candidate_targets('x86_64-unknown-linux-gnu', self.digests),
                         {'x86_64-unknown-linux-gnu', 'x86_64-unknown-linux-musl'})
        self.assertEqual(vi.candidate_targets('aarch64-apple-darwin', self.digests),
                         {'aarch64-apple-darwin'})
        with self.assertRaises(ValueError):
            vi.candidate_targets('x86_64-pc-windows-msvc', self.digests)

    def test_confirm_identifies_musl_install(self):
        digest = hashlib.sha256(self.musl).hexdigest()
        self.assertEqual(vi.confirm_installed_target(self.digests, digest, 'x86_64-unknown-linux-gnu'),
                         'x86_64-unknown-linux-musl')

    def test_confirm_restricts_to_formula_targets(self):
        digest = hashlib.sha256(self.musl).hexdigest()
        with self.assertRaises(ValueError):
            vi.confirm_installed_target(self.digests, digest, 'x86_64-unknown-linux-gnu',
                                        {'x86_64-unknown-linux-gnu'})

    def test_confirm_rejects_unknown_binary(self):
        with self.assertRaises(ValueError):
            vi.confirm_installed_target(self.digests, hashlib.sha256(b'other').hexdigest(),
                                        'x86_64-unknown-linux-gnu')


class ArgumentTests(unittest.TestCase):
    def test_requires_target_and_installer(self):
        with self.assertRaises(SystemExit):
            vi.build_parser().parse_args(['--assets', 'a', '--version', '0.1.0'])

    def test_rejects_unknown_target(self):
        with self.assertRaises(SystemExit):
            vi.build_parser().parse_args(
                ['--assets', 'a', '--version', '0.1.0', '--target', 'x86_64-pc-windows-msvc',
                 '--installer', 'shell'])

    def test_accepts_offline_arguments(self):
        args = vi.build_parser().parse_args(
            ['--assets', 'a', '--version', '0.1.0', '--target', 'aarch64-apple-darwin',
             '--installer', 'homebrew', '--fixture', 'examples/Fixture'])
        self.assertEqual(args.installer, 'homebrew')
        self.assertEqual(args.fixture, Path('examples/Fixture'))
        self.assertIsNone(args.previous_assets)


if __name__ == '__main__':
    unittest.main()
