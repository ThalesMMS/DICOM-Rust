use super::transport::authoritative_snapshot;
use super::{Error, Result, blank, json_string, trim_unicode_space, write_text};
use crate::render::volume_store::VolumeLease;
use crate::textfmt::quote;
use std::io::Write;
pub const TRACE_V1_SCHEMA: &str = "trace-v1";
pub const RESOURCE_SAMPLE_V1_SCHEMA: &str = "resource-sample-v1";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Generation {
    pub volume: u64,
    pub view: u64,
    pub presentation: u64,
}
impl Generation {
    pub(crate) fn write_json(&self, out: &mut String) {
        out.push_str(&format!(
            "{{\"volume\":{},\"view\":{},\"presentation\":{}}}",
            self.volume, self.view, self.presentation
        ));
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct U64Measurement {
    pub status: String,
    pub value: Option<u64>,
    pub reason: String,
}
impl U64Measurement {
    pub fn available(value: u64) -> U64Measurement {
        U64Measurement {
            status: "available".to_string(),
            value: Some(value),
            reason: String::new(),
        }
    }
    pub fn unavailable(reason: &str) -> U64Measurement {
        U64Measurement {
            status: "unavailable".to_string(),
            value: None,
            reason: trim_unicode_space(reason).to_string(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        match self.status.as_str() {
            "available" => {
                if self.value.is_none() || !self.reason.is_empty() {
                    return Err(Error::evidence(
                        "available metric must contain only a value",
                    ));
                }
            }
            "unavailable" => {
                if self.value.is_some() || blank(&self.reason) {
                    return Err(Error::evidence(
                        "unavailable metric requires a reason and no value",
                    ));
                }
            }
            other => {
                return Err(Error::evidence(format!("metric status {}", quote(other))));
            }
        }
        Ok(())
    }
    fn write_json(&self, out: &mut String) {
        out.push_str("{\"status\":");
        json_string(out, &self.status);
        if let Some(value) = self.value {
            out.push_str(&format!(",\"value\":{value}"));
        }
        if !self.reason.is_empty() {
            out.push_str(",\"reason\":");
            json_string(out, &self.reason);
        }
        out.push('}');
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TraceV1 {
    pub schema: String,
    pub run_id: String,
    pub backend: String,
    pub sequence: String,
    pub step_id: String,
    pub operation: String,
    pub phase: String,
    pub outcome: String,
    pub generation: Generation,
    pub sequence_no: u64,
    pub timestamp_ns: i64,
    pub duration_ns: U64Measurement,
    pub frame_sha256: String,
}
impl TraceV1 {
    pub fn validate(&self) -> Result<()> {
        if self.schema != TRACE_V1_SCHEMA {
            return Err(Error::evidence(format!(
                "trace schema {}",
                quote(&self.schema)
            )));
        }
        for (field, value) in [
            ("run_id", &self.run_id),
            ("backend", &self.backend),
            ("sequence", &self.sequence),
            ("step_id", &self.step_id),
            ("operation", &self.operation),
            ("phase", &self.phase),
            ("outcome", &self.outcome),
        ] {
            if blank(value) {
                return Err(Error::evidence(format!("empty trace {field}")));
            }
        }
        if self.generation.volume == 0 {
            return Err(Error::evidence("zero trace volume generation"));
        }
        if self.sequence_no == 0 {
            return Err(Error::evidence("zero trace sequence number"));
        }
        self.validate_generation()?;
        self.duration_ns.validate()
    }
    fn validate_generation(&self) -> Result<()> {
        let g = &self.generation;
        match self.operation.as_str() {
            "2d" => {
                if g.view != 0 || g.presentation != 0 {
                    return Err(Error::evidence(format!(
                        "2D phase {} must not claim view/presentation generations",
                        quote(&self.phase)
                    )));
                }
            }
            "mpr" | "vr" => {
                if self.phase == "render" || self.phase == "publish" {
                    if g.view == 0 || g.presentation == 0 {
                        return Err(Error::evidence(format!(
                            "{} {} requires complete view/presentation generations",
                            self.operation, self.phase
                        )));
                    }
                } else if (g.view == 0) != (g.presentation == 0) {
                    return Err(Error::evidence(format!(
                        "{} phase {} has partial view/presentation generation",
                        self.operation,
                        quote(&self.phase)
                    )));
                }
            }
            other => {
                return Err(Error::evidence(format!("trace operation {}", quote(other))));
            }
        }
        Ok(())
    }
    fn write_json(&self, out: &mut String) {
        let field = |out: &mut String, name: &str, value: &str| {
            out.push_str(name);
            json_string(out, value);
        };
        field(out, "{\"schema\":", &self.schema);
        field(out, ",\"run_id\":", &self.run_id);
        field(out, ",\"backend\":", &self.backend);
        field(out, ",\"sequence\":", &self.sequence);
        field(out, ",\"step_id\":", &self.step_id);
        field(out, ",\"operation\":", &self.operation);
        field(out, ",\"phase\":", &self.phase);
        field(out, ",\"outcome\":", &self.outcome);
        out.push_str(",\"generation\":");
        self.generation.write_json(out);
        out.push_str(&format!(
            ",\"sequence_no\":{},\"timestamp_ns\":{},\"duration_ns\":",
            self.sequence_no, self.timestamp_ns
        ));
        self.duration_ns.write_json(out);
        if !self.frame_sha256.is_empty() {
            field(out, ",\"frame_sha256\":", &self.frame_sha256);
        }
        out.push_str("}\n");
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceSampleV1 {
    pub schema: String,
    pub run_id: String,
    pub backend: String,
    pub sequence: String,
    pub step_id: String,
    pub generation: Generation,
    pub sequence_no: u64,
    pub timestamp_ns: i64,
    pub private_bytes: U64Measurement,
    pub rss_bytes: U64Measurement,
    pub heap_bytes: U64Measurement,
    pub gpu_owned_bytes: U64Measurement,
    pub copied_bytes: U64Measurement,
    pub package_bytes: U64Measurement,
    pub handles: U64Measurement,
    pub threads: U64Measurement,
    pub child_processes: U64Measurement,
    pub tasks: U64Measurement,
    pub mappings: U64Measurement,
    pub volume_uploads: U64Measurement,
    pub pipeline_builds: U64Measurement,
    pub renders: U64Measurement,
    pub readbacks: U64Measurement,
}
impl ResourceSampleV1 {
    pub fn metrics(&self) -> [(&'static str, &U64Measurement); 15] {
        [
            ("private_bytes", &self.private_bytes),
            ("rss_bytes", &self.rss_bytes),
            ("heap_bytes", &self.heap_bytes),
            ("gpu_owned_bytes", &self.gpu_owned_bytes),
            ("copied_bytes", &self.copied_bytes),
            ("package_bytes", &self.package_bytes),
            ("handles", &self.handles),
            ("threads", &self.threads),
            ("child_processes", &self.child_processes),
            ("tasks", &self.tasks),
            ("mappings", &self.mappings),
            ("volume_uploads", &self.volume_uploads),
            ("pipeline_builds", &self.pipeline_builds),
            ("renders", &self.renders),
            ("readbacks", &self.readbacks),
        ]
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema != RESOURCE_SAMPLE_V1_SCHEMA {
            return Err(Error::evidence(format!(
                "resource schema {}",
                quote(&self.schema)
            )));
        }
        for (field, value) in [
            ("run_id", &self.run_id),
            ("backend", &self.backend),
            ("sequence", &self.sequence),
            ("step_id", &self.step_id),
        ] {
            if blank(value) {
                return Err(Error::evidence(format!("empty resource {field}")));
            }
        }
        if self.generation.volume == 0 {
            return Err(Error::evidence("zero resource volume generation"));
        }
        if self.sequence_no == 0 {
            return Err(Error::evidence("zero resource sequence number"));
        }
        for (index, (_, metric)) in self.metrics().iter().enumerate() {
            metric
                .validate()
                .map_err(|e| e.prefixed(format!("resource metric {index}")))?;
        }
        Ok(())
    }
    fn write_json(&self, out: &mut String) {
        let field = |out: &mut String, name: &str, value: &str| {
            out.push_str(name);
            json_string(out, value);
        };
        field(out, "{\"schema\":", &self.schema);
        field(out, ",\"run_id\":", &self.run_id);
        field(out, ",\"backend\":", &self.backend);
        field(out, ",\"sequence\":", &self.sequence);
        field(out, ",\"step_id\":", &self.step_id);
        out.push_str(",\"generation\":");
        self.generation.write_json(out);
        out.push_str(&format!(
            ",\"sequence_no\":{},\"timestamp_ns\":{}",
            self.sequence_no, self.timestamp_ns
        ));
        for (name, metric) in self.metrics() {
            out.push_str(&format!(",\"{name}\":"));
            metric.write_json(out);
        }
        out.push_str("}\n");
    }
}
pub(crate) fn trace_jsonl(records: &[TraceV1]) -> Result<String> {
    if records.is_empty() {
        return Err(Error::evidence("empty trace stream"));
    }
    let mut buffer = String::new();
    for (index, record) in records.iter().enumerate() {
        record
            .validate()
            .map_err(|e| e.prefixed(format!("trace record {index}")))?;
        record.write_json(&mut buffer);
    }
    Ok(buffer)
}
pub fn write_trace_jsonl(dst: &mut dyn Write, records: &[TraceV1]) -> Result<()> {
    let text = trace_jsonl(records)?;
    write_text(dst, &text)
}
pub(crate) fn resource_sample_jsonl(samples: &[ResourceSampleV1]) -> Result<String> {
    if samples.is_empty() {
        return Err(Error::evidence("empty resource stream"));
    }
    let mut buffer = String::new();
    for (index, sample) in samples.iter().enumerate() {
        sample
            .validate()
            .map_err(|e| e.prefixed(format!("resource sample {index}")))?;
        sample.write_json(&mut buffer);
    }
    Ok(buffer)
}
pub fn write_resource_sample_jsonl(
    dst: &mut dyn Write,
    samples: &[ResourceSampleV1],
) -> Result<()> {
    let text = resource_sample_jsonl(samples)?;
    write_text(dst, &text)
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EvidenceBundleV1 {
    pub traces: Vec<TraceV1>,
    pub resources: Vec<ResourceSampleV1>,
}
impl EvidenceBundleV1 {
    pub fn validate(&self) -> Result<()> {
        if self.traces.is_empty() || self.resources.is_empty() {
            return Err(Error::evidence(
                "evidence bundle requires trace and resource records",
            ));
        }
        if self.traces.len() != self.resources.len() {
            return Err(Error::evidence(format!(
                "trace/resource count mismatch {}/{}",
                self.traces.len(),
                self.resources.len()
            )));
        }
        let mut previous = 0u64;
        for (index, (trace, resource)) in self.traces.iter().zip(&self.resources).enumerate() {
            trace
                .validate()
                .map_err(|e| e.prefixed(format!("bundle trace {index}")))?;
            resource
                .validate()
                .map_err(|e| e.prefixed(format!("bundle resource {index}")))?;
            if trace.sequence_no <= previous {
                return Err(Error::evidence(format!(
                    "sequence_no {} is duplicate or out of order after {previous}",
                    trace.sequence_no
                )));
            }
            if resource.sequence_no != trace.sequence_no {
                return Err(Error::evidence(format!(
                    "trace/resource sequence_no mismatch {}/{}",
                    trace.sequence_no, resource.sequence_no
                )));
            }
            if trace.run_id != resource.run_id
                || trace.backend != resource.backend
                || trace.sequence != resource.sequence
                || trace.step_id != resource.step_id
                || trace.generation != resource.generation
            {
                return Err(Error::evidence(format!(
                    "trace/resource identity mismatch at sequence_no {}",
                    trace.sequence_no
                )));
            }
            previous = trace.sequence_no;
        }
        Ok(())
    }
    pub fn jsonl(&self) -> Result<(String, String)> {
        self.validate()?;
        Ok((
            trace_jsonl(&self.traces)?,
            resource_sample_jsonl(&self.resources)?,
        ))
    }
}
pub fn write_evidence_bundle_jsonl(
    trace_dst: &mut dyn Write,
    resource_dst: &mut dyn Write,
    bundle: &EvidenceBundleV1,
) -> Result<()> {
    let (traces, resources) = bundle.jsonl()?;
    write_text(trace_dst, &traces)?;
    write_text(resource_dst, &resources)
}
pub fn bind_trace_to_lease(mut record: TraceV1, lease: &VolumeLease) -> Result<TraceV1> {
    let (_, descriptor) = authoritative_snapshot(lease)?;
    if record.generation.volume != 0 && record.generation.volume != descriptor.volume_generation {
        return Err(Error::evidence(format!(
            "trace volume generation {} differs from authoritative lease {}",
            record.generation.volume, descriptor.volume_generation
        )));
    }
    record.generation.volume = descriptor.volume_generation;
    record.validate()?;
    Ok(record)
}
pub fn bind_resource_sample_to_lease(
    mut sample: ResourceSampleV1,
    lease: &VolumeLease,
) -> Result<ResourceSampleV1> {
    let (_, descriptor) = authoritative_snapshot(lease)?;
    if sample.generation.volume != 0 && sample.generation.volume != descriptor.volume_generation {
        return Err(Error::evidence(format!(
            "resource volume generation {} differs from authoritative lease {}",
            sample.generation.volume, descriptor.volume_generation
        )));
    }
    sample.generation.volume = descriptor.volume_generation;
    sample.validate()?;
    Ok(sample)
}
