use super::MeshError;
use crate::textfmt;
pub const MAX_VERTEX_SCALAR_ATTRIBUTES: usize = 32;
pub const MAX_GLB_SCALAR_ATTRIBUTES: usize = 4096;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VertexScalarAttribute {
    pub name: String,
    pub values: Vec<f64>,
    pub units: String,
}
pub(crate) fn validate_vertex_scalars(
    attributes: &[VertexScalarAttribute],
    vertices: usize,
    float32_output: bool,
) -> Result<(), MeshError> {
    if attributes.len() > MAX_VERTEX_SCALAR_ATTRIBUTES {
        return Err(MeshError::invalid(format!(
            "{} vertex scalar attributes exceed {MAX_VERTEX_SCALAR_ATTRIBUTES}",
            attributes.len()
        )));
    }
    if !attributes.is_empty() && vertices as u64 >= u64::from(u32::MAX) {
        return Err(MeshError::invalid("vertex scalar index capacity"));
    }
    for (i, attribute) in attributes.iter().enumerate() {
        let quoted = || textfmt::quote(&attribute.name);
        if !scalar_name(&attribute.name) {
            return Err(MeshError::invalid(format!(
                "invalid vertex scalar name {}",
                quoted()
            )));
        }
        if matches!(
            attribute.name.to_ascii_lowercase().as_str(),
            "x" | "y" | "z" | "nx" | "ny" | "nz"
        ) {
            return Err(MeshError::invalid(format!(
                "reserved vertex scalar name {}",
                quoted()
            )));
        }
        if attributes[..i].iter().any(|p| p.name == attribute.name) {
            return Err(MeshError::invalid(format!(
                "duplicate vertex scalar name {}",
                quoted()
            )));
        }
        if !scalar_units(&attribute.units) {
            return Err(MeshError::invalid(format!(
                "invalid units for vertex scalar {}",
                quoted()
            )));
        }
        if attribute.values.len() != vertices {
            return Err(MeshError::invalid(format!(
                "{} values for vertex scalar {} of {vertices} vertices",
                attribute.values.len(),
                quoted()
            )));
        }
        for (vertex, value) in attribute.values.iter().enumerate() {
            if !value.is_finite() || (float32_output && !(*value as f32).is_finite()) {
                return Err(MeshError::invalid(format!(
                    "vertex scalar {} value {vertex} is not finite/representable",
                    quoted()
                )));
            }
        }
    }
    Ok(())
}
pub(crate) fn scalar_name(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > 64 || !b[0].is_ascii_alphabetic() {
        return false;
    }
    b[1..]
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}
pub(crate) fn scalar_units(units: &str) -> bool {
    units.len() <= 64
        && units
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"^*/._-".contains(&c))
}
