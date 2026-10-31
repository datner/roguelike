//! Running the bench's games on every core (docs/plans/bench-cli.md): each run is a game of its own,
//! built from its seed, so they share nothing and the order they finish in changes no number.
//! Results come back in job order, so the report reads the same as a run on one core. While a
//! terminal watches, one stderr line says how far along it is.

use std::io::{IsTerminal, Write as _};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// `work` on every job, on as many threads as the machine has, the results in job order. The
/// first error (in job order) wins. `part` and `label` name the work and each job for the
/// progress line.
pub(crate) fn run_all<J, R>(
    part: &str,
    jobs: &[J],
    label: impl Fn(&J) -> String + Sync,
    work: impl Fn(&J) -> Result<R, String> + Sync,
) -> Result<Vec<R>, String>
where
    J: Sync,
    R: Send,
{
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let next = AtomicUsize::new(0);
    let progress = Progress::new(part, jobs.len());
    let mut done: Vec<(usize, Result<R, String>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers.min(jobs.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(at) else {
                            return mine;
                        };
                        mine.push((at, work(job)));
                        progress.tick(|| label(job));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            // A worker that panicked would leave holes: the panic goes on up instead.
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    progress.finish();
    done.sort_by_key(|&(at, _)| at);
    done.into_iter().map(|(_, result)| result).collect()
}

/// The progress line, on stderr, only when it is a terminal: a log or a pipe gets the closing line
/// alone.
struct Progress<'a> {
    part: &'a str,
    of: usize,
    done: Mutex<usize>,
    start: Instant,
    shown: bool,
}

impl<'a> Progress<'a> {
    fn new(part: &'a str, of: usize) -> Self {
        Self {
            part,
            of,
            done: Mutex::new(0),
            start: Instant::now(),
            shown: std::io::stderr().is_terminal(),
        }
    }

    fn tick(&self, label: impl FnOnce() -> String) {
        let Ok(mut done) = self.done.lock() else {
            return;
        };
        *done += 1;
        if self.shown {
            // Holding the lock keeps two workers' lines from interleaving.
            let line = format!(
                "{} {}/{} · {} · {}",
                self.part,
                *done,
                self.of,
                label(),
                clock(self.start)
            );
            let _ = write!(std::io::stderr(), "\r\x1b[2K{line}");
        }
    }

    fn finish(&self) {
        let clear = if self.shown { "\r\x1b[2K" } else { "" };
        eprintln!(
            "{clear}{}: {} runs in {}",
            self.part,
            self.of,
            clock(self.start)
        );
    }
}

/// `m:ss` since `start`.
fn clock(start: Instant) -> String {
    let secs = start.elapsed().as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_come_back_in_job_order_and_the_first_error_wins() {
        let jobs: Vec<u64> = (0..500).collect();
        let label = |n: &u64| n.to_string();
        let squares = run_all("test", &jobs, label, |&n| Ok(n * n)).unwrap();
        assert_eq!(squares, jobs.iter().map(|n| n * n).collect::<Vec<_>>());
        let failed = run_all("test", &jobs, label, |&n| {
            if n % 100 == 7 {
                Err(format!("job {n}"))
            } else {
                Ok(n)
            }
        });
        assert_eq!(failed, Err("job 7".to_string()));
        assert_eq!(
            run_all("test", &[] as &[u64], label, |&n| Ok(n)),
            Ok(Vec::new())
        );
    }
}
