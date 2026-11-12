#!/usr/bin/env node
'use strict';
// The dotnet-crap scanner: arguments, output and exit status are the native binary's.
require('../lib/index.js').runScanner(process.argv.slice(2));
