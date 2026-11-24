#[path = "support/build.rs"]
mod build;
use build::*;
use dicom_rust::dicomjson;
use dicom_rust::dicomxml;
use dicom_rust::object::{Object, Tag, writer};
fn encoded(ds: &Object) -> Vec<u8> {
    writer::write_data_set(ds, EXPLICIT_LE).expect("encode")
}
fn to_json(ds: &Object) -> Vec<u8> {
    dicomjson::marshal_compact(ds).expect("JSON")
}
fn from_json(data: &[u8]) -> Object {
    dicomjson::unmarshal(data, &dicomjson::UnmarshalOptions::default())
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(data)))
}
fn to_xml(ds: &Object) -> Vec<u8> {
    dicomxml::marshal(Some(ds), &mut dicomxml::Options::default()).expect("XML")
}
fn from_xml(data: &[u8]) -> Object {
    dicomxml::unmarshal(data, &mut dicomxml::UnmarshalOptions::default())
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(data)))
}
#[test]
fn json_round_trips() {
    let ds = rich_data_set();
    let json = to_json(&ds);
    let text = String::from_utf8(json.clone()).expect("UTF-8 JSON");
    assert!(text.starts_with('{'), "{text}");
    assert!(
        text.contains(r#""00100010":{"vr":"PN","Value":[{"Alphabetic":"Muller^Jose"}]}"#),
        "{text}"
    );
    assert!(
        text.contains(r#""00280010":{"vr":"US","Value":[4]}"#),
        "{text}"
    );
    assert!(text.contains(r#""InlineBinary""#), "{text}");
    let back = from_json(&json);
    assert_eq!(encoded(&back), encoded(&ds));
    let pretty = dicomjson::marshal_pretty(&ds).expect("pretty");
    assert!(pretty.len() > json.len());
    assert_eq!(encoded(&from_json(&pretty)), encoded(&ds));
}
#[test]
fn xml_round_trips() {
    let ds = rich_data_set();
    let xml = to_xml(&ds);
    let text = String::from_utf8(xml.clone()).expect("UTF-8 XML");
    assert!(text.contains("<NativeDicomModel"), "{text}");
    assert!(
        text.contains(r#"tag="00100010""#) && text.contains(r#"vr="PN""#),
        "{text}"
    );
    assert!(text.contains("<FamilyName>Muller</FamilyName>"), "{text}");
    assert!(text.contains("<Item number=\"1\">"), "{text}");
    let back = from_xml(&xml);
    assert_eq!(encoded(&back), encoded(&ds));
}
#[test]
fn json_to_xml_to_json() {
    let ds = rich_data_set();
    let via_xml = from_xml(&to_xml(&from_json(&to_json(&ds))));
    assert_eq!(to_json(&via_xml), to_json(&ds));
}
#[test]
fn images_keep_their_pixels() {
    for (spp, allocated, bits) in [(1, 8, 8), (1, 16, 12), (3, 8, 8)] {
        let ds = image(5, 3, spp, allocated, bits);
        let want = ds.get_raw(Tag::PIXEL_DATA).unwrap();
        assert_eq!(
            from_json(&to_json(&ds)).get_raw(Tag::PIXEL_DATA).unwrap(),
            want
        );
        assert_eq!(
            from_xml(&to_xml(&ds)).get_raw(Tag::PIXEL_DATA).unwrap(),
            want
        );
    }
}
#[test]
fn damaged_documents_are_errors() {
    let json = to_json(&rich_data_set());
    let opts = dicomjson::UnmarshalOptions::default();
    for cut in [1, json.len() / 3, json.len() - 1] {
        assert!(
            dicomjson::unmarshal(&json[..cut], &opts).is_err(),
            "JSON cut at {cut}"
        );
    }
    for bad in [
        &br#"[]"#[..],
        br#"{"0010001":{"vr":"PN"}}"#,
        br#"{"00100010":{"vr":"XX","Value":["a"]}}"#,
        br#"{"00280010":{"vr":"US","Value":["four"]}}"#,
    ] {
        assert!(
            dicomjson::unmarshal(bad, &opts).is_err(),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
    let xml = to_xml(&rich_data_set());
    for cut in [10, xml.len() / 2, xml.len() - 5] {
        assert!(
            dicomxml::unmarshal(&xml[..cut], &mut dicomxml::UnmarshalOptions::default()).is_err(),
            "XML cut at {cut}"
        );
    }
    assert!(
        dicomxml::unmarshal(b"<NotDicom/>", &mut dicomxml::UnmarshalOptions::default()).is_err()
    );
}
