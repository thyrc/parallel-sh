use clap::{
    builder::{RangedU64ValueParser, ValueParser},
    Arg, ArgAction, ArgMatches, Command,
};
use log::{debug, error, info, warn};
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, SharedLogger, TermLogger,
    TerminalMode, WriteLogger,
};
use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader},
    path::PathBuf,
    process::Output,
    sync::mpsc::{channel, Receiver, Sender},
    sync::{Arc, Mutex},
    thread,
};
use time::{Duration, Instant};

#[allow(dead_code)]
#[derive(Debug)]
struct JobResult {
    seq: usize,
    starttime: Instant,
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

fn create_logger(opts: &ArgMatches) -> Result<(), std::io::Error> {
    let level = match (opts.contains_id("quiet"), opts.get_count("v")) {
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

    if let Some(file) = opts.get_one::<OsString>("log").map(PathBuf::from) {
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
            let file = File::open(&jobsfile)?;
            for command in BufReader::new(file).lines().flatten() {
                start_job(command);
            }
        } else {
            let stdin = io::stdin();
            let handle = stdin.lock();
            for command in BufReader::new(handle).lines().flatten() {
                start_job(command);
            }
        }
    } else {
        // preferred
        clijobs.into_iter().for_each(start_job);
    }

    Ok(())
}

fn run(dry_run: bool, command: &str) -> Output {
    let mut shell = std::process::Command::new("sh");

    if cfg!(target_os = "windows") {
        shell = std::process::Command::new("powershell");
    }

    if dry_run {
        return shell.output().expect("Failed to run shell");
    };

    shell
        .arg("-c")
        .arg(command)
        .output()
        .expect("Failed to execute command")
}

#[allow(clippy::needless_pass_by_value)]
fn start_workers(
    threads: usize,
    dry_run: bool,
    jobs: &SharedReceiver<String>,
    results: Sender<JobResult>,
) {
    debug!("Starting {} worker threads", threads);
    for seq in 0..threads {
        let jobs = jobs.clone();
        let results = results.clone();
        thread::spawn(move || {
            for job in jobs {
                let starttime = Instant::now();
                let output = run(dry_run, &job);
                let duration = starttime.elapsed();
                results
                    .send(JobResult {
                        seq,
                        starttime,
                        duration,
                        job,
                        output,
                    })
                    .unwrap_or_else(|e| error!("Could not send job: {}", e));
            }
        });
    }
}

#[allow(clippy::too_many_lines)]
fn main() {
    let matches = Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .conflicts_with("v")
                .help(format!("Do not print `{}` warnings", env!("CARGO_PKG_NAME")).as_ref()),
        )
        .arg(
            Arg::new("dry_run")
                .long("dry-run")
                .help("Perform a trial run, only print what would be done (with -vv)"),
        )
        .arg(
            Arg::new("v")
                .long("verbose")
                .short('v')
                .action(ArgAction::Count)
                .conflicts_with("quiet")
                .help("Sets the level of verbosity"),
        )
        .arg(
            Arg::new("log")
                .long("log")
                .short('l')
                .value_name("FILE")
                .value_parser(ValueParser::os_string())
                .takes_value(true)
                .help("Log output to file"),
        )
        .arg(
            Arg::new("halt")
                .long("halt-on-error")
                .help("Stop execution if an error occurs in any thread"),
        )
        .arg(
            Arg::new("threads")
                .long("jobs")
                .short('j')
                .value_name("THREADS")
                .value_parser(RangedU64ValueParser::<usize>::new().range(1..=4096))
                .takes_value(true)
                .help("Number of parallel executions"),
        )
        .arg(
            Arg::new("jobsfile")
                .long("file")
                .short('f')
                .value_name("FILE")
                .value_parser(ValueParser::os_string())
                .takes_value(true)
                .help("Read commands from file (one command per line)"),
        )
        .arg(
            Arg::new("clijobs")
                .multiple_values(true)
                .action(ArgAction::Append),
        )
        .get_matches();

    if let Err(e) = create_logger(&matches) {
        error!("Could create logger: {}", e);
        std::process::exit(1);
    }

    let (tx, rx) = shared_channel();

    // return channel
    let (rtx, rrx) = channel();

    start_workers(
        *matches.get_one("threads").unwrap_or(&num_cpus::get()),
        matches.contains_id("dry_run"),
        &rx,
        rtx,
    );

    let mut clijobs = vec![];
    if matches.contains_id("clijobs") {
        clijobs = matches
            .get_many::<String>("clijobs")
            .unwrap_or_default()
            .cloned()
            .collect::<Vec<_>>();
    }

    let jobsfile = matches.get_one::<OsString>("jobsfile").map(PathBuf::from);

    if let Err(e) = add_jobs(clijobs, jobsfile, tx) {
        error!("Could not start jobs: {}", e);
        std::process::exit(1);
    }

    let mut exit = 0;
    for result in rrx {
        if !matches.contains_id("dry_run") {
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
                warn!(
                    "'{}' exited with status code {}",
                    &result.job, &result.output.status
                );
                print!("{}", String::from_utf8_lossy(&result.output.stdout));
                eprint!("{}", String::from_utf8_lossy(&result.output.stderr));

                if matches.contains_id("halt") {
                    std::process::exit(1);
                } else {
                    exit = result.output.status.code().unwrap_or(127);
                }
            }
        }
    }
    std::process::exit(exit);
}
