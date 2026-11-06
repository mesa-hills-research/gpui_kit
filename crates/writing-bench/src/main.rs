//! Replays written text against GPUI Kit's word suggestions, keystroke by
//! keystroke, and measures what they offer.
//!
//! ```sh
//! writing-bench --corpus CORPUS --out RUN [--mode eager|quiet]
//!     [--timing fluent|hesitant|thinking|recorded|MEDIAN_MS] [--quiet-timing fixed|rhythm]
//!     [--reaction MS] [--policy default|ungated|frequency|frequency-always|gate|confidence]
//!     [--gate-prior F] [--gate-scale F] [--gate-long] [--typist tab|word|blind|slip]
//!     [--splits tuning,validation] [--writers PREFIX] [--threads N] [--latency]
//! ```
//!
//! A corpus is a folder with `manifest.json`, naming each writer's split,
//! and `writers/<id>/stream.jsonl`, one piece of their writing a line,
//! oldest first. Each writer starts with an empty vocabulary and the bundled
//! English dictionary, and the pieces are typed in order, each in a new
//! field, a document or a composer that sends at the end. The run writes
//! `writers.jsonl`, one line of counts per writer, and `run.json`, its
//! settings. Held-out writers are replayed only when `--splits` names them.
//!
//! A number for `--timing` is a typist whose letters come that many
//! milliseconds apart in the median, with the spread, hitches and pauses
//! real typists have. `--reaction` is how long a ghost must have been on
//! screen before the typist can take it. `--quiet-timing rhythm` times
//! Quiet by the typist's own rhythm instead of the fixed pause.

mod corpus;
mod typist;

use corpus::Corpus;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use typist::{Settings, Timing, TypistKind};

use gpui_kit_writing::replay::{Policy, QuietTiming, SuggestionMode};

fn usage() -> ! {
    eprintln!(
        "writing-bench --corpus DIR --out DIR [--mode eager|quiet] \
         [--timing fluent|hesitant|thinking|recorded|MEDIAN_MS] [--quiet-timing fixed|rhythm] \
         [--reaction MS] [--policy default|ungated|frequency|frequency-always|gate|confidence] \
         [--gate-prior F] [--gate-scale F] [--gate-long] [--typist tab|word|blind|slip] \
         [--splits tuning,validation] [--writers PREFIX] [--threads N] [--latency]"
    );
    std::process::exit(2);
}

fn main() {
    let mut corpus = None;
    let mut out = None;
    let mut settings = Settings::default();
    let mut splits = vec!["tuning".to_string(), "validation".to_string()];
    let mut prefix = String::new();
    let mut threads = 1usize;
    let mut gate = false;
    let (mut prior, mut scale, mut long_bar) = (2.0, 1.0, false);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match arg.as_str() {
            "--corpus" => corpus = Some(PathBuf::from(value())),
            "--out" => out = Some(PathBuf::from(value())),
            "--mode" => {
                settings.mode = match value().as_str() {
                    "eager" => SuggestionMode::Eager,
                    "quiet" => SuggestionMode::Quiet,
                    _ => usage(),
                }
            }
            "--timing" => {
                settings.timing = match value().as_str() {
                    "fluent" => Timing::Fluent,
                    "hesitant" => Timing::Hesitant,
                    "thinking" => Timing::Thinking,
                    "recorded" => Timing::Recorded,
                    median => Timing::Natural(median.parse().unwrap_or_else(|_| usage())),
                }
            }
            "--quiet-timing" => {
                settings.quiet_timing = match value().as_str() {
                    "fixed" => QuietTiming::FixedPause,
                    "rhythm" => QuietTiming::Rhythm,
                    _ => usage(),
                }
            }
            "--reaction" => {
                settings.reaction =
                    Duration::from_millis(value().parse().unwrap_or_else(|_| usage()))
            }
            "--policy" => {
                settings.policy = match value().as_str() {
                    "default" => Policy::Default,
                    "ungated" => Policy::Ungated,
                    "frequency" => Policy::FrequencyOnly,
                    "frequency-always" => Policy::FrequencyAlways,
                    "confidence" => Policy::Confidence,
                    "gate" => {
                        gate = true;
                        Policy::Default
                    }
                    _ => usage(),
                }
            }
            "--gate-prior" => prior = value().parse().unwrap_or_else(|_| usage()),
            "--gate-scale" => scale = value().parse().unwrap_or_else(|_| usage()),
            "--gate-long" => long_bar = true,
            "--typist" => {
                settings.typist = match value().as_str() {
                    "tab" => TypistKind::Tab,
                    "word" => TypistKind::Word,
                    "blind" => TypistKind::Blind,
                    "slip" => TypistKind::Slip,
                    _ => usage(),
                }
            }
            "--splits" => splits = value().split(',').map(str::to_string).collect(),
            "--writers" => prefix = value(),
            "--threads" => threads = value().parse().unwrap_or_else(|_| usage()),
            "--latency" => settings.latency = true,
            _ => usage(),
        }
    }
    let (Some(corpus), Some(out)) = (corpus, out) else {
        usage()
    };
    if gate {
        settings.policy = Policy::Gate {
            prior,
            scale,
            long_bar,
        };
    }
    let corpus = Corpus::open(&corpus).unwrap_or_else(|error| {
        eprintln!("couldn't read the corpus: {error}");
        std::process::exit(1);
    });
    let writers: Vec<(String, String)> = corpus
        .writers()
        .into_iter()
        .filter(|(id, split)| splits.contains(split) && id.starts_with(&prefix))
        .collect();
    if writers.is_empty() {
        eprintln!("no writers in the splits {splits:?} start with {prefix:?}");
        std::process::exit(1);
    }
    std::fs::create_dir_all(&out).expect("the run folder");
    let run = serde_json::json!({
        "corpus": corpus.root().display().to_string(),
        "settings": settings.describe(),
        "splits": splits,
        "writers": writers.len(),
        "threads": threads,
    });
    std::fs::write(
        out.join("run.json"),
        serde_json::to_vec_pretty(&run).unwrap(),
    )
    .expect("run.json");

    let started = Instant::now();
    let queue = Arc::new(Mutex::new(writers));
    let (sender, results) = mpsc::channel::<String>();
    let corpus = Arc::new(corpus);
    let workers: Vec<_> = (0..threads.max(1))
        .map(|_| {
            let queue = queue.clone();
            let sender = sender.clone();
            let corpus = corpus.clone();
            let settings = settings.clone();
            std::thread::spawn(move || {
                let dictionary = gpui_kit_writing::replay::english();
                loop {
                    let Some((id, split)) = queue.lock().unwrap().pop() else {
                        return;
                    };
                    let pieces = corpus.pieces(&id).expect("the writer's stream");
                    let result = typist::replay(&id, &split, &pieces, &settings, &dictionary);
                    sender
                        .send(serde_json::to_string(&result).unwrap())
                        .unwrap();
                }
            })
        })
        .collect();
    drop(sender);
    let mut file = std::fs::File::create(out.join("writers.jsonl")).expect("writers.jsonl");
    let mut done = 0;
    for line in results {
        writeln!(file, "{line}").unwrap();
        done += 1;
        eprintln!("{done} writers, {:.0?}", started.elapsed());
    }
    for worker in workers {
        worker.join().unwrap();
    }
}
