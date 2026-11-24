#[allow(unused_imports)]
use super::support::*;
use dicom_rust::numtext::format_float;
use dicom_rust::object::{self, transfer};
use dicom_rust::sr::CodedEntry;
use dicom_rust::sr::image_library::ImageLibraryOptions;
use dicom_rust::sr::image_library_extract::extract_image_library_entry;
use dicom_rust::sr::numeric_representation::NumericRepresentationMetadata;
use dicom_rust::sr::template_metadata::{CodeContext, TemplateValidationMetadata};
use dicom_rust::textfmt::quote;
fn code(c: &CodedEntry) -> String {
    format!(
        "({},{},{})",
        quote(&c.code_value),
        quote(&c.coding_scheme_designator),
        quote(&c.code_meaning)
    )
}
fn code_context(c: &CodeContext) -> String {
    let fields = [
        &c.coding_scheme_version,
        &c.context_identifier,
        &c.mapping_resource,
        &c.context_group_version,
        &c.context_group_local_version,
        &c.extension_flag,
        &c.extension_creator_uid,
        &c.context_uid,
        &c.mapping_resource_uid,
        &c.mapping_resource_name,
        &c.long_code_value,
        &c.urn_code_value,
    ];
    let parts: Vec<String> = fields.iter().map(|f| quote(f)).collect();
    format!("[{}]", parts.join(" "))
}
fn metadata(b: &mut String, m: &TemplateValidationMetadata, n: &NumericRepresentationMetadata) {
    for c in &m.code_contexts {
        b.push_str(&format!(
            "  context {} value={} eq={}\n",
            c.path,
            code_context(&c.value),
            c.value_equivalent_codes.len()
        ));
        for e in &c.value_equivalent_codes {
            b.push_str(&format!(
                "    eq {} {}\n",
                code(&e.code),
                code_context(&e.context)
            ));
        }
    }
    for id in &m.template_identifications {
        b.push_str(&format!("  template {} {}\n", id.path, id.identifier));
    }
    for e in &n.entries {
        let r = &e.representation;
        b.push_str(&format!(
            "  numeric {} ds={}/{} fd={}/{}\n",
            e.path,
            quote(&r.decimal_string),
            r.has_decimal_string,
            format_float(r.floating_point, 'g', -1),
            r.has_floating_point
        ));
    }
}
pub fn extract(
    data: Vec<i64>,
    frames: String,
    max_items: i64,
    max_depth: i64,
    max_value_bytes: i64,
    image_path: String,
) -> Result<String, String> {
    let data: Vec<u8> = data.into_iter().map(|b| b as u8).collect();
    let Some(syntax) = transfer::lookup("1.2.840.10008.1.2.1") else {
        return Ok("harness: syntax".to_string());
    };
    let obj = match object::read_data_set_bytes(&data, syntax) {
        Ok(obj) => obj,
        Err(err) => return Ok(format!("read error {err}")),
    };
    let mut fs = Vec::new();
    if !frames.is_empty() {
        for f in frames.split(',') {
            match f.parse::<i64>() {
                Ok(n) => fs.push(n),
                Err(_) => return Ok("harness: frames".to_string()),
            }
        }
    }
    let options = ImageLibraryOptions {
        max_items,
        max_depth,
        max_value_bytes,
    };
    let out = match extract_image_library_entry(&obj, &fs, options) {
        Ok(out) => out,
        Err(err) => return Ok(format!("error {err}")),
    };
    let img = &out.entry.image;
    let frames: Vec<String> = img.frames.iter().map(|f| f.to_string()).collect();
    let mut b = format!(
        "image {} {} {} {} [{}]\n",
        img.sop_class_uid,
        img.sop_instance_uid,
        img.study_instance_uid,
        img.series_instance_uid,
        frames.join(" ")
    );
    for d in &out.entry.descriptors {
        b.push_str(&format!(
            "descriptor {} {} {} text={} code={} date={} time={} dt={} uid={}",
            d.value_type,
            d.relationship_type,
            code(&d.concept_name),
            quote(&d.text),
            code(&d.code),
            quote(&d.date),
            quote(&d.time),
            quote(&d.date_time),
            quote(&d.uid)
        ));
        if let Some(m) = &d.measurement {
            b.push_str(&format!(
                " num={} {}",
                format_float(m.value, 'g', -1),
                code(&m.units)
            ));
        }
        b.push('\n');
    }
    metadata(&mut b, &out.metadata, &out.numeric);
    for f in &out.findings {
        b.push_str(&format!("finding {} {} {}\n", f.template, f.row, f.reason));
    }
    match out.metadata_at(&image_path) {
        Err(err) => b.push_str(&format!("at error {err}\n")),
        Ok((m, n)) => {
            b.push_str("at\n");
            metadata(&mut b, &m, &n);
        }
    }
    Ok(b)
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Extract" => {
            let out = extract(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
