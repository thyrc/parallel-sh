# parallel-sh

[![Build Status](https://travis-ci.com/thyrc/parallel-sh.svg?branch=master)](https://travis-ci.com/thyrc/parallel-sh)
[![CI](https://github.com/thyrc/parallel-sh/workflows/Rust/badge.svg)](https://github.com/thyrc/parallel-sh/actions?query=workflow%3ARust)
[![GitHub license](https://img.shields.io/github/license/thyrc/parallel-sh.svg)](https://github.com/thyrc/parallel-sh/blob/master/LICENSE)

`parallel-sh` was heavily inspired by [parallel](https://crates.io/crates/parallel) parallelzing 'otherwise non-parallel command-line tasks.'
But instead of recreating the full functionality of `GNU Parallel` `parallel-sh` will only execute (lines of) commands in the platform's
preferred shell ('sh -c' on Unix systems, and 'powershell.exe -c' on Windows) in separate threads.

- There are no replacement strings (e.g. '{}') or input tokens. Commands will be executed as provided by argument, file or via stdin.
- Command sources will not be 'linked'. Arguments will be processed by [preference](##Preference):
    1. If ARGS are found, `--file` option and stdin is ignored.
    2. If `--file` is provided anything on stdin is ignored.
    3. Only when there are no command arguments and no '--file' option is found, any lines on stdin are treated as commands to
        execute.
- There is no progressbar.
- There is no '--pipe' feature. Stdin is not inherited from the parent and any attempt by the child process to read from the stdin
    stream will result in the stream immediately closing.

Output (stdout and stderr) of each child process is stored and printed only after the child exits.

## Options
```
parallel-sh 0.1.0
Execute commands in parallel

USAGE:
    parallel-sh [FLAGS] [OPTIONS] [clijobs]...

FLAGS:
        --dry-run          Perform a trial run, only print what would be done (with -vv)
        --halt-on-error    Stop execution if an error occurs in any thread
    -h, --help             Prints help information
    -q, --quiet            Do not print `parallel-sh` warnings
    -v, --verbose          Sets the level of verbosity
    -V, --version          Prints version information

OPTIONS:
    -f, --file <FILE>       Read commands from file (one command per line)
    -l, --log <FILE>        Log output to file
    -j, --jobs <THREADS>    Number of parallel executions

ARGS:
    <clijobs>...
```

## Preference

1. Pass commands as arguments:
   ```
   parallel-sh "sleep 1 && echo 1" "sleep 2 && echo 2"
   ```

2. Pass a file with one command (-line) per line:
   ```
   parallel-sh -f /tmp/commands

   $ cat /tmp/commands
   sleep 1 && echo 1
   sleep 2 && echo 2
   ```

3. Pass commadn via stdin:
  ```
  echo -e 'sleep 1 && echo 1\nsleep 2 && echo 2' |parallel-sh
  ```
