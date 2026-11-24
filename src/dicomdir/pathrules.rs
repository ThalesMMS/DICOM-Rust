#![forbid(unsafe_code)]
#![allow(dead_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use std::io;
#[cfg(windows)]
pub const SEPARATOR: u8 = b'\\';
#[cfg(not(windows))]
pub const SEPARATOR: u8 = b'/';
pub fn is_sep(c: u8) -> bool {
    if cfg!(windows) {
        c == b'\\' || c == b'/'
    } else {
        c == b'/'
    }
}
#[cfg(windows)]
fn to_upper(c: u8) -> u8 {
    c.to_ascii_uppercase()
}
#[cfg(windows)]
fn path_has_prefix_fold(s: &[u8], prefix: &[u8]) -> bool {
    let Some(head) = s.get(..prefix.len()) else {
        return false;
    };
    for (&p, &c) in prefix.iter().zip(head) {
        if is_sep(p) {
            if !is_sep(c) {
                return false;
            }
        } else if to_upper(p) != to_upper(c) {
            return false;
        }
    }
    !s.get(prefix.len()).is_some_and(|&c| !is_sep(c))
}
#[cfg(windows)]
fn unc_len(path: &[u8], prefix_len: usize) -> usize {
    let mut count = 0u32;
    for (i, &c) in path.iter().enumerate().skip(prefix_len) {
        if is_sep(c) {
            count = count.saturating_add(1);
            if count == 2 {
                return i;
            }
        }
    }
    path.len()
}
pub fn volume_name_len(path: &str) -> usize {
    #[cfg(windows)]
    {
        let p = path.as_bytes();
        if p.get(1) == Some(&b':') {
            return 2;
        }
        if !p.first().is_some_and(|&c| is_sep(c)) {
            return 0;
        }
        if path_has_prefix_fold(p, b"\\\\.\\UNC") {
            return unc_len(p, "\\\\.\\UNC\\".len());
        }
        if path_has_prefix_fold(p, b"\\\\.")
            || path_has_prefix_fold(p, b"\\\\?")
            || path_has_prefix_fold(p, b"\\??")
        {
            if p.len() == 3 {
                return 3;
            }
            let rest = p.get(4..).unwrap_or_default();
            return match rest.iter().position(|&c| is_sep(c)) {
                None => p.len(),
                Some(i) => i.saturating_add(4),
            };
        }
        if p.get(1).is_some_and(|&c| is_sep(c)) {
            return unc_len(p, 2);
        }
        0
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        0
    }
}
pub fn volume_name(path: &str) -> &str {
    &path[..volume_name_len(path)]
}
pub fn is_abs(path: &str) -> bool {
    #[cfg(windows)]
    {
        let l = volume_name_len(path);
        if l == 0 {
            return false;
        }
        let p = path.as_bytes();
        if matches!(p, [a, b, ..] if is_sep(*a) && is_sep(*b)) {
            return true;
        }
        p.get(l).is_some_and(|&c| is_sep(c))
    }
    #[cfg(not(windows))]
    {
        path.starts_with('/')
    }
}
fn from_slash(path: &str) -> String {
    if SEPARATOR == b'/' {
        path.to_string()
    } else {
        path.replace('/', "\\")
    }
}
pub fn clean(original: &str) -> String {
    let vol_len = volume_name_len(original);
    let path = original.as_bytes().get(vol_len..).unwrap_or_default();
    let ob = original.as_bytes();
    if path.is_empty() {
        if vol_len > 1 && matches!(ob, [a, b, ..] if is_sep(*a) && is_sep(*b)) {
            return from_slash(original);
        }
        return format!("{original}.");
    }
    let rooted = path.first().is_some_and(|&c| is_sep(c));
    let at = |i: usize| path.get(i).copied().unwrap_or(0);
    let n = path.len();
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let (mut r, mut dotdot) = (0usize, 0usize);
    if rooted {
        out.push(SEPARATOR);
        r = 1;
        dotdot = 1;
    }
    while r < n {
        let (r1, r2) = (r.saturating_add(1), r.saturating_add(2));
        if is_sep(at(r)) || (at(r) == b'.' && (r1 == n || is_sep(at(r1)))) {
            r = r1;
        } else if at(r) == b'.' && at(r1) == b'.' && (r2 == n || is_sep(at(r2))) {
            r = r2;
            if out.len() > dotdot {
                let mut w = out.len().saturating_sub(1);
                while w > dotdot && !out.get(w).is_some_and(|&c| is_sep(c)) {
                    w = w.saturating_sub(1);
                }
                out.truncate(w);
            } else if !rooted {
                if !out.is_empty() {
                    out.push(SEPARATOR);
                }
                out.extend_from_slice(b"..");
                dotdot = out.len();
            }
        } else {
            if (rooted && out.len() != 1) || (!rooted && !out.is_empty()) {
                out.push(SEPARATOR);
            }
            while let Some(&c) = path.get(r)
                && !is_sep(c)
            {
                out.push(c);
                r = r.saturating_add(1);
            }
        }
    }
    if out.is_empty() {
        out.push(b'.');
    }
    #[cfg(windows)]
    if vol_len == 0 {
        let mut prefix: &[u8] = &[];
        for &c in &out {
            if is_sep(c) {
                break;
            }
            if c == b':' {
                prefix = b".\\";
                break;
            }
        }
        if prefix.is_empty() && matches!(out.as_slice(), [s, b'?', b'?', ..] if is_sep(*s)) {
            prefix = b"\\.";
        }
        if !prefix.is_empty() {
            let mut v = prefix.to_vec();
            v.extend_from_slice(&out);
            out = v;
        }
    }
    let mut s = original[..vol_len].to_string();
    s.push_str(&String::from_utf8_lossy(&out));
    from_slash(&s)
}
fn same_word(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        equal_fold(a, b)
    } else {
        a == b
    }
}
pub fn equal_fold(a: &str, b: &str) -> bool {
    let mut x = a.chars();
    let mut y = b.chars();
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(c), Some(d)) => {
                if c == d {
                    continue;
                }
                let fold = |c: char| {
                    let mut l = c.to_lowercase();
                    match (l.next(), l.next()) {
                        (Some(f), None) => f,
                        _ => c,
                    }
                };
                let up = |c: char| {
                    let mut u = c.to_uppercase();
                    match (u.next(), u.next()) {
                        (Some(f), None) => f,
                        _ => c,
                    }
                };
                if fold(c) != fold(d) && up(c) != up(d) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}
