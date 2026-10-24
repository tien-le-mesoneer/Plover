//! Run reports (JUnit, JSON, HTML) rendered from a masked `RunRecord`, so `plv run`,
//! `plv runs show` and the app produce the same files.

pub mod html;
pub mod json;
pub mod junit;

use crate::run_record::{ResultRecord, RunRecord};

/// The iterations a report shows, in order, with their results: every iteration that has
/// results or is listed in `record.iterations` (one stopped before its first request), and
/// always at least iteration 0.
pub(crate) fn iterations(record: &RunRecord) -> Vec<(usize, Vec<&ResultRecord>)> {
    let mut indices: Vec<usize> = record
        .results
        .iter()
        .map(|r| r.iteration_index)
        .chain(record.iterations.iter().map(|i| i.index))
        .collect();
    indices.sort_unstable();
    indices.dedup();
    if indices.is_empty() {
        indices.push(0);
    }
    indices
        .into_iter()
        .map(|i| {
            let results = record
                .results
                .iter()
                .filter(|r| r.iteration_index == i)
                .collect();
            (i, results)
        })
        .collect()
}

/// Bruno CLI's `--reporter-skip-*` flags (sanitize-results.js): what to leave out of the
/// JSON and HTML reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportFilter {
    pub skip_all_headers: bool,
    /// Header names, matched case-insensitively.
    pub skip_headers: Vec<String>,
    pub skip_request_body: bool,
    pub skip_response_body: bool,
}

impl ReportFilter {
    /// The headers a report shows.
    pub fn headers<'a>(&self, headers: &'a [(String, String)]) -> Vec<&'a (String, String)> {
        if self.skip_all_headers {
            return Vec::new();
        }
        headers
            .iter()
            .filter(|(name, _)| {
                !self
                    .skip_headers
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(name))
            })
            .collect()
    }
}
