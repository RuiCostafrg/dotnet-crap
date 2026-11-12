#!/usr/bin/env python3
"""Exercise real public installers using authenticated release assets as the oracle."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


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


def check_binary(executable, assets, targets, version, fixture):
    executable = executable.resolve()
    if executable.read_bytes() not in [archive_binary(assets, target) for target in targets]:
        raise ValueError(f'{executable}: installed bytes differ from authenticated archives')
    # Installed programs do not receive GitHub credentials even on authenticated runners.
    env = {key: value for key, value in os.environ.items() if key not in ('GH_TOKEN', 'GITHUB_TOKEN')}
    if run([str(executable), '--version'], env=env) != f'dotnet-crap {version}':
        raise ValueError('installed version differs from expected release')
    run([str(executable), '--root', str(fixture), '--threshold', '100', '--fail-on-empty'], env=env)
    run([str(executable), '--root', str(fixture), '--threshold', '0'], env=env, expected=1)


def registry(args, work):
    check = load('verify-npm-install')
    # Reuse its real package-manager execution and byte checks, replacing only
    # local-tarball manifests and blocked registries with exact public versions.
    check.ENVIRONMENT.update(npm_config_registry='https://registry.npmjs.org/',
                             YARN_NPM_REGISTRY_SERVER='https://registry.npmjs.org/',
                             BUN_CONFIG_REGISTRY='https://registry.npmjs.org/',
                             npm_config_offline='false')
    check.NPM_OFFLINE = {}
    check.manifest = lambda *_: {'name': 'published-dotnet-crap-check', 'version': '0.0.0',
                                'private': True, 'dependencies': {'dotnet-crap': args.version}}
    check.workspace_files = lambda manager, *_: (
        {'yarn.lock': '', '.yarnrc.yml': 'nodeLinker: node-modules\n'} if manager == 'yarn-node-modules'
        else {'yarn.lock': ''} if manager == 'yarn' else {})
    for manager in ('npm', 'pnpm', 'yarn', 'yarn-node-modules', 'bun'):
        check.verify(manager, args.version, args.assets, work, args.fixture, work,
                     check.host_target())
        print(f'{manager}: installed exact registry version and matched authenticated executable')


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


def tool(args, work):
    feed = work / 'feed'
    feed.mkdir()
    shutil.copyfile(args.assets / f'dotnet-crap.{args.version}.nupkg', feed / f'dotnet-crap.{args.version}.nupkg')
    config = work / 'NuGet.Config'
    config.write_text('<configuration><packageSources><clear /></packageSources></configuration>')
    destination = work / 'tool'
    env = {key: value for key, value in os.environ.items() if key not in ('GH_TOKEN', 'GITHUB_TOKEN')}
    env['NUGET_PACKAGES'] = str(work / 'cache')
    run(['dotnet', 'tool', 'install', 'dotnet-crap', '--tool-path', str(destination),
         '--configfile', str(config), '--add-source', str(feed), '--version', args.version], env=env)
    native_name = 'dotnet-crap.exe' if args.rid == 'win-x64' else 'dotnet-crap'
    copies = list(destination.rglob(f'native/{args.rid}/{native_name}'))
    if len(copies) != 1:
        raise ValueError(f'expected one installed native executable for {args.rid}, got {copies}')
    check_binary(copies[0], args.assets, [args.target], args.version, args.fixture)
    shim = destination / native_name
    if run([str(shim), '--version'], env=env) != f'dotnet-crap {args.version}':
        raise ValueError('installed .NET launcher reports wrong version')
    run([str(shim), '--root', str(args.fixture), '--threshold', '100', '--fail-on-empty'], env=env)
    run([str(shim), '--root', str(args.fixture), '--threshold', '0'], env=env, expected=1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('channel', choices=('registry', 'shell', 'homebrew', 'tool'))
    parser.add_argument('--assets', required=True, type=Path)
    parser.add_argument('--version', required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--target')
    parser.add_argument('--rid')
    args = parser.parse_args()
    load('verify-published-inputs').validate(args.version, args.source_sha)
    args.assets = args.assets.resolve()
    args.fixture = ROOT / 'examples/Fixture'
    if args.channel != 'registry' and not args.target:
        parser.error('--target is required for this channel')
    if args.channel == 'tool' and not args.rid:
        parser.error('--rid is required for tool installation')
    for name in ('GH_TOKEN', 'GITHUB_TOKEN'):
        os.environ.pop(name, None)
    with tempfile.TemporaryDirectory(prefix='published-install-') as temporary:
        work = Path(temporary).resolve()
        if args.channel == 'registry':
            registry(args, work)
        elif args.channel == 'shell':
            shell(args, work)
        elif args.channel == 'homebrew':
            homebrew(args)
        else:
            tool(args, work)
    print(json.dumps({'channel': args.channel, 'version': args.version, 'source_sha': args.source_sha,
                      'target': args.target, 'rid': args.rid, 'status': 'passed'}))


if __name__ == '__main__':
    main()
