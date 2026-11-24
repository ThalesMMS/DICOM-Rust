use sha2::{Digest, Sha256};
pub const UUID_ROOT: &str = "2.25";
fn set_uuid_version(mut uuid: [u8; 16], version: u8) -> [u8; 16] {
    uuid[6] = uuid[6] & 0x0f | version << 4;
    uuid[8] = uuid[8] & 0x3f | 0x80;
    uuid
}
pub fn uid_from_uuid(uuid: [u8; 16]) -> String {
    format!("{UUID_ROOT}.{}", u128::from_be_bytes(uuid))
}
fn name_message(parts: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(&(part.len() as u64).to_be_bytes());
        out.extend_from_slice(part.as_bytes());
    }
    out
}
fn first16(digest: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}
pub fn name_uid(parts: &[&str]) -> String {
    let digest = Sha256::digest(name_message(parts));
    uid_from_uuid(set_uuid_version(first16(&digest), 8))
}
pub(crate) fn name_uid_bytes(parts: &[&[u8]]) -> String {
    let mut message = Vec::new();
    for part in parts {
        message.extend_from_slice(&(part.len() as u64).to_be_bytes());
        message.extend_from_slice(part);
    }
    let digest = Sha256::digest(message);
    uid_from_uuid(set_uuid_version(first16(&digest), 8))
}
pub(crate) fn keyed_name_uid(key: &[u8], parts: &[&str]) -> String {
    let digest = hmac_sha256(key, &name_message(parts));
    uid_from_uuid(set_uuid_version(first16(&digest), 8))
}
pub fn new_uid() -> String {
    uid_from_uuid(set_uuid_version(*uuid::Uuid::new_v4().as_bytes(), 4))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hmac_rfc4231_case2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            mac.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }
    #[test]
    fn random_uids_are_version_4() {
        let uid = new_uid();
        let n: u128 = uid[5..].parse().unwrap();
        assert_eq!((n >> 76) & 0xf, 4);
    }
}