pub fn rel(basepath: &str, targpath: &str) -> Result<String, String> {
    let base_vol = volume_name(basepath).to_string();
    let targ_vol = volume_name(targpath).to_string();
    let base_c = clean(basepath);
    let targ_c = clean(targpath);
    if same_word(&targ_c, &base_c) {
        return Ok(".".into());
    }
    let mut base = &base_c[base_vol.len()..];
    let targ = &targ_c[targ_vol.len()..];
    let sep_s = if SEPARATOR == b'\\' { "\\" } else { "/" };
    if base == "." {
        base = "";
    } else if base.is_empty() && volume_name_len(&base_vol) > 2 {
        base = sep_s;
    }
    let base_slashed = base.as_bytes().first() == Some(&SEPARATOR);
    let targ_slashed = targ.as_bytes().first() == Some(&SEPARATOR);
    let err = || format!("Rel: can't make {targpath} relative to {basepath}");
    if base_slashed != targ_slashed || !same_word(&base_vol, &targ_vol) {
        return Err(err());
    }
    let (bb, tb) = (base.as_bytes(), targ.as_bytes());
    let (bl, tl) = (bb.len(), tb.len());
    let (mut b0, mut bi, mut t0, mut ti) = (0, 0, 0, 0);
    loop {
        while bb.get(bi).is_some_and(|&c| c != SEPARATOR) {
            bi = bi.saturating_add(1);
        }
        while tb.get(ti).is_some_and(|&c| c != SEPARATOR) {
            ti = ti.saturating_add(1);
        }
        if !same_word(&targ[t0..ti], &base[b0..bi]) {
            break;
        }
        if bi < bl {
            bi = bi.saturating_add(1);
        }
        if ti < tl {
            ti = ti.saturating_add(1);
        }
        b0 = bi;
        t0 = ti;
    }
    if &base[b0..bi] == ".." {
        return Err(err());
    }
    if b0 != bl {
        let seps = bb
            .get(b0..)
            .unwrap_or_default()
            .iter()
            .filter(|&&c| c == SEPARATOR)
            .count();
        let mut out = String::from("..");
        for _ in 0..seps {
            out.push_str(sep_s);
            out.push_str("..");
        }
        if t0 != tl {
            out.push_str(sep_s);
            out.push_str(&targ[t0..]);
        }
        return Ok(out);
    }
    Ok(targ[t0..].to_string())
}
pub fn dir(path: &str) -> String {
    let vol = volume_name(path);
    let p = path.as_bytes();
    let mut i = p.len();
    while i > vol.len() && !p.get(i.saturating_sub(1)).is_some_and(|&c| is_sep(c)) {
        i = i.saturating_sub(1);
    }
    let d = clean(&path[vol.len()..i]);
    if d == "." && vol.len() > 2 {
        return vol.to_string();
    }
    format!("{vol}{d}")
}
pub fn base(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let mut p = path;
    while let Some(&c) = p.as_bytes().last() {
        if !is_sep(c) {
            break;
        }
        p = &p[..p.len().saturating_sub(1)];
    }
    let p = &p[volume_name_len(p)..];
    let start = p
        .as_bytes()
        .iter()
        .rposition(|&c| is_sep(c))
        .map_or(0, |i| i.saturating_add(1));
    let b = &p[start..];
    if b.is_empty() {
        return (SEPARATOR as char).to_string();
    }
    b.to_string()
}
pub fn abs(path: &str) -> io::Result<String> {
    if is_abs(path) && !(cfg!(windows) && path.ends_with([' ', '.'])) {
        return Ok(clean(path));
    }
    if path.is_empty() {
        let cwd = std::env::current_dir()?;
        return Ok(clean(&cwd.to_string_lossy()));
    }
    let full = std::path::absolute(path)?;
    Ok(clean(&full.to_string_lossy()))
}
pub fn join(parts: &[&str]) -> String {
    #[cfg(windows)]
    {
        let mut b = String::new();
        let mut last: u8 = 0;
        for e in parts {
            let mut e: &str = e;
            if b.is_empty() {
            } else if is_sep(last) {
                while e.as_bytes().first().is_some_and(|&c| is_sep(c)) {
                    e = &e[1..];
                }
                if e.is_empty() {
                    continue;
                }
            } else if last == b':' {
            } else {
                b.push('\\');
                last = b'\\';
            }
            if !e.is_empty() {
                b.push_str(e);
                last = *e.as_bytes().last().unwrap_or(&0);
            }
        }
        if b.is_empty() {
            return String::new();
        }
        clean(&b)
    }
    #[cfg(not(windows))]
    {
        match parts.iter().position(|e| !e.is_empty()) {
            Some(i) => clean(&parts[i..].join("/")),
            None => String::new(),
        }
    }
}
pub(crate) fn sep_str() -> &'static str {
    if SEPARATOR == b'\\' { "\\" } else { "/" }
}
pub fn is_not_exist(e: &io::Error) -> bool {
    match e.raw_os_error() {
        #[cfg(windows)]
        Some(code) => matches!(code, 2 | 3 | 53),
        _ => e.kind() == io::ErrorKind::NotFound,
    }
}
