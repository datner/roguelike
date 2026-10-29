//! Calibration (docs/plans/calibration.md): `playtime.csv`, written by the frontends beside the
//! saves, read back as keypresses per hour, overall and per character, for the owner to set
//! `actions_per_hour` in `bench/targets.ron` by hand.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Keypresses and seconds, summed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Play {
    keypresses: u64,
    seconds: u64,
}

impl Play {
    fn per_hour(self) -> Option<f64> {
        (self.seconds > 0).then(|| self.keypresses as f64 * 3600.0 / self.seconds as f64)
    }
}

/// The report for `text` (the file's contents): one line overall, then one
/// per character. Lines that don't parse are counted and skipped.
pub fn calibrate(text: &str) -> String {
    let mut all = Play::default();
    let mut by: BTreeMap<&str, Play> = BTreeMap::new();
    let mut skipped = 0;
    for line in text.lines().skip(1).filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split(',').collect();
        let parsed = match fields.as_slice() {
            [name, keys, secs] => keys
                .trim()
                .parse::<u64>()
                .ok()
                .zip(secs.trim().parse::<u64>().ok())
                .map(|(keypresses, seconds)| {
                    (
                        *name,
                        Play {
                            keypresses,
                            seconds,
                        },
                    )
                }),
            _ => None,
        };
        let Some((name, play)) = parsed else {
            skipped += 1;
            continue;
        };
        for sum in [&mut all, by.entry(name).or_default()] {
            sum.keypresses += play.keypresses;
            sum.seconds += play.seconds;
        }
    }
    let rate = |play: Play| {
        play.per_hour().map_or_else(
            || "no time played".to_string(),
            |rate| format!("{rate:.0} keypresses an hour"),
        )
    };
    let mut out = String::new();
    if by.is_empty() {
        let _ = writeln!(out, "No play recorded yet.");
        return out;
    }
    let hours = all.seconds as f64 / 3600.0;
    let _ = writeln!(
        out,
        "All: {} keypresses in {hours:.1} h: {}",
        all.keypresses,
        rate(all)
    );
    for (name, play) in &by {
        let _ = writeln!(
            out,
            "{name}: {} keypresses: {}",
            play.keypresses,
            rate(*play)
        );
    }
    if skipped > 0 {
        let _ = writeln!(out, "({skipped} lines could not be read)");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_gives_its_rate_overall_and_per_character() {
        let text = "character,keypresses,seconds\nana,500,1800\nbo,250,900\nana,bad,line\n";
        assert_eq!(
            calibrate(text),
            "All: 750 keypresses in 0.8 h: 1000 keypresses an hour\n\
             ana: 500 keypresses: 1000 keypresses an hour\n\
             bo: 250 keypresses: 1000 keypresses an hour\n\
             (1 lines could not be read)\n"
        );
    }

    #[test]
    fn an_empty_file_says_so() {
        assert_eq!(
            calibrate("character,keypresses,seconds\n"),
            "No play recorded yet.\n"
        );
    }
}
