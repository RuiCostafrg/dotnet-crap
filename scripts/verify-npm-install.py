#!/usr/bin/env python3
"""Install the packed dotnet-crap npm packages with npm, pnpm, Yarn or Bun in clean projects and check what runs.

Each manager installs the local tarballs from scripts/npm-packages.py, pinning every
platform package to its tarball through the manager's override field, with lifecycle
scripts disabled. The installed executable must equal the release archive's bytes,
and the commands run through the package manager. Nothing is fetched for the
dotnet-crap packages from a registry: the packed mode points an unreachable registry
at the managers, so any attempt to reach one fails the check.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('npm_packages', ROOT / 'scripts/npm-packages.py')
npm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(npm)

# Yarn runs under two linkers: Plug'n'Play (the default) and node_modules.
MANAGERS = ('npm', 'pnpm', 'yarn', 'yarn-node-modules', 'bun')
# Run an installed bin. `npm exec --no` never downloads, so it cannot fall back to a registry package.
RUN = {'npm': ['npm', 'exec', '--no', '--'], 'pnpm': ['pnpm', 'exec'], 'yarn': ['yarn', 'run'], 'bun': ['bun', 'run']}
# Yarn Plug'n'Play resolves packages only inside `yarn node`.
NODE = {'npm': ['node'], 'pnpm': ['node'], 'yarn': ['yarn', 'node'], 'bun': ['node']}
INSTALL = {
    'npm': ['npm', 'install', '--ignore-scripts', '--no-audit', '--no-fund'],
    'pnpm': ['pnpm', 'install', '--ignore-scripts', '--no-frozen-lockfile'],
    'yarn': ['yarn', 'install'],  # YARN_ENABLE_SCRIPTS=false below
    'bun': ['bun', 'install', '--ignore-scripts'],
}
# An address nothing listens on: any registry request during the packed install fails.
UNREACHABLE_REGISTRY = 'http://127.0.0.1:9/'
# Yarn quarantines npm versions younger than npmMinimalAgeGate (1 day by default from 4.15),
# so a check right after a release would fail. These projects install only dotnet-crap
# and its platform packages, so the gate is off here; users keep Yarn's default.
ENVIRONMENT = {'npm_config_update_notifier': 'false', 'npm_config_fund': 'false',
               'npm_config_registry': UNREACHABLE_REGISTRY,
               'YARN_ENABLE_SCRIPTS': 'false', 'YARN_ENABLE_IMMUTABLE_INSTALLS': 'false',
               'YARN_NPM_MINIMAL_AGE_GATE': '0', 'YARN_NPM_REGISTRY_SERVER': UNREACHABLE_REGISTRY,
               'YARN_ENABLE_TELEMETRY': '0', 'COREPACK_ENABLE_DOWNLOAD_PROMPT': '0',
               'BUN_CONFIG_REGISTRY': UNREACHABLE_REGISTRY}
# npm's offline mode resolves the file: tarballs without looking up the platform package
# names on the registry, which the main package's optionalDependencies would otherwise do.
# Only npm gets it: pnpm reads npm_config_offline too and then skips the platform package.
NPM_OFFLINE = {'npm_config_offline': 'true'}
PRINT_BINARY = "process.stdout.write(require('dotnet-crap').binaryPath())"
PRINT_PACKAGE = "process.stdout.write(require.resolve('dotnet-crap/package.json'))"


def base(manager):
    return 'yarn' if manager.startswith('yarn') else manager


def host_target(system=None, machine=None):
    system = system or platform.system()
    machine = (machine or platform.machine()).lower()
    cpu = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}.get(machine)
    target = {('Linux', 'x86_64'): 'x86_64-unknown-linux-musl', ('Linux', 'aarch64'): 'aarch64-unknown-linux-musl',
              ('Darwin', 'x86_64'): 'x86_64-apple-darwin', ('Darwin', 'aarch64'): 'aarch64-apple-darwin'}.get((system, cpu))
    if target is None:
        raise ValueError(f'no npm platform package for {system} {machine}')
    return target


def tarball_spec(packages, version, name):
    return 'file:' + (packages / npm.tarball_name(name, version)).resolve().as_posix()


def manifest(manager, version, packages):
    """The clean project's package.json: the main package plus the local platform overrides."""
    fields = {'name': 'dotnet-crap-install-check', 'version': '0.0.0', 'private': True}
    fields['dependencies'] = {npm.MAIN: tarball_spec(packages, version, npm.MAIN)}
    pinned = {npm.platform_name(t): tarball_spec(packages, version, npm.platform_name(t)) for t in npm.TARGETS}
    if manager == 'npm':
        # Root optional dependencies satisfy the main package's exact pins on every
        # npm, keep os/cpu filtering, and avoid npm 9's rejection of file: overrides.
        fields['optionalDependencies'] = pinned
    elif manager == 'pnpm':
        pass  # pnpm reads overrides from pnpm-workspace.yaml; see workspace_files()
    elif manager == 'yarn' or manager == 'yarn-node-modules':
        fields['resolutions'] = pinned
    else:
        fields['overrides'] = pinned
    return fields


