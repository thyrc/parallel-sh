use log::{debug, error, info, warn};
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, SharedLogger, TermLogger,
    TerminalMode, WriteLogger,
};
#[cfg(not(target_os = "windows"))]
use std::os::unix::process::ExitStatusExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::ExitStatusExt;
use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader},
    path::PathBuf,
    process::{self, ExitStatus, Output},
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
};
use time::{Duration, Instant};

const HELP: &str = "\
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
";

#[derive(Debug)]
struct Args {
    quiet: bool,
    dryrun: bool,
    verbose: usize,
    logfile: Option<OsString>,
    halt: bool,
    threads: usize,
    shell: Option<OsString>,
    file: Option<OsString>,
    clijobs: Vec<String>,
}

#[derive(Debug)]
struct JobResult {
    duration: Duration,
    job: String,
    output: Output,
}

// A thread-safe wrapper around a `Receiver`
#[derive(Debug, Clone)]
struct SharedReceiver<T>(Arc<Mutex<Receiver<T>>>);

impl<T> Iterator for SharedReceiver<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let guard = self.0.lock().unwrap();
        guard.recv().ok()
    }
}

fn shared_channel<T>() -> (Sender<T>, SharedReceiver<T>) {
    let (sender, receiver) = channel();
    (sender, SharedReceiver(Arc::new(Mutex::new(receiver))))
}

