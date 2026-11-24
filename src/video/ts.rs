use super::{CODEC_HEVC, Error, Kind, malformed, unsupported_stream};
const TS_PACKET_SIZE: usize = 188;
fn ts_stream_type(codec: &str) -> u8 {
    if codec == CODEC_HEVC { 0x24 } else { 0x1b }
}
fn pid13(hi: u8, lo: u8) -> i32 {
    i32::from(hi & 0x1f) << 8 | i32::from(lo)
}
fn len12(hi: u8, lo: u8) -> usize {
    usize::from(hi & 0x0f) << 8 | usize::from(lo)
}
pub(crate) fn demux_transport_stream(data: &[u8], codec: &str) -> Result<Vec<u8>, Error> {
    if data.len() < TS_PACKET_SIZE || !data.len().is_multiple_of(TS_PACKET_SIZE) {
        return Err(malformed(format!(
            "transport stream length {} is not a multiple of {TS_PACKET_SIZE}",
            data.len()
        )));
    }
    let (mut pmt_pid, mut video_pid) = (-1i32, -1i32);
    let mut es: Vec<u8> = Vec::new();
    let mut in_pes = false;
    for (index, p) in data.as_chunks::<TS_PACKET_SIZE>().0.iter().enumerate() {
        if p[0] != 0x47 {
            return Err(malformed(format!(
                "transport packet {index} has no sync byte"
            )));
        }
        if p[1] & 0x80 != 0 {
            continue;
        }
        let start = p[1] & 0x40 != 0;
        let pid = pid13(p[1], p[2]);
        let control = (p[3] >> 4) & 3;
        if control & 1 == 0 {
            continue;
        }
        let mut pos = 4;
        if control & 2 != 0 {
            pos += 1 + usize::from(p[4]);
        }
        if pos > TS_PACKET_SIZE {
            return Err(malformed(format!(
                "transport packet {index} adaptation field length"
            )));
        }
        let mut payload = &p[pos..];
        if pid == 0 && start && pmt_pid < 0 {
            let section = psi_section(payload, 0x00)?;
            for entry in section.as_chunks::<4>().0 {
                if entry[0] != 0 || entry[1] != 0 {
                    pmt_pid = pid13(entry[2], entry[3]);
                    break;
                }
            }
        } else if pid == pmt_pid && start && video_pid < 0 {
            let section = psi_section(payload, 0x02)?;
            if section.len() < 4 {
                return Err(malformed("PMT section too short"));
            }
            let mut i = 4 + len12(section[2], section[3]);
            while i + 5 <= section.len() {
                if section[i] == ts_stream_type(codec) {
                    video_pid = pid13(section[i + 1], section[i + 2]);
                    break;
                }
                i += 5 + len12(section[i + 3], section[i + 4]);
            }
            if video_pid < 0 {
                return Err(Error::new(
                    Kind::UnsupportedContainer,
                    format!("transport stream has no {codec} elementary stream"),
                ));
            }
        } else if pid == video_pid && video_pid >= 0 {
            if start {
                if payload.len() < 9 || payload[..3] != [0, 0, 1] {
                    return Err(malformed("PES packet start code"));
                }
                let header = 9 + usize::from(payload[8]);
                if header > payload.len() {
                    return Err(malformed("PES header length"));
                }
                payload = &payload[header..];
                in_pes = true;
            }
            if in_pes {
                es.extend_from_slice(payload);
            }
        }
    }
    if pmt_pid < 0 {
        return Err(malformed(
            "transport stream has no program association table",
        ));
    }
    if video_pid < 0 {
        return Err(malformed("transport stream has no program map table"));
    }
    if es.is_empty() {
        return Err(malformed(format!(
            "transport stream carries no {codec} data"
        )));
    }
    Ok(es)
}
fn psi_section(payload: &[u8], table_id: u8) -> Result<&[u8], Error> {
    if payload.is_empty() || 1 + usize::from(payload[0]) >= payload.len() {
        return Err(malformed("PSI pointer field"));
    }
    let s = &payload[1 + usize::from(payload[0])..];
    if s.len() < 3 || s[0] != table_id {
        return Err(malformed(format!("PSI table_id {table_id:#x}")));
    }
    let length = len12(s[1], s[2]);
    if length < 9 || 3 + length > s.len() {
        return Err(unsupported_stream("PSI section spanning transport packets"));
    }
    Ok(&s[8..3 + length - 4])
}
