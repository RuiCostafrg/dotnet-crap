# Bound parallel scanning

Schedule larger files first to reduce the wait for the last worker, then return reports and errors in source selection order so the choice of worker count cannot change what users see. Limit bytes admitted for parsing because multiple large syntax trees can dominate peak memory even when the worker count is small. This lets small files run concurrently and gives a file larger than the budget the parser to itself.

See [the worker pool PR](https://github.com/RuiCostafrg/dotnet-crap/pull/46) and [benchmark workloads](../BENCHMARKS.md#workloads).
