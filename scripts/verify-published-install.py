#!/usr/bin/env python3
"""Exercise real public installers using authenticated release assets as the oracle."""
import argparse
import http.client
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
NUGET_ORG = 'https://api.nuget.org/v3/index.json'
NUGET_FLAT_CONTAINER = 'https://api.nuget.org/v3-flatcontainer'
NUGET_SIGNATURE = '.signature.p7s'
# NuGet.org can take several minutes to expose a new version everywhere.
NUGET_ATTEMPTS = 20
NUGET_RETRY_SECONDS = 30
# Bound decompression: a package may add only a small repository signature.
NUGET_SIGNATURE_LIMIT = 1 << 20


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), ROOT / 'scripts' / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run(command, *, cwd=None, env=None, expected=0):
    env = dict(os.environ if env is None else env)
    for name in ('GH_TOKEN', 'GITHUB_TOKEN'):
        env.pop(name, None)
    # Fixed argument vectors: never execute caller-supplied command text.
    # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit
    result = subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True, timeout=900)
    if result.returncode != expected:
        raise ValueError(f'{command}: expected {expected}, got {result.returncode}\n'
                         f'{result.stdout[-8000:]}\n{result.stderr[-8000:]}')
    return result.stdout.strip()


def archive_binary(assets, target):
    return load('stage-dotnet-tool').executable(assets, target)


def check_bytes(executable, assets, targets):
    executable = executable.resolve()
    if executable.read_bytes() not in [archive_binary(assets, target) for target in targets]:
        raise ValueError(f'{executable}: installed bytes differ from authenticated archives')


def check_binary(executable, assets, targets, version, fixture):
    executable = executable.resolve()
    check_bytes(executable, assets, targets)
    # Installed programs do not receive GitHub credentials even on authenticated runners.
    env = {key: value for key, value in os.environ.items() if key not in ('GH_TOKEN', 'GITHUB_TOKEN')}
    if run([str(executable), '--version'], env=env) != f'dotnet-crap {version}':
        raise ValueError('installed version differs from expected release')
    run([str(executable), '--root', str(fixture), '--threshold', '100', '--fail-on-empty'], env=env)
    run([str(executable), '--root', str(fixture), '--threshold', '0'], env=env, expected=1)


def shell(args, work):
    home = work / 'home'
    home.mkdir()
    prefix = home / '.local/bin'
    env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'),
               CARGO_DIST_FORCE_INSTALL_DIR=str(prefix), DOTNET_CRAP_INSTALL_DIR=str(prefix),
               DOTNET_CRAP_NO_MODIFY_PATH='1', INSTALLER_NO_MODIFY_PATH='1')
    # This is the actual authenticated published installer: no URL rewriting.
    run(['sh', str(args.assets / 'dotnet-crap-installer.sh')], env=env)
    targets = [args.target]
    if args.target.endswith('-linux-gnu'):
        targets.append(args.target[:-3] + 'musl')
    check_binary(home / '.local/bin/dotnet-crap', args.assets, targets, args.version, args.fixture)


def homebrew(args):
    run(['brew', 'tap', 'RuiCostafrg/tap'])
    tap = Path(run(['brew', '--repository', 'RuiCostafrg/tap']))
    # Verify the live tap contains precisely the authenticated released formula
    # before allowing Homebrew to interpret it. Older releases fail explicitly
    # if the tap has advanced; this check never substitutes a local formula.
    if (tap / 'Formula/dotnet-crap.rb').read_bytes() != (args.assets / 'dotnet-crap.rb').read_bytes():
        raise ValueError('live tap formula differs from the expected released formula')
    run(['brew', 'install', 'RuiCostafrg/tap/dotnet-crap'])
    prefix = Path(run(['brew', '--prefix', 'RuiCostafrg/tap/dotnet-crap']))
    check_binary(prefix / 'bin/dotnet-crap', args.assets, [args.target], args.version, args.fixture)


