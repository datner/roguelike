//! What moved between two reports (docs/plans/bench-cli.md): their rows matched by section and name,
//! so a change reads as one row's before and after, not as a diff's removed and added lines.

/// One row that changed, appeared or went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The heading (`##` or `###`) the row sits under.
    pub section: String,
    pub was: Option<String>,
    pub now: Option<String>,
}

/// The rows of `new` that differ from `old`'s, in `new`'s order, then the rows `old` had and
/// `new` lacks.
pub fn moved(old: &str, new: &str) -> Vec<Change> {
    let old = rows(old);
    let new = rows(new);
    let mut changes: Vec<Change> = new
        .iter()
        .filter_map(|(key, line)| {
            let was = old.iter().find(|(k, _)| k == key).map(|(_, l)| l);
            (was != Some(line)).then(|| Change {
                section: key.0.clone(),
                was: was.cloned(),
                now: Some(line.clone()),
            })
        })
        .collect();
    changes.extend(
        old.iter()
            .filter(|(key, _)| !new.iter().any(|(k, _)| k == key))
            .map(|(key, line)| Change {
                section: key.0.clone(),
                was: Some(line.clone()),
                now: None,
            }),
    );
    changes
}

/// How many to list before saying how many more: enough to see what a change did.
const LISTED: usize = 20;

/// `changes` for a terminal: a count, then each row's was and now under its section.
pub fn summary(changes: &[Change]) -> String {
    use std::fmt::Write as _;
    if changes.is_empty() {
        return "nothing moved against bench/report.md\n".to_string();
    }
    let mut out = format!("{} rows moved against bench/report.md:\n", changes.len());
    let mut section = "";
    for change in changes.iter().take(LISTED) {
        if change.section != section {
            section = &change.section;
            let _ = writeln!(out, "  {section}");
        }
        for (label, line) in [("was", &change.was), ("now", &change.now)] {
            let line = line.as_deref().unwrap_or("(none)");
            let _ = writeln!(out, "    {label} {line}");
        }
    }
    if changes.len() > LISTED {
        let _ = writeln!(out, "  … and {} more", changes.len() - LISTED);
    }
    out
}

/// (section, name, how many rows of that name came before in the section) for each table row
/// and bullet, with its line. A table row's name is its first cell (a dive's floors repeat the
/// build, so the count tells them apart), a bullet's what comes before its first colon.
type Keyed = Vec<((String, String, usize), String)>;

fn rows(report: &str) -> Keyed {
    let mut section = String::new();
    let mut keyed: Keyed = Vec::new();
    for line in report.lines() {
        // Each dive is a `###` under Floors: the nearest heading of either level.
        if let Some(heading) = line
            .strip_prefix("### ")
            .or_else(|| line.strip_prefix("## "))
        {
            section = heading.to_string();
            continue;
        }
        let name = if let Some(cells) = line.strip_prefix('|') {
            let first = cells.split('|').next().unwrap_or("").trim();
            // The header and its rule are the same in every report.
            if first.is_empty() || first.starts_with('-') || first == "build" {
                continue;
            }
            first
        } else if let Some(bullet) = line.strip_prefix("- ") {
            bullet.split(':').next().unwrap_or(bullet)
        } else {
            continue;
        };
        let seen = keyed
            .iter()
            .filter(|((s, n, _), _)| *s == section && n == name)
            .count();
        keyed.push(((section.clone(), name.to_string(), seen), line.to_string()));
    }
    keyed
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "\
## Caves
| build | floor |
|---|---|
| duelist | 1 | 50 |
| duelist | 2 | 60 |
| archer | 1 | 40 |
- **duelist**: 3 died
- **archer**: 2 died
";

    #[test]
    fn rows_match_by_section_name_and_place() {
        let new = OLD
            .replace("| duelist | 2 | 60 |", "| duelist | 2 | 65 |")
            .replace("- **archer**: 2 died\n", "");
        let changes = moved(OLD, &new);
        assert_eq!(
            changes,
            [
                Change {
                    section: "Caves".into(),
                    was: Some("| duelist | 2 | 60 |".into()),
                    now: Some("| duelist | 2 | 65 |".into()),
                },
                Change {
                    section: "Caves".into(),
                    was: Some("- **archer**: 2 died".into()),
                    now: None,
                },
            ]
        );
        assert!(moved(OLD, OLD).is_empty());
    }
}
