//! Batch rename: rules → preview plan → collision-safe execution.
//!
//! Rules apply left-to-right to each filename. The plan is pure and cheap:
//! it never touches the disk except `exists()` checks on targets. Execution
//! stages through unique temp names whenever a target is occupied by another
//! rename source or exists on disk — so cycles (A↔B) and case-only renames
//! on case-insensitive filesystems are safe.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::conflict::validate_filename;
use crate::copy::{CopyCtx, cancelled, note_error};

#[derive(Clone, Debug)]
pub enum RenameRule {
    /// Literal or regex find/replace over the full filename.
    FindReplace {
        find: String,
        replace: String,
        regex: bool,
        case_sensitive: bool,
    },
    /// Append `_N` (or `N_` prefix) with padding, `n` starting at `start`.
    Numbering {
        start: u32,
        pad: usize,
        prefix: bool,
    },
    ChangeCase(CaseKind),
    /// Replace the extension (empty string removes it).
    SetExtension(String),
    /// Tokens: {name} {ext} {n} {n:0<pad>} {parent}
    Template(String),
}

#[derive(Clone, Copy, Debug)]
pub enum CaseKind {
    Upper,
    Lower,
    Title,
}

#[derive(Clone, Debug)]
pub struct RenameAction {
    pub src: PathBuf,
    pub dst: PathBuf,
    pub status: RenameStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameStatus {
    Ready,
    /// New name equals the old name.
    Unchanged,
    /// Target name exists on disk and isn't one of the sources being renamed.
    Conflict,
    /// Two planned items want the same target.
    DuplicateTarget,
    Invalid(String),
}

impl RenameStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self, RenameStatus::Ready)
    }
}

fn split_name(file_name: &str) -> (String, String) {
    match file_name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !file_name.starts_with('.') => {
            (s.to_string(), e.to_string())
        }
        // dotfile like ".env": no extension
        _ => (file_name.to_string(), String::new()),
    }
}

