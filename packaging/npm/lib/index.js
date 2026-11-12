'use strict';
/**
 * Locates and runs the prebuilt dotnet-crap scanner from this package's matching
 * platform package. There is no install script and nothing is downloaded: the
 * package manager installs exactly one optional platform package by os and cpu.
 */
const { spawn } = require('node:child_process');
const path = require('node:path');

const MAIN = 'dotnet-crap';
// process.platform-process.arch -> platform package. Linux uses the static musl
// build, so one package per CPU serves glibc and musl systems alike.
const PLATFORMS = {
  'linux-x64': 'dotnet-crap-linux-x64',
  'linux-arm64': 'dotnet-crap-linux-arm64',
  'darwin-x64': 'dotnet-crap-darwin-x64',
  'darwin-arm64': 'dotnet-crap-darwin-arm64',
};

class LauncherError extends Error {}

function version() {
  return require('../package.json').version;
}

function platformPackage(platform = process.platform, arch = process.arch) {
  const name = PLATFORMS[`${platform}-${arch}`];
  if (!name) {
    const windows = platform === 'win32'
      ? ' The npm package has no Windows build yet; use the dotnet-crap .NET tool package instead.'
      : '';
    throw new LauncherError(`${MAIN}: no prebuilt scanner for ${platform}-${arch}; supported platforms are `
      + `${Object.keys(PLATFORMS).join(', ')}.${windows}`);
  }
  return name;
}

/** Absolute path of the installed native scanner; throws a LauncherError with the fix. */
function binaryPath(options = {}) {
  const platform = options.platform ?? process.platform;
  const name = platformPackage(platform, options.arch ?? process.arch);
  const resolve = options.resolve ?? (request => require.resolve(request));
  let manifest;
  try {
    manifest = resolve(`${name}/package.json`);
  } catch {
    throw new LauncherError(`${MAIN}: the platform package ${name} is not installed. It is an optional dependency, `
      + 'so installs that omit optional dependencies (--omit=optional, --no-optional) or copy node_modules between '
      + `platforms skip it. Reinstall ${MAIN} on this machine with optional dependencies enabled.`);
  }
  const installed = (options.readVersion ?? (file => require(file).version))(manifest);
  const expected = options.version ?? version();
  if (installed !== expected) {
    throw new LauncherError(`${MAIN}: ${name} is version ${installed} but ${MAIN} is ${expected}; `
      + `reinstall so both are ${expected}`);
  }
  return path.join(path.dirname(manifest), 'bin', 'dotnet-crap');
}

const FORWARDED = ['SIGINT', 'SIGTERM', 'SIGHUP'];

/**
 * Run a child with inherited stdio, forwarding termination signals, and settle
 * with its exit status. A child stopped by a signal resolves to { signal }.
 */
function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: 'inherit', ...options });
    const forwards = FORWARDED.map(signal => [signal, () => child.kill(signal)]);
    for (const [signal, forward] of forwards) process.on(signal, forward);
    const stop = () => { for (const [signal, forward] of forwards) process.off(signal, forward); };
    child.on('error', error => { stop(); reject(error); });
    child.on('exit', (code, signal) => { stop(); resolve(signal ? { signal } : { code }); });
  });
}

/** Mirror the child's result: same exit code, or the same terminating signal. */
function finish(result) {
  if (result.signal) {
    process.kill(process.pid, result.signal);
    return;
  }
  process.exitCode = result.code;
}

function fail(error) {
  // The scanner reports usage and runtime errors with exit status 2.
  let message = error instanceof LauncherError ? error.message : `${MAIN}: ${error.message}`;
  if (error.code === 'EACCES') message += `; the executable lost its permission bits, so reinstall ${MAIN} on this machine`;
  console.error(message);
  process.exitCode = 2;
}

/** Run the native scanner with ARGS, exactly as the standalone `dotnet-crap` binary. */
function runScanner(args) {
  let binary;
  try { binary = binaryPath(); } catch (error) { fail(error); return Promise.resolve(); }
  return run(binary, args).then(finish, fail);
}

module.exports = {
  FORWARDED, LauncherError, PLATFORMS, binaryPath, fail, finish, platformPackage, run, runScanner, version,
};
