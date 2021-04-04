extern crate chrono;
extern crate clap;
extern crate log;
extern crate simplelog;

use chrono::{DateTime, Duration, Local};
use clap::{value_t, values_t, App, Arg, ArgMatches};
use log::{debug, error, info, warn};
use simplelog::*;
use std::{
    fs::File,
    io::{self, BufRead, BufReader},
    path::PathBuf,
    process::{Command, Output},
    sync::mpsc::{channel, Receiver, Sender},
    sync::{Arc, Mutex},
    thread,
};

#[derive(Debug)]
struct JobResult {
    seq: usize,
    output: Output,
    starttime: DateTime<Local>,
    duration: Duration,
    job: String,
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
    let level = match (opts.is_present("quiet"), opts.occurrences_of("v")) {
        (true, _) => LevelFilter::Error,
        (_, 0) => LevelFilter::Warn,
        (_, 1) => LevelFilter::Info,
        (_, 2) => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    let logconfig = ConfigBuilder::new()
        .set_time_format("[%FT%T%:z]".to_string())
        .set_time_to_local(true)
        .build();

    let mut loggers: Vec<Box<dyn SharedLogger>> = vec![TermLogger::new(
        level,
        logconfig.clone(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    )];

    if let Some(file) = opts.value_of("log") {
        loggers.push(WriteLogger::new(
            LevelFilter::Info,
            logconfig,
            File::create(file)?,
        ));
    }
    CombinedLogger::init(loggers).unwrap();

    Ok(())
}

fn add_jobs(
    clijobs: Vec<String>,
    jobsfile: Option<PathBuf>,
    tx: Sender<String>,
) -> Result<(), std::io::Error> {
    let start_job = |job| {
        debug!("Starting job '{}'", &job);
        tx.send(job)
            .unwrap_or_else(|e| error!("Could no add job: {}", e));
    };
    if clijobs.is_empty() {
        if let Some(jobsfile) = jobsfile {
            let file = File::open(&jobsfile)?;
            for command in BufReader::new(file).lines() {
                if let Ok(jobline) = command {
                    start_job(jobline);
                }
            }
        } else {
            let stdin = io::stdin();
            let handle = stdin.lock();
            for command in BufReader::new(handle).lines() {
                if let Ok(jobline) = command {
                    start_job(jobline);
                }
            }
        }
    } else {
        // preferred
        clijobs.into_iter().for_each(start_job);
    }

    Ok(())
}

fn run(dry_run: bool, command: &str) -> Output {
    let mut shell = Command::new("sh");

    if cfg!(target_os = "windows") {
        shell = Command::new("powershell");
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

fn start_workers(
    threads: usize,
    dry_run: bool,
    jobs: SharedReceiver<String>,
    results: Sender<JobResult>,
) {
    debug!("Starting {} worker threads", threads);
    for seq in 0..threads {
        let jobs = jobs.clone();
        let results = results.clone();
        thread::spawn(move || {
            for job in jobs {
                let starttime = Local::now();
                let output = run(dry_run, &job);
                let duration = Local::now().signed_duration_since(starttime);
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

fn main() {
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(
            Arg::with_name("quiet")
                .short("q")
                .long("quiet")
                .conflicts_with("v")
                .help(format!("Do not print `{}` warnings", env!("CARGO_PKG_NAME")).as_ref()),
        )
        .arg(
            Arg::with_name("dry_run")
                .long("dry-run")
                .help("Perform a trial run, only print what would be done (with -vv)"),
        )
        .arg(
            Arg::with_name("v")
                .long("verbose")
                .short("v")
                .multiple(true)
                .conflicts_with("quiet")
                .help("Sets the level of verbosity"),
        )
        .arg(
            Arg::with_name("log")
                .long("log")
                .short("l")
                .value_name("FILE")
                .takes_value(true)
                .help("Log output to file"),
        )
        .arg(
            Arg::with_name("halt")
                .long("halt-on-error")
                .help("Stop execution if an error occurs in any thread"),
        )
        .arg(
            Arg::with_name("threads")
                .long("jobs")
                .short("j")
                .value_name("THREADS")
                .takes_value(true)
                .help("Number of parallel executions"),
        )
        .arg(
            Arg::with_name("jobsfile")
                .long("file")
                .short("f")
                .takes_value(true)
                .value_name("FILE")
                .help("Read commands from file (one command per line)"),
        )
        .arg(Arg::with_name("clijobs").multiple(true))
        .get_matches();

    if let Err(e) = create_logger(&matches) {
        error!("Could create logger: {}", e);
        std::process::exit(1);
    }

    let (tx, rx) = shared_channel();

    // return channel
    let (rtx, rrx) = channel();

    start_workers(
        value_t!(matches, "threads", usize).unwrap_or_else(|_| num_cpus::get()),
        matches.is_present("dry_run"),
        rx,
        rtx,
    );

    let mut clijobs = vec![];
    if matches.is_present("clijobs") {
        for v in values_t!(matches, "clijobs", String).unwrap() {
            clijobs.push(v);
        }
    }

    let jobsfile = match matches.value_of_os("jobsfile") {
        Some(path) => Some(PathBuf::from(path)),
        _ => None,
    };

    if let Err(e) = add_jobs(clijobs, jobsfile, tx) {
        error!("Could not start jobs: {}", e);
        std::process::exit(1);
    }

    let mut exit = 0;
    for result in rrx {
        if !matches.is_present("dry_run") {
            info!(
                "'{}' took {}.{}s",
                &result.job,
                &result.duration.num_seconds(),
                &result.duration.num_nanoseconds().unwrap_or(0)
            );
            if !result.output.status.success() {
                warn!(
                    "'{}' exited with status code {}",
                    &result.job, &result.output.status
                );
                print!("{}", String::from_utf8_lossy(&result.output.stdout));
                eprint!("{}", String::from_utf8_lossy(&result.output.stderr));

                if matches.is_present("halt") {
                    std::process::exit(1);
                } else {
                    exit = result.output.status.code().unwrap_or(127);
                }
            } else {
                print!("{}", String::from_utf8_lossy(&result.output.stdout));
                eprint!("{}", String::from_utf8_lossy(&result.output.stderr));
            }
        }
    }
    std::process::exit(exit);
}