fn apply_template(tpl: &str, name: &str, ext: &str, n: u32, parent: &str) -> String {
    let mut out = String::with_capacity(tpl.len() + 16);
    let mut rest = tpl;
    while let Some(pos) = rest.find('{') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let Some(end) = after.find('}') else {
            out.push('{');
            out.push_str(after);
            return out;
        };
        let token = &after[..end];
        match token {
            "name" => out.push_str(name),
            "ext" => out.push_str(ext),
            "n" => out.push_str(&n.to_string()),
            "parent" => out.push_str(parent),
            _ if token.starts_with("n:0") => {
                let pad: usize = token[3..].parse().unwrap_or(0);
                out.push_str(&format!("{n:0>pad$}"));
            }
            _ => {
                out.push('{');
                out.push_str(token);
                out.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn apply_rules(file_name: &str, parent: &str, index: usize, rules: &[RenameRule]) -> String {
    let (mut stem, mut ext) = split_name(file_name);
    let mut n = index as u32;
    for rule in rules {
        match rule {
            RenameRule::FindReplace {
                find,
                replace,
                regex,
                case_sensitive,
            } => {
                let apply = |s: &mut String| {
                    if find.is_empty() {
                        return;
                    }
                    if *regex {
                        if let Ok(re) = regex::Regex::new(find) {
                            *s = re.replace_all(s, replace.as_str()).into_owned();
                        }
                    } else if *case_sensitive {
                        *s = s.replace(find.as_str(), replace);
                    } else {
                        // ASCII fast path plus Unicode fold fallback.
                        if find.is_ascii() && s.is_ascii() {
                            let mut out = String::with_capacity(s.len());
                            let lower_s = s.to_ascii_lowercase();
                            let lower_f = find.to_ascii_lowercase();
                            let mut pos = 0;
                            while let Some(hit) = lower_s[pos..].find(&lower_f) {
                                let at = pos + hit;
                                out.push_str(&s[pos..at]);
                                out.push_str(replace);
                                pos = at + find.len();
                            }
                            out.push_str(&s[pos..]);
                            *s = out;
                        } else {
                            let lower_s = s.to_lowercase();
                            let lower_f = find.to_lowercase();
                            let mut out = String::with_capacity(s.len());
                            let mut pos = 0;
                            while let Some(hit) = lower_s[pos..].find(&lower_f) {
                                let at = pos + hit;
                                out.push_str(&s[pos..at]);
                                out.push_str(replace);
                                pos = at + lower_f.len();
                            }
                            out.push_str(&s[pos..]);
                            *s = out;
                        }
                    }
                };
                apply(&mut stem);
                // Apply to extension too? Conventional tools don't; we only
                // rewrite the stem. Extension changes use SetExtension.
            }
            RenameRule::Numbering { start, pad, prefix } => {
                n = start + index as u32;
                let num = format!("{n:0>pad$}");
                if *prefix {
                    stem = format!("{num}_{stem}");
                } else {
                    stem = format!("{stem}_{num}");
                }
            }
            RenameRule::ChangeCase(kind) => {
                stem = match kind {
                    CaseKind::Upper => stem.to_uppercase(),
                    CaseKind::Lower => stem.to_lowercase(),
                    CaseKind::Title => {
                        let mut out = String::with_capacity(stem.len());
                        let mut boundary = true;
                        for c in stem.chars() {
                            if boundary && c.is_alphabetic() {
                                out.extend(c.to_uppercase());
                                boundary = false;
                            } else {
                                boundary = !c.is_alphanumeric();
                                out.push(c);
                            }
                        }
                        out
                    }
                };
            }
            RenameRule::SetExtension(new) => ext = new.clone(),
            RenameRule::Template(tpl) => {
                let rendered = apply_template(tpl, &stem, &ext, n, parent);
                let (s, e) = split_name(&rendered);
                stem = s;
                ext = e;
            }
        }
    }
    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

/// Compute the full rename plan. `sources` must all be file paths (dirs work
/// too — "extension" rules treat a dir's full name as its stem).
pub fn plan_renames(sources: &[PathBuf], rules: &[RenameRule]) -> Vec<RenameAction> {
    // Source names per parent for cycle detection.
    let mut src_names: HashMap<(PathBuf, String), ()> = HashMap::with_capacity(sources.len());
    for s in sources {
        if let (Some(p), Some(n)) = (s.parent(), s.file_name()) {
            src_names.insert((p.to_path_buf(), n.to_string_lossy().to_lowercase()), ());
        }
    }
    let mut targets: HashMap<(PathBuf, String), usize> = HashMap::with_capacity(sources.len());
    let mut out = Vec::with_capacity(sources.len());

    for (i, src) in sources.iter().enumerate() {
        let parent = src.parent().map(Path::to_path_buf).unwrap_or_default();
        let parent_name = parent
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(name) = src.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            out.push(RenameAction {
                src: src.clone(),
                dst: src.clone(),
                status: RenameStatus::Invalid("no filename".into()),
            });
            continue;
        };
        let new_name = apply_rules(&name, &parent_name, i, rules);
        let dst = parent.join(&new_name);
        let status = if new_name == name {
            RenameStatus::Unchanged
        } else if let Err(e) = validate_filename(&new_name) {
            RenameStatus::Invalid(e)
        } else {
            let key = (parent.clone(), new_name.to_lowercase());
            if let Some(_prev) = targets.insert(key.clone(), i) {
                RenameStatus::DuplicateTarget
            } else if src_names.contains_key(&key) {
                // Target is another source in this plan → cycle; staged exec handles it.
                RenameStatus::Ready
            } else if dst.exists() || dst.symlink_metadata().is_ok() {
                RenameStatus::Conflict
            } else {
                RenameStatus::Ready
            }
        };
        out.push(RenameAction {
            src: src.clone(),
            dst,
            status,
        });
    }
    out
}

/// A `.fluxstage-*` sibling name that is currently free, so staging a file
/// can never overwrite a leftover from a crashed run (or a user file that
/// happens to match the pattern).
fn staging_path(dst: &Path, tag: u64, i: usize) -> PathBuf {
    for n in 0..100u32 {
        let mut name = OsString::from(".fluxstage-");
        name.push(format!("{tag}-{i}-{n}"));
        let p = dst.with_file_name(name);
        if p.symlink_metadata().is_err() {
            return p;
        }
    }
    // Should be unreachable; fall back to a PID-suffixed name.
    let mut name = OsString::from(".fluxstage-");
    name.push(format!("{}-{}-{}", tag, i, std::process::id()));
    dst.with_file_name(name)
}

/// Execute a plan. `Unchanged`/`Invalid`/`DuplicateTarget`/`Conflict` actions
/// are skipped (they were shown to the user in the preview). `Ready` actions
/// whose target collides with a live source or an existing file go through a
/// staging name. Returns when done/cancelled; per-item errors land in
/// progress.errors.
pub fn execute_renames(actions: &[RenameAction], c: &CopyCtx<'_>) {
    if let Ok(mut p) = c.progress.lock() {
        p.state = crate::OpState::Running;
        p.total_items = actions.iter().filter(|a| a.status.is_ready()).count() as u64;
        p.done_items = 0;
    }
    // Pass 1: direct renames for non-colliding targets; stage the rest.
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new(); // (tmp, final)
    for (i, a) in actions.iter().enumerate() {
        if cancelled(c) {
            return;
        }
        if !a.status.is_ready() {
            continue;
        }
        if let Ok(mut p) = c.progress.lock() {
            p.current_item = a.src.clone();
        }
        let collides = a.dst.exists() || a.dst.symlink_metadata().is_ok();
        if !collides {
            match std::fs::rename(&a.src, &a.dst) {
                Ok(()) => {
                    if let Ok(mut p) = c.progress.lock() {
                        p.done_items += 1;
                    }
                }
                Err(e) => note_error(c, format!("rename {}: {e}", a.src.display())),
            }
        } else {
            let tmp = staging_path(&a.dst, c.tag, i);
            match std::fs::rename(&a.src, &tmp) {
                Ok(()) => staged.push((tmp, a.dst.clone())),
                Err(e) => note_error(c, format!("stage {}: {e}", a.src.display())),
            }
        }
    }
    finish_staged(&staged, c);
}

/// Pass 2: staged temps → final targets (targets are free by now).
/// On cancel every leftover mapping is reported so the user can recover
/// the `.fluxstage-*` files by hand — sources can't be blindly restored
/// because their names may already have been reused.
#[doc(hidden)]
pub fn finish_staged(staged: &[(PathBuf, PathBuf)], c: &CopyCtx<'_>) {
    for (tmp, dst) in staged {
        if cancelled(c) {
            note_error(
                c,
                format!(
                    "cancelled: {} left staged (intended {})",
                    tmp.display(),
                    dst.display()
                ),
            );
            continue;
        }
        if dst.exists() || dst.symlink_metadata().is_ok() {
            note_error(
                c,
                format!(
                    "{}: target appeared during rename; left as {}",
                    dst.display(),
                    tmp.display()
                ),
            );
            continue;
        }
        match std::fs::rename(tmp, dst) {
            Ok(()) => {
                if let Ok(mut p) = c.progress.lock() {
                    p.done_items += 1;
                }
            }
            Err(e) => note_error(c, format!("rename {}: {e}", dst.display())),
        }
    }
}
