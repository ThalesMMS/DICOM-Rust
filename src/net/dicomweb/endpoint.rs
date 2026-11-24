use super::mimeheader::trim_space;
use super::types::{Endpoint, Error, InstanceRef};
use super::urlrules::{self, Url, Values};
pub(crate) fn path_clean(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let p = path.as_bytes();
    let rooted = p[0] == b'/';
    let n = p.len();
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let (mut r, mut dotdot) = (0, 0);
    if rooted {
        out.push(b'/');
        r = 1;
        dotdot = 1;
    }
    while r < n {
        if p[r] == b'/' || (p[r] == b'.' && (r + 1 == n || p[r + 1] == b'/')) {
            r += 1;
        } else if p[r] == b'.' && p[r + 1] == b'.' && (r + 2 == n || p[r + 2] == b'/') {
            r += 2;
            if out.len() > dotdot {
                out.pop();
                while out.len() > dotdot && out[out.len() - 1] != b'/' {
                    out.pop();
                }
                if out.len() > dotdot {
                    out.pop();
                }
            } else if !rooted {
                if !out.is_empty() {
                    out.push(b'/');
                }
                out.extend_from_slice(b"..");
                dotdot = out.len();
            }
        } else {
            if (rooted && out.len() != 1) || (!rooted && !out.is_empty()) {
                out.push(b'/');
            }
            while r < n && p[r] != b'/' {
                out.push(p[r]);
                r += 1;
            }
        }
    }
    if out.is_empty() {
        return ".".into();
    }
    String::from_utf8_lossy(&out).into_owned()
}
pub(crate) fn path_join(parts: &[String]) -> String {
    let kept: Vec<&str> = parts
        .iter()
        .map(String::as_str)
        .filter(|p| !p.is_empty())
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    path_clean(&kept.join("/"))
}
pub(crate) fn path_base(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let trimmed = path.trim_end_matches('/');
    let base = match trimmed.rfind('/') {
        Some(i) => &trimmed[i + 1..],
        None => trimmed,
    };
    if base.is_empty() {
        return "/".into();
    }
    base.to_string()
}
pub(crate) fn path_dir(path: &str) -> String {
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "",
    };
    path_clean(dir)
}
fn escaped_resource(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| trim_space(part).trim_matches('/'))
        .filter(|part| !part.is_empty())
        .map(urlrules::path_escape)
        .collect::<Vec<_>>()
        .join("/")
}
pub(crate) fn join_url_path(base_path: &str, service_path: &str, resource: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for value in [base_path, service_path] {
        for part in value.trim_matches('/').split('/') {
            if !part.is_empty() {
                parts.push(part.to_string());
            }
        }
    }
    let mut resource = resource.trim_matches('/');
    if !resource.is_empty() {
        let last = parts.last().map_or("", String::as_str);
        let first = resource.split('/').next().unwrap_or("");
        if crate::textfmt::to_upper(last) == crate::textfmt::to_upper(first)
            || super::mimeheader::to_lower(last) == super::mimeheader::to_lower(first)
        {
            let rest = &resource[first.len()..];
            resource = rest.strip_prefix('/').unwrap_or(rest);
        }
        if !resource.is_empty() {
            parts.push(resource.to_string());
        }
    }
    if parts.is_empty() {
        return "/".into();
    }
    format!("/{}", path_join(&parts))
}
fn required(value: &str, what: &str) -> Result<String, Error> {
    let value = trim_space(value);
    if value.is_empty() {
        return Err(Error::invalid_endpoint(&format!("{what} is required")));
    }
    Ok(value.to_string())
}
const STUDY: &str = "study instance UID";
const SERIES: &str = "series instance UID";
const SOP: &str = "SOP instance UID";
impl Endpoint {
    pub fn study_search_url(&self, params: &Values) -> Result<Url, Error> {
        self.service_url(&self.qido_path, &["studies"], Some(params))
    }
    pub fn series_search_url(&self, study: &str, params: &Values) -> Result<Url, Error> {
        let study = required(study, STUDY)?;
        self.service_url(
            &self.qido_path,
            &["studies", &study, "series"],
            Some(params),
        )
    }
    pub fn instance_search_url(
        &self,
        study: &str,
        series: &str,
        params: &Values,
    ) -> Result<Url, Error> {
        let study_uid = trim_space(study);
        let series_uid = trim_space(series);
        required(study_uid, STUDY)?;
        required(series_uid, SERIES)?;
        self.service_url(
            &self.qido_path,
            &["studies", study_uid, "series", series_uid, "instances"],
            Some(params),
        )
    }
    pub fn study_metadata_url(&self, study: &str) -> Result<Url, Error> {
        let study = required(study, STUDY)?;
        self.service_url(&self.wado_path, &["studies", &study, "metadata"], None)
    }
    pub fn study_url(&self, study: &str) -> Result<Url, Error> {
        let study = required(study, STUDY)?;
        self.service_url(&self.wado_path, &["studies", &study], None)
    }
    pub fn series_metadata_url(&self, study: &str, series: &str) -> Result<Url, Error> {
        let study = required(study, STUDY)?;
        let series = required(series, SERIES)?;
        self.service_url(
            &self.wado_path,
            &["studies", &study, "series", &series, "metadata"],
            None,
        )
    }
    pub fn series_url(&self, study: &str, series: &str) -> Result<Url, Error> {
        let study = required(study, STUDY)?;
        let series = required(series, SERIES)?;
        self.service_url(
            &self.wado_path,
            &["studies", &study, "series", &series],
            None,
        )
    }
    pub fn instance_metadata_url(&self, instance: &InstanceRef) -> Result<Url, Error> {
        let mut url = self.instance_url(instance)?;
        url.path = join_url_path(&url.path, "", "metadata");
        Ok(url)
    }
    pub fn instance_url(&self, instance: &InstanceRef) -> Result<Url, Error> {
        let study = required(&instance.study_instance_uid, STUDY)?;
        let series = required(&instance.series_instance_uid, SERIES)?;
        let sop = required(&instance.sop_instance_uid, SOP)?;
        self.service_url(
            &self.wado_path,
            &["studies", &study, "series", &series, "instances", &sop],
            None,
        )
    }
    pub fn frames_url(&self, instance: &InstanceRef, frames: &[i64]) -> Result<Url, Error> {
        let mut url = self.instance_url(instance)?;
        if frames.is_empty() {
            return Err(Error::invalid_endpoint(
                "at least one frame number is required",
            ));
        }
        let mut values: Vec<String> = Vec::with_capacity(frames.len());
        for (index, &frame) in frames.iter().enumerate() {
            if frame <= 0 {
                return Err(Error::invalid_endpoint("frame numbers are one-based"));
            }
            if frames[..index].contains(&frame) {
                return Err(Error::invalid_endpoint(&format!(
                    "duplicate frame number {frame}"
                )));
            }
            values.push(frame.to_string());
        }
        url.path = join_url_path(&url.path, "", &format!("frames/{}", values.join(",")));
        Ok(url)
    }
    pub fn store_studies_url(&self) -> Result<Url, Error> {
        self.service_url(&self.stow_path, &["studies"], None)
    }
    pub fn store_study_url(&self, study: &str) -> Result<Url, Error> {
        let study = required(study, STUDY)?;
        self.service_url(&self.stow_path, &["studies", &study], None)
    }
    pub(crate) fn service_url(
        &self,
        service_path: &str,
        parts: &[&str],
        params: Option<&Values>,
    ) -> Result<Url, Error> {
        let mut url = self.service_base()?;
        url.path = join_url_path(&url.path, service_path, &escaped_resource(parts));
        url.raw_query = params.map(Values::encode).unwrap_or_default();
        Ok(url)
    }
    pub(crate) fn service_root_url(
        &self,
        service_path: &str,
        parts: &[&str],
        params: Option<&Values>,
    ) -> Result<Url, Error> {
        let mut url = self.service_base()?;
        let mut root = join_url_path(&url.path, service_path, "");
        if super::mimeheader::to_lower(&path_base(&root)) == "studies"
            || crate::textfmt::to_upper(&path_base(&root)) == "STUDIES"
        {
            root = path_dir(&root);
        }
        url.path = join_url_path(&root, "", &escaped_resource(parts));
        url.raw_query = params.map(Values::encode).unwrap_or_default();
        Ok(url)
    }
    pub(crate) fn service_base(&self) -> Result<Url, Error> {
        let raw = trim_space(&self.base_url);
        if raw.is_empty() {
            return Err(Error::invalid_endpoint("base URL cannot be empty"));
        }
        let Ok(url) = urlrules::parse(raw) else {
            return Err(Error::invalid_endpoint("base URL is malformed"));
        };
        if url.scheme != "http" && url.scheme != "https" {
            return Err(Error::invalid_endpoint("base URL must use http or https"));
        }
        if url.host.is_empty() {
            return Err(Error::invalid_endpoint("base URL must include a host"));
        }
        if url.user.is_some() {
            return Err(Error::invalid_endpoint(
                "base URL must not include user information",
            ));
        }
        if !url.raw_query.is_empty() || url.force_query {
            return Err(Error::invalid_endpoint("base URL must not include a query"));
        }
        if !url.fragment.is_empty() || !url.raw_fragment.is_empty() {
            return Err(Error::invalid_endpoint(
                "base URL must not include a fragment",
            ));
        }
        Ok(url)
    }
}