fn parse_args() -> Result<Args, lexopt::Error> {
    use lexopt::prelude::*;

    let mut shell = if cfg!(target_os = "windows") {
        Some(OsString::from("powershell"))
    } else {
        Some(OsString::from("sh"))
    };

    let mut quiet = false;
    let mut dryrun = false;
    let mut verbose = 0;
    let mut logfile = None;
    let mut halt = false;
    let mut threads = num_cpus::get();
    let mut file = None;
    let mut clijobs = vec![];

    let mut parser = lexopt::Parser::from_env();

    while let Some(arg) = parser.next()? {
        match arg {
            Short('q') | Long("quiet") => {
                quiet = true;
            }
            Short('n') | Long("dry-run") => {
                dryrun = true;
            }
            Short('v') | Long("verbose") => {
                verbose += 1;
            }
            Short('l') | Long("log") => {
                logfile = Some(parser.value()?.parse()?);
            }
            Long("halt-on-error") => {
                halt = true;
            }
            Short('j') | Long("jobs") => {
                threads = parser.value()?.parse()?;
            }
            Short('s') | Long("shell") => {
                shell = Some(parser.value()?.parse()?);
            }
            Long("no-shell") => {
                shell = None;
            }
            Short('f') | Long("file") => {
                file = Some(parser.value()?.parse()?);
            }
            Short('h') | Long("help") => {
                println!("{HELP}");
                process::exit(0);
            }
            Short('V') | Long("version") => {
                println!("{} {}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            //Value(_) if clijobs.is_empty() => {
            //    let jobs: Vec<OsString> = parser.values()?.map(Into::into).collect();
            //    clijobs = jobs
            //        .iter()
            //        .map(|os_str| os_str.to_string_lossy().to_string())
            //        .collect();
            //}
            Value(value) => {
                clijobs.push(value.string()?);
            }
            _ => return Err(arg.unexpected()),
        }
    }

    Ok(Args {
        quiet,
        dryrun,
        verbose,
        logfile,
        halt,
        threads,
        shell,
        file,
        clijobs,
    })
}

fn create_logger(opts: &Args) -> Result<(), std::io::Error> {
    let level = match (opts.quiet, opts.verbose) {
        (true, _) => LevelFilter::Error,
        (_, 0) => LevelFilter::Warn,
        (_, 1) => LevelFilter::Info,
        (_, 2) => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    let logconfig = ConfigBuilder::new()
        .set_time_format_rfc3339()
        .set_time_offset_to_local()
        .unwrap_or_else(|v| v)
        .build();

    let mut loggers: Vec<Box<dyn SharedLogger>> = vec![TermLogger::new(
        level,
        logconfig.clone(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    )];

    if let Some(file) = opts.logfile.clone().map(PathBuf::from) {
        loggers.push(WriteLogger::new(
            level,
            logconfig,
            OpenOptions::new().append(true).create(true).open(file)?,
        ));
    }

    if CombinedLogger::init(loggers).is_err() {
        error!("Could not initialize logger.");
    };

    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn add_jobs(
    clijobs: Vec<String>,
    jobsfile: Option<PathBuf>,
    tx: Sender<String>,
) -> Result<(), std::io::Error> {
    let start_job = |job| {
        debug!("Starting job '{}'", &job);
        tx.send(job)
            .unwrap_or_else(|e| error!("Could not add job: {}", e));
    };
    if clijobs.is_empty() {
        if let Some(jobsfile) = jobsfile {
            let file = File::open(jobsfile)?;
            BufReader::new(file)
                .lines()
                .map_while(Result::ok)
                .for_each(start_job);
        } else {
            let stdin = io::stdin();
            let handle = stdin.lock();
            BufReader::new(handle)
                .lines()
                .map_while(Result::ok)
                .for_each(start_job);
        }
    } else {
        // preferred
        clijobs.into_iter().for_each(start_job);
    }

    Ok(())
}

fn run(dry_run: bool, command: &str, shell: &Option<OsString>) -> Output {
    if dry_run {
        return Output {
            status: ExitStatus::default(),
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
    };

    if let Some(s) = shell {
        let mut shell = std::process::Command::new(s);

        match shell.arg("-c").arg(command).output() {
            Ok(s) => s,
            Err(_) => Output {
                status: ExitStatus::from_raw(1),
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
        }
    } else {
        let cmd: Vec<_> = command.split(' ').collect();
        let mut command = std::process::Command::new(cmd[0]);

        match command.args(&cmd[1..]).output() {
            Ok(c) => c,
            Err(_) => Output {
                status: ExitStatus::from_raw(1),
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn start_workers(
    threads: usize,
    dry_run: bool,
    jobs: &SharedReceiver<String>,
    results: Sender<JobResult>,
    shell: &Option<OsString>,
) {
    if dry_run {
        debug!("Perform a trial run with no changes made");
    }
    debug!("Starting {} worker threads", threads);
    for _seq in 0..threads {
        let jobs = jobs.clone();
        let results = results.clone();
        let shell = shell.clone();
        thread::spawn(move || {
            for job in jobs {
                let starttime = Instant::now();
                let output = run(dry_run, &job, &shell);
                let duration = starttime.elapsed();
                results
                    .send(JobResult {
                        duration,
                        job,
                        output,
                    })
                    .unwrap_or_else(|e| error!("Could not send job: {}", e));
            }
        });
    }
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("ERROR: {e}");
            process::exit(1);
        }
    };

    if let Err(e) = create_logger(&args) {
        error!("Could create logger: {}", e);
        process::exit(1);
    }

    let (tx, rx) = shared_channel();

    // return channel
    let (rtx, rrx) = channel();

    let shell = if let Some(s) = args.shell {
        debug!("Using shell: '{}'", s.to_string_lossy());
        Some(s)
    } else {
        debug!("Running command without shell");
        None
    };

    start_workers(args.threads, args.dryrun, &rx, rtx, &shell);

    let jobsfile = args.file.map(PathBuf::from);

    if let Err(e) = add_jobs(args.clijobs, jobsfile, tx) {
        error!("Could not start jobs: {}", e);
        std::process::exit(1);
    }

    let mut exit = 0;
    for result in rrx {
        if !args.dryrun {
            info!(
                "'{}' took {}.{}s",
                &result.job,
                &result.duration.whole_seconds(),
                &result.duration.whole_nanoseconds()
            );
            if result.output.status.success() {
                print!("{}", String::from_utf8_lossy(&result.output.stdout));
                eprint!("{}", String::from_utf8_lossy(&result.output.stderr));
            } else {
                warn!("'{}' {}", &result.job, &result.output.status);
                print!("{}", String::from_utf8_lossy(&result.output.stdout));
                eprint!("{}", String::from_utf8_lossy(&result.output.stderr));

                if args.halt {
                    std::process::exit(1);
                } else {
                    exit = result.output.status.code().unwrap_or(127);
                }
            }
        }
    }
    std::process::exit(exit);
}
