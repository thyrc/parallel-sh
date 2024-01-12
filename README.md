# parallel-sh

[![Crates.io](https://img.shields.io/crates/v/parallel-sh.svg)](https://crates.io/crates/parallel-sh)
[![CI](https://github.com/thyrc/parallel-sh/workflows/Rust/badge.svg)](https://github.com/thyrc/parallel-sh/actions?query=workflow%3ARust)
[![GitHub license](https://img.shields.io/github/license/thyrc/parallel-sh.svg)](https://github.com/thyrc/parallel-sh/blob/main/LICENSE)

`parallel-sh` was heavily inspired by Rust Parallel ([parallel](https://crates.io/crates/parallel)) parallelizing 'otherwise non-parallel command-line tasks.' But instead of trying to recreate the full functionality of GNU Parallel `parallel-sh` will simply execute (lines of) commands in the platform's preferred shell (by default 'sh -c' on Unix systems, and 'powershell.exe -c' on Windows) in separate threads.

What to expect:

- Output (stdout and stderr) of each child process is stored and printed only after the child exits.
- There is some simple logging and some runtime metric (via -v, -vv or -vvv) available.
- The whole crate is tiny, <400 lines of code (a lot of it command line argument parsing), and can quickly be modified to meet more complex requirements.

What is not part of `parallel-sh`:

- There are no replacement strings (e.g. '{}') or input tokens. Commands will be executed as provided by argument, file or via stdin.
- Command sources will not be 'linked'. Arguments will be processed by [preference](#preference):
    1. If ARGS are found, `--file` option and stdin are ignored.
    2. If `--file` is provided anything on stdin is ignored.
    3. Only when there are no command arguments and no '--file' option is found, any lines on stdin are treated as commands to
        execute.
- Stdin is not inherited from the parent and any attempt by the child processes to read from the stdin stream will result in the stream immediately closing. But you can use pipes, redirects etc. within each thread as long as your shell provides the functionality, e.g. `parallel-sh 'ls -1 |wc -l` or `parallel-sh.exe "Get-ChildItem -Path * | Measure-Object -Line"`

Most of the effects of these features can be achieved by processing the commands before passing them to `parallel-sh`.

## Options
```text
parallel-sh 0.1.14
Execute commands in parallel

Usage: parallel-sh [OPTIONS] [clijobs]...

Arguments:
  [clijobs]...

Options:
  -q, --quiet           Do not print `parallel-sh` warnings
  -n, --dry-run         Perform a trial run, only print what would be done (with -vv)
  -v, --verbose...      Sets the level of verbosity
  -l, --log <FILE>      Log output to file
      --halt-on-error   Stop execution if an error occurs in any thread
  -j, --jobs <THREADS>  Number of parallel executions
  -s, --shell <SHELL>   Shell to use for command execution. Must support '-c' (defaults to sh)
      --no-shell        Do not pass commands through a shell, but execute them directly
  -f, --file <FILE>     Read commands from file (one command per line)
  -h, --help            Print help
  -V, --version         Print version
```

## Note

Per default commands are executed via <SHELL> -c "command", therefore the provided shell must support the '-c' option.

With `--no-shell` the commands are started without passing them through a shell. This will avoid the overhead of starting a shell in each thread, but you will lose features like quotes, escaped characters, word splitting, glob patterns, variable substitution, etc.

The commands inherit `parallel-sh`’s working directory.

## Preference

1. Pass commands as arguments:
   ```shell
   parallel-sh "sleep 2 && echo first" "sleep 1 && echo second"
   ```

2. Pass a file with one command (-line) per line:
   ```shell
   parallel-sh -f /tmp/commands

   $ cat /tmp/commands
   sleep 2 && echo first
   sleep 1 && echo second
   ```

3. Pass commands via stdin:
   ```shell
   echo -e 'sleep 2 && echo first\nsleep 1 && echo second' |parallel-sh
   ```
