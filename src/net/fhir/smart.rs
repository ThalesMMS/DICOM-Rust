use super::{Error, valid_resource_id};
pub const SMART_PATIENT: &str = "patient";
pub const SMART_USER: &str = "user";
pub const SMART_SYSTEM: &str = "system";
pub fn check_smart_scopes(
    granted: &str,
    principal: &str,
    method: &str,
    resource_path: &str,
) -> Result<(), Error> {
    let Some((resource, permission)) = smart_interaction(method, resource_path) else {
        return Err(Error::SmartTarget);
    };
    if !matches!(principal, SMART_PATIENT | SMART_USER | SMART_SYSTEM) {
        return Err(Error::SmartScopeDenied);
    }
    const MAX_SCOPE_BYTES: usize = 8 << 10;
    if granted.len() > MAX_SCOPE_BYTES || !valid_scope_claim(granted) {
        return Err(Error::SmartScopeDenied);
    }
    if granted
        .split(' ')
        .any(|grant| scope_permits(grant, principal, resource, permission))
    {
        return Ok(());
    }
    Err(Error::SmartScopeDenied)
}
fn smart_interaction<'a>(method: &str, path: &'a str) -> Option<(&'a str, u8)> {
    const MAX_PATH_BYTES: usize = 256;
    if path.len() > MAX_PATH_BYTES {
        return None;
    }
    let path = path.strip_prefix('/').unwrap_or(path);
    let (resource, suffix) = match path.split_once('/') {
        Some((resource, suffix)) => (resource, Some(suffix)),
        None => (path, None),
    };
    if !valid_resource_type(resource) {
        return None;
    }
    let Some(suffix) = suffix else {
        return match method {
            "GET" => Some((resource, b's')),
            "POST" => Some((resource, b'c')),
            _ => None,
        };
    };
    if (suffix == "_search" && method == "POST") || (suffix == "_history" && method == "GET") {
        return Some((resource, b's'));
    }
    let (id, history) = match suffix.split_once('/') {
        Some((id, history)) => (id, Some(history)),
        None => (suffix, None),
    };
    if !valid_resource_id(id) {
        return None;
    }
    if let Some(history) = history {
        let (segment, version) = match history.split_once('/') {
            Some((segment, version)) => (segment, Some(version)),
            None => (history, None),
        };
        if method == "GET" && segment == "_history" && version.is_none_or(valid_resource_id) {
            return Some((resource, b'r'));
        }
        return None;
    }
    match method {
        "GET" => Some((resource, b'r')),
        "PATCH" | "PUT" => Some((resource, b'u')),
        "DELETE" => Some((resource, b'd')),
        _ => None,
    }
}
fn valid_resource_type(resource: &str) -> bool {
    let b = resource.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_uppercase()
        && b[1..].iter().all(u8::is_ascii_alphanumeric)
}
fn valid_scope_claim(claim: &str) -> bool {
    claim
        .bytes()
        .all(|c| (b' '..=b'~').contains(&c) && c != b'"' && c != b'\\')
}
fn scope_permits(scope: &str, principal: &str, resource: &str, permission: u8) -> bool {
    let Some((context, resource_scope)) = scope.split_once('/') else {
        return false;
    };
    if context != principal {
        return false;
    }
    let Some((target, permissions)) = resource_scope.split_once('.') else {
        return false;
    };
    if target != resource && target != "*" {
        return false;
    }
    const V2_PERMISSIONS: &str = "cruds";
    let permissions = match permissions {
        "read" => "rs",
        "write" => "cud",
        "*" => V2_PERMISSIONS,
        other => other,
    };
    let mut remaining = V2_PERMISSIONS.as_bytes();
    for &p in permissions.as_bytes() {
        let Some(at) = remaining.iter().position(|&c| c == p) else {
            return false;
        };
        remaining = &remaining[at + 1..];
    }
    permissions.as_bytes().contains(&permission)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_scope_covers_search_and_read() {
        let g = "patient/ImagingStudy.read";
        assert_eq!(
            check_smart_scopes(g, "patient", "GET", "ImagingStudy"),
            Ok(())
        );
        assert_eq!(
            check_smart_scopes(g, "patient", "GET", "/ImagingStudy/1"),
            Ok(())
        );
        assert_eq!(
            check_smart_scopes(g, "patient", "DELETE", "ImagingStudy/1"),
            Err(Error::SmartScopeDenied)
        );
        assert_eq!(
            check_smart_scopes(g, "patient", "HEAD", "ImagingStudy"),
            Err(Error::SmartTarget)
        );
    }
}