def workspace_files(manager, version, packages):
    """Extra project files: pnpm's overrides (JSON is valid YAML) and Yarn Berry's project-root marker."""
    if manager == 'yarn':
        return {'yarn.lock': ''}
    if manager == 'yarn-node-modules':
        return {'yarn.lock': '', '.yarnrc.yml': 'nodeLinker: node-modules\n'}
    if manager == 'pnpm':
        pinned = {npm.platform_name(t): tarball_spec(packages, version, npm.platform_name(t)) for t in npm.TARGETS}
        return {'pnpm-workspace.yaml': json.dumps({'overrides': pinned}, indent=2) + '\n'}
    return {}


def run(command, cwd, expected=0, timeout=600, extra=None):
    executable = shutil.which(command[0])
    if executable is None:
        raise RuntimeError(f'{command[0]} is not installed')
    env = dict(os.environ, **ENVIRONMENT, **(extra or {}))
    # Audited: argument vectors built in this script from fixed package-manager
    # commands and local paths, never a shell or untrusted command text.
    # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit
    result = subprocess.run([executable, *command[1:]], cwd=cwd, capture_output=True, text=True,
                            timeout=timeout, env=env)
    if result.returncode != expected:
        raise RuntimeError(f'{command}: exit {result.returncode}, expected {expected}\n'
                           f'{result.stdout[-8000:]}\n{result.stderr[-8000:]}')
    return result


def other_platform_paths(project, target):
    """Paths in the project named after a platform package for another platform."""
    others = [npm.platform_name(t) for t in npm.TARGETS if t != target]
    found = []
    for path in project.rglob('*'):
        if any(path.name == o or path.name.startswith(o + '@') for o in others):
            found.append(path)
    return found


def verify(manager, version, assets, packages, scan_root, work, target, *, require_static=True):
    expected = npm.executable(assets, target, require_static=require_static)
    project = work / f'{manager}-project'
    project.mkdir()
    (project / 'package.json').write_text(json.dumps(manifest(manager, version, packages), indent=2) + '\n')
    for name, text in workspace_files(manager, version, packages).items():
        (project / name).write_text(text)
    offline = NPM_OFFLINE if manager == 'npm' else None
    run(INSTALL[base(manager)], project, extra=offline)
    runner = RUN[base(manager)]
    result = run([*runner, 'dotnet-crap', '--version'], project)
    if result.stdout.strip() != f'dotnet-crap {version}':
        raise ValueError(f'{manager} dotnet-crap --version printed {result.stdout!r}')
    if 'Usage: dotnet-crap' not in run([*runner, 'dotnet-crap', '--help'], project).stdout:
        raise ValueError(f'{manager} dotnet-crap --help did not print usage')
    # The fixture has two methods: a pass at threshold 100 with --fail-on-empty, and a
    # quality-gate failure at threshold 0 (exit 1), both through the launcher.
    result = run([*runner, 'dotnet-crap', '--root', str(scan_root), '--threshold', '100',
                  '--fail-on-empty', '--format', 'json'], project)
    if len(json.loads(result.stdout)['functions']) < 1:
        raise ValueError(f'{manager}: the scan found no functions in {scan_root}')
    run([*runner, 'dotnet-crap', '--root', str(scan_root), '--threshold', '0'], project, expected=1)
    run([*runner, 'dotnet-crap', '--no-such-option'], project, expected=2)
    binary = Path(run([*NODE[base(manager)], '-e', PRINT_BINARY], project).stdout)
    if binary.read_bytes() != expected:
        raise ValueError(f'{manager}: installed {binary} differs from {npm.archive_name(target)}')
    if os.name != 'nt' and not os.access(binary, os.X_OK):
        raise ValueError(f'{manager}: installed {binary} is not executable')
    # Yarn may keep other platforms' archives in its cache, and Bun installs every
    # override-pinned local tarball, so only npm and pnpm are checked for strays.
    if manager in ('npm', 'pnpm'):
        stray = other_platform_paths(project, target)
        if stray:
            raise ValueError(f'{manager} installed other platform packages: {stray}')
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manager', choices=MANAGERS, action='append', required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--assets', type=Path, required=True, help='release archives with .sha256 sidecars')
    parser.add_argument('--packages', type=Path, required=True, help='local tarballs from scripts/npm-packages.py')
    parser.add_argument('--scan-root', type=Path, default=ROOT / 'examples/Fixture')
    parser.add_argument('--target', choices=sorted(npm.TARGETS), help='release target of this host (default: detected)')
    args = parser.parse_args()
    target = args.target or host_target()
    with tempfile.TemporaryDirectory(prefix='dotnet-crap-npm-') as temporary:
        work = Path(temporary).resolve()
        for name in args.manager:
            installed = verify(name, args.version, args.assets.resolve(), args.packages.resolve(),
                               args.scan_root.resolve(), work, target)
            print(f'{name}: dotnet-crap {args.version} installed without scripts; {installed} matches the release archive')


if __name__ == '__main__':
    main()
