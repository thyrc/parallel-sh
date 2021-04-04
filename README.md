# parallel-sh

[![Crates.io](https://img.shields.io/crates/v/parallel-sh.svg)](https://crates.io/crates/parallel-sh)
[![Build Status](https://travis-ci.com/thyrc/parallel-sh.svg?branch=main)](https://travis-ci.com/thyrc/parallel-sh)
[![CI](https://github.com/thyrc/parallel-sh/workflows/Rust/badge.svg)](https://github.com/thyrc/parallel-sh/actions?query=workflow%3ARust)
[![GitHub license](https://img.shields.io/github/license/thyrc/parallel-sh.svg)](https://github.com/thyrc/parallel-sh/blob/main/LICENSE)

`parallel-sh` was heavily inspired by Rust Parallel ([parallel](https://crates.io/crates/parallel)) parallelzing 'otherwise non-parallel command-line tasks.' But instead of recreating the full functionality of GNU Parallel `parallel-sh` will only execute (lines of) commands in the platform's preferred shell ('sh -c' on Unix systems, and 'powershell.exe -c' on Windows) in separate threads.

What is not part of `parallel-sh`:

- There are no replacement strings (e.g. '{}') or input tokens. Commands will be executed as provided by argument, file or via stdin.
- Command sources will not be 'linked'. Arguments will be processed by [preference](#preference):
    1. If ARGS are found, `--file` option and stdin is ignored.
    2. If `--file` is provided anything on stdin is ignored.
    3. Only when there are no command arguments and no '--file' option is found, any lines on stdin are treated as commands to
        execute.
- There is no progressbar or '--eta' flag.
- There is no '--pipe'. Stdin is not inherited from the parent and any attempt by the child processes to read from the stdin
    stream will result in the stream immediately closing.
- There is no '--sshlogin' or '--transferfile'.
- There are no '--limit', '--retry', '--resume' or '--sql' options.

If you need any of these to be part of your parallelizing tool please check GNU Parallel or Rust Parallel.

Most of the effects of these features can be achieved by processing the commands before passing them to `parallel-sh`.

What to expect:

- Output (stdout and stderr) of each child process is stored and printed only after the child exits.
- There is some simple logging and some runtime metric (via -v, -vv or -vvv) available.
- The whole crate is tiny, <300 lines of code (w/ ~25% command line argument parsing), and can quickly be modified to meet more complex requirements.

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

3. Pass commands via stdin:
   ```
   echo -e 'sleep 1 && echo 1\nsleep 2 && echo 2' |parallel-sh
   ```
