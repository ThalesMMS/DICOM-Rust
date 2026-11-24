use dicom_rust::object::{self, Element, Object, Tag, Value};
use std::error::Error;
fn default_file() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/examples/data/ct_small.dcm").to_string()
}
fn show(ds: &Object, e: &Element) -> String {
    match &e.value {
        Value::Sequence(items) => format!("<{} item(s)>", items.len()),
        Value::Fragments { fragments, .. } => format!("<{} fragment(s)>", fragments.len()),
        Value::Raw(bytes) if e.header.tag == Tag::PIXEL_DATA => format!("<{} bytes>", bytes.len()),
        _ => match ds.decode_text_values(e) {
            Ok(values) if !values.is_empty() => values.join("\\"),
            _ => match ds.get_ints(e.header.tag) {
                Ok(n) => n.iter().map(i64::to_string).collect::<Vec<_>>().join("\\"),
                Err(_) => match ds.get_floats(e.header.tag) {
                    Ok(f) => f.iter().map(f64::to_string).collect::<Vec<_>>().join("\\"),
                    Err(_) => e
                        .raw()
                        .map_or(String::new(), |b| format!("<{} bytes>", b.len())),
                },
            },
        },
    }
}
fn print(ds: &Object) {
    for e in ds.sorted_elements() {
        let name = object::standard_names(e.header.tag).map_or("", |(keyword, _)| keyword);
        let value = show(ds, &e);
        let value: String = value.chars().take(64).collect();
        println!("{} {} {:<36} {}", e.header.tag, e.header.vr, name, value);
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(default_file);
    let file = object::open_file(&path).map_err(|(_, e)| e)?;
    println!("transfer syntax: {}", file.transfer_syntax.uid);
    println!("-- file meta information");
    print(&file.meta);
    println!("-- data set");
    print(&file.dataset);
    let name = file
        .get_string(Tag::new(0x0010, 0x0010))
        .unwrap_or_default();
    let rows = file.get_int(Tag::new(0x0028, 0x0010))?;
    let columns = file.get_int(Tag::new(0x0028, 0x0011))?;
    println!("patient {name}, image {columns} x {rows}");
    Ok(())
}