def download(url, destination, *, attempts=NUGET_ATTEMPTS, delay=NUGET_RETRY_SECONDS):
    """Fetch one file, retrying because nuget.org indexing lags behind the release."""
    destination = Path(destination)
    for attempt in range(1, attempts + 1):
        try:
            with urllib.request.urlopen(url, timeout=120) as response, destination.open('wb') as stream:
                shutil.copyfileobj(response, stream)
            return destination
        except (OSError, http.client.HTTPException) as error:
            if attempt == attempts:
                raise ValueError(f'cannot download {url}: {error}') from error
            time.sleep(delay)


def zip_contents(path, *, signature, limit=None):
    """Return entry names and bytes; only the NuGet.org repository signature may appear late.

    With a limit, the declared uncompressed sizes are checked before anything is
    decompressed, and reads stop at the declared size, so a crafted archive cannot
    exhaust memory.
    """
    try:
        with zipfile.ZipFile(path) as archive:
            infos = archive.infolist()
            names = [info.filename for info in infos]
            if len(names) != len(set(names)):
                raise ValueError(f'{path}: duplicate ZIP entries')
            if limit is not None and sum(info.file_size for info in infos) > limit:
                raise ValueError(f'{path}: uncompressed size exceeds the release package by too much')
            contents = {}
            for info in infos:
                with archive.open(info) as entry:
                    data = entry.read(info.file_size + 1)
                if len(data) != info.file_size:
                    raise ValueError(f'{path}: entry {info.filename} does not match its declared size')
                contents[info.filename] = data
    except (OSError, zipfile.BadZipFile) as error:
        raise ValueError(f'{path}: not a readable NuGet package: {error}') from error
    if NUGET_SIGNATURE in contents:
        if not signature:
            raise ValueError(f'{path}: unexpected {NUGET_SIGNATURE} in the release asset')
        del contents[NUGET_SIGNATURE]
    return contents


def compare_packages(released, published):
    """NuGet.org adds a repository signature; compare ZIP entries instead of whole bytes."""
    expected = zip_contents(released, signature=False)
    limit = sum(len(data) for data in expected.values()) + NUGET_SIGNATURE_LIMIT
    actual = zip_contents(published, signature=True, limit=limit)
    missing = sorted(set(expected) - set(actual))
    extra = sorted(set(actual) - set(expected))
    if missing or extra:
        raise ValueError(f'{Path(published).name}: ZIP entries differ from the release asset; '
                         f'missing {missing}; unexpected {extra}')
    changed = sorted(name for name in expected if expected[name] != actual[name])
    if changed:
        raise ValueError(f'{Path(published).name}: ZIP entry bytes differ from the release asset: {changed}')


def tool(args, work, source=None):
    """Install the released package from an isolated local feed, or from one given source URL."""
    if source is None:
        feed = work / 'feed'
        feed.mkdir()
        shutil.copyfile(args.assets / f'dotnet-crap.{args.version}.nupkg',
                        feed / f'dotnet-crap.{args.version}.nupkg')
        package_sources = ''
        add_source = ['--add-source', str(feed)]
    else:
        package_sources = f'<add key="nuget.org" value="{source}" />'
        add_source = []
    config = work / 'NuGet.Config'
    config.write_text(f'<configuration><packageSources><clear />{package_sources}</packageSources></configuration>')
    destination = work / 'tool'
    env = {key: value for key, value in os.environ.items() if key not in ('GH_TOKEN', 'GITHUB_TOKEN')}
    env['NUGET_PACKAGES'] = str(work / 'cache')
    command = ['dotnet', 'tool', 'install', 'dotnet-crap', '--tool-path', str(destination),
               '--configfile', str(config), *add_source, '--version', args.version]
    # A registry may serve the package before its version metadata resolves.
    attempts = 1 if source is None else NUGET_ATTEMPTS
    for attempt in range(1, attempts + 1):
        try:
            run(command, env=env)
            break
        except subprocess.CalledProcessError:
            if attempt == attempts:
                raise
            shutil.rmtree(destination, ignore_errors=True)
            time.sleep(NUGET_RETRY_SECONDS)
    if source is not None:
        check_restored(args, destination)
    native_name = 'dotnet-crap.exe' if args.rid == 'win-x64' else 'dotnet-crap'
    copies = list(destination.rglob(f'native/{args.rid}/{native_name}'))
    if len(copies) != 1:
        raise ValueError(f'expected one installed native executable for {args.rid}, got {copies}')
    # NuGet does not preserve Unix executable bits. The launcher deliberately
    # copies the stored payload into a private executable file; compare stored
    # bytes here and exercise execution through the supported launcher below.
    check_bytes(copies[0], args.assets, [args.target])
    shim = destination / native_name
    if run([str(shim), '--version'], env=env) != f'dotnet-crap {args.version}':
        raise ValueError('installed .NET launcher reports wrong version')
    run([str(shim), '--root', str(args.fixture), '--threshold', '100', '--fail-on-empty'], env=env)
    run([str(shim), '--root', str(args.fixture), '--threshold', '0'], env=env, expected=1)


