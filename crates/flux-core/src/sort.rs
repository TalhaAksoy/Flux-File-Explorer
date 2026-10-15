use std::cmp::Ordering;

use crate::entry::{DirEntry, EntryKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Modified,
    Extension,
    Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// How a view is ordered. `natural` makes `file2` < `file10`.
/// `case_sensitive = false` compares names with Unicode lowercase folding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub key: SortKey,
    pub dir: SortDir,
    pub dirs_first: bool,
    pub natural: bool,
    pub case_sensitive: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            key: SortKey::Name,
            dir: SortDir::Asc,
            dirs_first: true,
            natural: true,
            case_sensitive: false,
        }
    }
}

impl SortSpec {
    /// Clicking a header column returns the spec to apply: same key toggles
    /// direction, different key resets to Asc.
    pub fn for_header_click(current: &SortSpec, key: SortKey) -> SortSpec {
        if current.key == key {
            SortSpec {
                dir: match current.dir {
                    SortDir::Asc => SortDir::Desc,
                    SortDir::Desc => SortDir::Asc,
                },
                ..*current
            }
        } else {
            SortSpec {
                key,
                dir: SortDir::Asc,
                ..*current
            }
        }
    }
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// Consume and return the next char, folded to lowercase when `ci`.
/// Multi-char folds (rare, e.g. 'İ') yield only their first char — a
/// documented best-effort simplification that keeps the comparator
/// allocation-free and correct for ASCII and almost all real filenames.
fn next_folded(it: &mut Chars<'_>, ci: bool) -> Option<char> {
    let c = it.next()?;
    if !ci {
        return Some(c);
    }
    c.to_lowercase().next()
}

/// Consume a run of ASCII digits starting from `first` (already consumed).
/// Runs past 64 digits are truncated; the ordering stays total and stable.
fn take_digit_run(it: &mut Chars<'_>, first: char) -> String {
    let mut s = String::with_capacity(16);
    s.push(first);
    while s.len() < 64 {
        match it.peek() {
            Some(&c) if c.is_ascii_digit() => {
                s.push(c);
                it.next();
            }
            _ => break,
        }
    }
    s
}

/// Zero-allocation natural compare: digit runs compare numerically
/// (value first; a purely-equal pair falls back to fewer leading zeros).
pub fn natural_cmp(a: &str, b: &str, ci: bool) -> Ordering {
    let mut ia = a.chars().peekable();
    let mut ib = b.chars().peekable();
    loop {
        let (x, y) = match (next_folded(&mut ia, ci), next_folded(&mut ib, ci)) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => (x, y),
        };
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let run_a = take_digit_run(&mut ia, x);
            let run_b = take_digit_run(&mut ib, y);
            let ta = run_a.trim_start_matches('0');
            let tb = run_b.trim_start_matches('0');
            let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
            if ord != Ordering::Equal {
                return ord;
            }
            if run_a.len() != run_b.len() {
                return run_a.len().cmp(&run_b.len());
            }
            continue;
        }
        if x != y {
            return x.cmp(&y);
        }
    }
}

/// Zero-allocation folded compare without digit-run semantics.
pub fn folded_cmp(a: &str, b: &str, ci: bool) -> Ordering {
    let mut ia = a.chars().peekable();
    let mut ib = b.chars().peekable();
    loop {
        match (next_folded(&mut ia, ci), next_folded(&mut ib, ci)) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x == y => continue,
            (Some(x), Some(y)) => return x.cmp(&y),
        }
    }
}

/// Name compare honoring the spec's case sensitivity and natural flag.
pub fn name_cmp(a: &str, b: &str, spec: &SortSpec) -> Ordering {
    if spec.natural {
        natural_cmp(a, b, !spec.case_sensitive)
    } else {
        folded_cmp(a, b, !spec.case_sensitive)
    }
}

/// Compare two entries per `spec`. `dirs_first` partitions before the key;
/// name breaks ties so ordering is total across view rebuilds.
pub fn cmp_entries(a: &DirEntry, b: &DirEntry, spec: &SortSpec) -> Ordering {
    if spec.dirs_first {
        match (a.is_dir(), b.is_dir()) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
    }
    let ord = match spec.key {
        SortKey::Name => Ordering::Equal, // decided by tie-break below
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Modified => match (a.mtime, b.mtime) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => Ordering::Greater, // known sorts after unknown
            (None, Some(_)) => Ordering::Less,
            (None, None) => Ordering::Equal,
        },
        SortKey::Extension => {
            let ea = a
                .extension()
                .map(|e| e.to_string_lossy())
                .unwrap_or_default();
            let eb = b
                .extension()
                .map(|e| e.to_string_lossy())
                .unwrap_or_default();
            folded_cmp(&ea, &eb, true)
        }
        SortKey::Kind => {
            let rank = |e: &DirEntry| match e.kind {
                EntryKind::Dir => 0,
                EntryKind::Symlink => 1,
                EntryKind::File => 2,
                EntryKind::Other => 3,
            };
            rank(a).cmp(&rank(b))
        }
    };
    let ord =
        ord.then_with(|| name_cmp(&a.name.to_string_lossy(), &b.name.to_string_lossy(), spec));
    match spec.dir {
        SortDir::Asc => ord,
        SortDir::Desc => ord.reverse(),
    }
}

/// Build the ordered index list for `entries` under `spec`, restricted to
/// entries passing `filter` and the hidden-file policy.
pub fn build_view(
    entries: &[DirEntry],
    spec: &SortSpec,
    filter: Option<&str>,
    include_hidden: bool,
) -> Vec<u32> {
    let mut view: Vec<u32> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| (include_hidden || !e.hidden) && e.matches_filter(filter))
        .map(|(i, _)| i as u32)
        .collect();
    view.sort_unstable_by(|&ia, &ib| {
        cmp_entries(&entries[ia as usize], &entries[ib as usize], spec)
    });
    view
}
