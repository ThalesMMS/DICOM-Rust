use dicom_rust::net::ul;
use dicom_rust::object::parser::source::{ByteRead, read_full};
use dicom_rust::object::{self, Error, ReadFileOptions};
struct OverReporting {
    data: Vec<u8>,
    pos: usize,
    extra: usize,
}
impl OverReporting {
    fn new(data: &[u8], extra: usize) -> OverReporting {
        OverReporting {
            data: data.to_vec(),
            pos: 0,
            extra,
        }
    }
    fn copy(&mut self, p: &mut [u8]) -> usize {
        let rest = self.data.get(self.pos..).unwrap_or_default();
        let n = p.len().min(rest.len());
        p[..n].copy_from_slice(&rest[..n]);
        self.pos += n;
        n
    }
}
impl ByteRead for OverReporting {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<Error>) {
        if self.pos >= self.data.len() {
            return (0, Some(Error::eof()));
        }
        let n = self.copy(p);
        (n + self.extra, None)
    }
}
impl ul::ByteRead for OverReporting {
    fn read_avail(&mut self, buf: &mut [u8]) -> ul::Result<usize> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let n = self.copy(buf);
        Ok(n + self.extra)
    }
}
fn element(group: u16, elem: u16, vr: &[u8; 2], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&elem.to_le_bytes());
    out.extend_from_slice(vr);
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value);
    out
}
#[test]
fn read_full_clamps_a_reader_that_reports_too_many_bytes() {
    for extra in [1, 3, 1 << 20] {
        let mut r = OverReporting::new(&[7; 10], extra);
        let mut buf = [0u8; 4];
        let (n, err) = read_full(&mut r, &mut buf);
        assert_eq!(n, 4);
        assert!(err.is_none());
        assert_eq!(buf, [7; 4]);
    }
}
#[test]
fn data_set_reader_survives_a_source_that_reports_too_many_bytes() {
    let mut body = element(0x0008, 0x0060, b"CS", b"CT");
    body.extend(element(0x0010, 0x0010, b"PN", b"Doe^Jane"));
    body.extend(element(0x0028, 0x0010, b"US", &4u16.to_le_bytes()));
    for extra in [1, 2, 7, 4096, 1 << 30] {
        let source = Box::new(OverReporting::new(&body, extra));
        let _ = object::read_data_set_with_options(
            source,
            object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
            ReadFileOptions::default(),
        );
    }
}
#[test]
fn pdu_reader_survives_a_stream_that_reports_too_many_bytes() {
    let stream = [5u8, 0, 0, 0, 0, 4, 0, 0, 0, 0, 7, 0, 0, 0, 0, 4, 0, 0, 2, 0];
    for extra in [1, 5, 64 << 10] {
        let mut r = OverReporting::new(&stream, extra);
        for _ in 0..4 {
            if ul::read_pdu(&mut r, ul::DEFAULT_MAX_PDU).is_err() {
                break;
            }
        }
    }
}
#[test]
fn repeated_xml_encoding_declarations_are_bounded() {
    let mut xml = Vec::new();
    for _ in 0..40 {
        xml.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"latin1\"?>");
    }
    xml.extend_from_slice(&[0xD2; 64]);
    xml.extend_from_slice(b"<NativeDicomModel/>");
    let mut options = dicom_rust::dicomxml::UnmarshalOptions::default();
    options.limits.max_xml_bytes = 1 << 20;
    let err = dicom_rust::dicomxml::unmarshal(&xml, &mut options).unwrap_err();
    assert!(
        err.to_string().contains("maximum XML bytes exceeded"),
        "{err}"
    );
}