def check_restored(args, destination):
    """Compare what was actually restored with the release package before running it.

    The registry may answer the install differently from an earlier download, so
    the installed package and every extracted file are checked, not just the
    native payload.
    """
    version = args.version.lower()
    released = args.assets / f'dotnet-crap.{args.version}.nupkg'
    store = destination / '.store' / 'dotnet-crap' / version / 'dotnet-crap' / version
    restored = store / f'dotnet-crap.{version}.nupkg'
    if not restored.is_file():
        raise ValueError(f'restored package not found at {restored}')
    compare_packages(released, restored)
    with zipfile.ZipFile(released) as archive:
        for info in archive.infolist():
            if info.is_dir() or not info.filename.startswith('tools/'):
                continue
            extracted = store / info.filename
            if not extracted.is_file() or extracted.read_bytes() != archive.read(info):
                raise ValueError(f'extracted {info.filename} differs from the release package')


def nuget(args, work):
    # The flat-container path segment and file name use the lowercase version.
    version = args.version.lower()
    package = work / f'dotnet-crap.{version}.nupkg'
    download(f'{NUGET_FLAT_CONTAINER}/dotnet-crap/{version}/dotnet-crap.{version}.nupkg', package)
    compare_packages(args.assets / f'dotnet-crap.{args.version}.nupkg', package)
    tool(args, work, source=NUGET_ORG)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('channel', choices=('shell', 'homebrew', 'tool', 'nuget'))
    parser.add_argument('--assets', required=True, type=Path)
    parser.add_argument('--version', required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--target')
    parser.add_argument('--rid')
    args = parser.parse_args()
    load('verify-published-inputs').validate(args.version, args.source_sha)
    args.assets = args.assets.resolve()
    args.fixture = ROOT / 'examples/Fixture'
    if not args.target:
        parser.error('--target is required for this channel')
    if args.channel == 'tool' and not args.rid:
        parser.error('--rid is required for tool installation')
    if args.channel == 'nuget' and not args.rid:
        parser.error('--rid is required for nuget installation')
    for name in ('GH_TOKEN', 'GITHUB_TOKEN'):
        os.environ.pop(name, None)
    with tempfile.TemporaryDirectory(prefix='published-install-') as temporary:
        work = Path(temporary).resolve()
        if args.channel == 'shell':
            shell(args, work)
        elif args.channel == 'homebrew':
            homebrew(args)
        elif args.channel == 'nuget':
            nuget(args, work)
        else:
            tool(args, work)
    print(json.dumps({'channel': args.channel, 'version': args.version, 'source_sha': args.source_sha,
                      'target': args.target, 'rid': args.rid, 'status': 'passed'}))


if __name__ == '__main__':
    main()
