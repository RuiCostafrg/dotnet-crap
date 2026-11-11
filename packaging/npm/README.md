# dotnet-crap

Score C# methods by cyclomatic complexity and source-bound coverage (CRAP),
from [RuiCostafrg/dotnet-crap](https://github.com/RuiCostafrg/dotnet-crap).

```sh
npm install --save-dev dotnet-crap
npx dotnet-crap --root . --threshold 30
```

The package installs a prebuilt native scanner for Linux (x64, arm64; static,
so glibc and musl both work) and macOS (x64, arm64). The executable comes from a
platform package (`dotnet-crap-linux-x64` and so on), which your package manager
selects by `os` and `cpu` and fetches like any other dependency. There is no
install script and no download outside the package manager, so installing works
with `--ignore-scripts`, pnpm, Yarn (including Plug'n'Play), Bun, offline caches
and registry mirrors. Do not omit optional dependencies (`--omit=optional`): the
platform packages are optional dependencies. Windows is not supported by this
package; use the .NET tool (`dotnet tool install --global dotnet-crap`).

## Command

`dotnet-crap ARGS` runs the scanner. Arguments, output and exit codes match the
standalone binary: 0 pass, 1 quality gate failed, 2 error.

## Verification

Each platform package's executable is byte-identical to the checksum-verified
`dotnet-crap-<target>.tar.xz` release archive it was built from. See the
[security policy](https://github.com/RuiCostafrg/dotnet-crap/blob/main/SECURITY.md).

MIT licensed.
