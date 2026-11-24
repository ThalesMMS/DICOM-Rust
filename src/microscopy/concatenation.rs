use super::{
    Error, InstanceRef, Level, MAX_MICROSCOPY_FRAMES, Pyramid, TAG_CONCATENATION_FRAME_OFFSET,
    TAG_PHOTOMETRIC_INTERPRETATION, TAG_SAMPLES_PER_PIXEL, TAG_SOP_CLASS_UID, ceil_div, tag,
};
use crate::derivedio;
use crate::object::{Object, Tag, is_valid_uid};
const TAG_CONCATENATION_UID: Tag = tag(0x0020, 0x9161);
const TAG_IN_CONCATENATION_NUMBER: Tag = tag(0x0020, 0x9162);
const TAG_IN_CONCATENATION_TOTAL_NUMBER: Tag = tag(0x0020, 0x9163);
const TAG_CONCATENATION_SOURCE_UID: Tag = tag(0x0020, 0x0242);
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SlideConcatenation {
    pub uid: String,
    pub number: i64,
    pub total: i64,
    pub offset: i64,
    image: ConcatenationImage,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ConcatenationImage {
    source_uid: String,
    class_uid: String,
    frame_of_reference: String,
    photometric: String,
    samples: i64,
    allocated: i64,
    stored: i64,
    representation: i64,
    planar: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SlideTileMember {
    pub source: InstanceRef,
    pub number: i64,
    pub offset: i64,
    pub frames: i64,
}
fn one(values: Option<Vec<i64>>) -> Option<i64> {
    match values.as_deref() {
        Some([value]) => Some(*value),
        _ => None,
    }
}
pub(super) fn read_slide_concatenation(root: &Object) -> Result<SlideConcatenation, Error> {
    let uid = derivedio::clean_uid(root, TAG_CONCATENATION_UID);
    let has = |t: Tag| root.get(t).is_some();
    let has_total = has(TAG_IN_CONCATENATION_TOTAL_NUMBER);
    let has_source = has(TAG_CONCATENATION_SOURCE_UID);
    if !has(TAG_CONCATENATION_UID)
        && !has_source
        && !has(TAG_IN_CONCATENATION_NUMBER)
        && !has_total
        && !has(TAG_CONCATENATION_FRAME_OFFSET)
    {
        return Ok(SlideConcatenation::default());
    }
    if !is_valid_uid(uid.as_bytes()) {
        return Err(Error::invalid("missing or invalid Concatenation UID"));
    }
    let number = one(derivedio::ints(Some(root), TAG_IN_CONCATENATION_NUMBER));
    let offset = one(derivedio::ints(Some(root), TAG_CONCATENATION_FRAME_OFFSET));
    let (number, offset) = match (number, offset) {
        (Some(number), Some(offset))
            if (1..=65535).contains(&number)
                && offset >= number - 1
                && offset < MAX_MICROSCOPY_FRAMES
                && (number != 1 || offset == 0) =>
        {
            (number, offset)
        }
        _ => {
            return Err(Error::invalid(
                "invalid concatenation part number or frame offset",
            ));
        }
    };
    let mut total = 0;
    if has_total {
        match one(derivedio::ints(
            Some(root),
            TAG_IN_CONCATENATION_TOTAL_NUMBER,
        )) {
            Some(value) if value >= number.max(2) && value <= 65535 => total = value,
            _ => return Err(Error::invalid("invalid In-concatenation Total Number")),
        }
    }
    let source_uid = derivedio::clean_uid(root, TAG_CONCATENATION_SOURCE_UID);
    if has_source && !is_valid_uid(source_uid.as_bytes()) {
        return Err(Error::invalid(
            "invalid SOP Instance UID of Concatenation Source",
        ));
    }
    Ok(SlideConcatenation {
        uid,
        number,
        total,
        offset,
        image: ConcatenationImage {
            source_uid,
            class_uid: derivedio::clean_uid(root, TAG_SOP_CLASS_UID),
            frame_of_reference: derivedio::clean_uid(root, tag(0x0020, 0x0052)),
            photometric: derivedio::clean_string(root, TAG_PHOTOMETRIC_INTERPRETATION),
            samples: derivedio::int(root, TAG_SAMPLES_PER_PIXEL),
            allocated: derivedio::int(root, tag(0x0028, 0x0100)),
            stored: derivedio::int(root, tag(0x0028, 0x0101)),
            representation: derivedio::int(root, tag(0x0028, 0x0103)),
            planar: derivedio::int(root, tag(0x0028, 0x0006)),
        },
    })
}
pub(super) fn implicit_tile_count(
    matrix_width: i64,
    matrix_height: i64,
    tile_width: i64,
    tile_height: i64,
    paths: i64,
    planes: i64,
) -> Result<i64, Error> {
    if matrix_width <= 0 || matrix_height <= 0 || tile_width <= 0 || tile_height <= 0 {
        return Err(Error::invalid("invalid implicit tile dimensions"));
    }
    let mut total = 1;
    for factor in [
        ceil_div(matrix_width, tile_width),
        ceil_div(matrix_height, tile_height),
        paths,
        planes,
    ] {
        if factor <= 0 || total > MAX_MICROSCOPY_FRAMES / factor {
            return Err(Error::invalid("implicit tiling exceeds frame limits"));
        }
        total *= factor;
    }
    Ok(total)
}
fn compatible_spans(left: &SlideTileMember, right: &SlideTileMember) -> bool {
    let missing = right.number - left.number - 1;
    let gap = right.offset - left.offset - left.frames;
    (missing == 0 && gap == 0) || (missing > 0 && gap >= missing)
}
pub(super) fn validate_slide_span(
    member: &SlideTileMember,
    total: i64,
    expected: i64,
) -> Result<(), Error> {
    let end = member.offset + member.frames;
    if (member.number == 1 && end == expected)
        || (total > 0
            && (member.number > total
                || expected - end < total - member.number
                || (member.number == total && end != expected)))
    {
        return Err(Error::invalid(
            "TILED_FULL span disagrees with concatenation count",
        ));
    }
    Ok(())
}
impl Level {
    pub(super) fn concatenation_uid(&self) -> &str {
        &self.concatenation.uid
    }
    pub(super) fn concatenation_number(&self) -> i64 {
        self.concatenation.number
    }
    pub(super) fn concatenation_total(&self) -> i64 {
        self.concatenation.total
    }
    fn concatenation_tile_count(&self) -> Result<i64, Error> {
        implicit_tile_count(
            self.matrix_width,
            self.matrix_height,
            self.tile_width,
            self.tile_height,
            self.optical_path_ids.len() as i64,
            self.focal_plane_offsets.len() as i64,
        )
    }
    pub(super) fn locate_tile_frame(&self, ordinal: i64) -> Option<(InstanceRef, i64)> {
        if self.members.is_empty() {
            let frame = ordinal - self.frame_offset + 1;
            return (frame > 0 && frame <= self.frame_count).then(|| (self.source.clone(), frame));
        }
        let index = self
            .members
            .partition_point(|member| ordinal >= member.offset + member.frames);
        let member = self.members.get(index)?;
        (ordinal >= member.offset).then(|| (member.source.clone(), ordinal - member.offset + 1))
    }
}
fn join_slide_member(
    mut current: Level,
    part: &Level,
    member: SlideTileMember,
    expected: i64,
) -> Result<Level, Error> {
    if current.source.study_instance_uid != part.source.study_instance_uid
        || current.source.series_instance_uid != part.source.series_instance_uid
        || current.concatenation.image != part.concatenation.image
        || current.matrix_width != part.matrix_width
        || current.matrix_height != part.matrix_height
        || current.tile_width != part.tile_width
        || current.tile_height != part.tile_height
        || current.pixel_spacing_x != part.pixel_spacing_x
        || current.pixel_spacing_y != part.pixel_spacing_y
        || current.origin != part.origin
        || current.orientation != part.orientation
        || current.imaged_volume_depth != part.imaged_volume_depth
        || current.optical_path_ids != part.optical_path_ids
        || current.focal_plane_offsets != part.focal_plane_offsets
    {
        return Err(Error::invalid(
            "inconsistent TILED_FULL concatenation geometry or identity",
        ));
    }
    let mut total = current.concatenation.total;
    if part.concatenation.total > 0 {
        if total > 0 && total != part.concatenation.total {
            return Err(Error::invalid("inconsistent In-concatenation Total Number"));
        }
        total = part.concatenation.total;
    }
    for previous in &current.members {
        if previous.source.sop_instance_uid == member.source.sop_instance_uid
            || previous.number == member.number
        {
            return Err(Error::invalid("duplicate TILED_FULL concatenation member"));
        }
        validate_slide_span(previous, total, expected)?;
    }
    validate_slide_span(&member, total, expected)?;
    let index = current
        .members
        .partition_point(|other| other.number < member.number);
    if index > 0 && !compatible_spans(&current.members[index - 1], &member)
        || index < current.members.len() && !compatible_spans(&member, &current.members[index])
    {
        return Err(Error::invalid(
            "frame offset disagrees with concatenation member order",
        ));
    }
    current.frame_count += member.frames;
    current.members.insert(index, member);
    current.source = current.members[0].source.clone();
    current.frame_offset = current.members[0].offset;
    current.concatenation.total = total;
    Ok(current)
}
impl Pyramid {
    pub(super) fn add_level(&mut self, mut level: Level) -> Result<(), Error> {
        if level.concatenation.uid.is_empty() {
            self.levels.push(level);
            return Ok(());
        }
        let mut matched = None;
        for (index, current) in self.levels.iter().enumerate() {
            if current.concatenation.uid != level.concatenation.uid {
                continue;
            }
            if current.implicit_tiling != level.implicit_tiling {
                return Err(Error::invalid(
                    "inconsistent tiling organization in concatenation",
                ));
            }
            matched = Some(index);
            break;
        }
        if !level.implicit_tiling {
            self.levels.push(level);
            return Ok(());
        }
        let expected = level.concatenation_tile_count()?;
        let member = SlideTileMember {
            source: level.source.clone(),
            number: level.concatenation.number,
            offset: level.frame_offset,
            frames: level.frame_count,
        };
        if member.source.sop_instance_uid.is_empty()
            || member.frames <= 0
            || member.offset != level.concatenation.offset
            || member.offset > expected - member.frames
        {
            return Err(Error::invalid(
                "invalid frame span in TILED_FULL concatenation",
            ));
        }
        if let Some(index) = matched {
            self.levels[index] =
                join_slide_member(self.levels[index].clone(), &level, member, expected)?;
            return Ok(());
        }
        validate_slide_span(&member, level.concatenation.total, expected)?;
        level.id = format!("concatenation:{}", level.concatenation.uid);
        level.members = vec![member];
        self.levels.push(level);
        Ok(())
    }
    pub fn validate_concatenations(&self) -> Result<(), Error> {
        for level in &self.levels {
            let (Some(first), Some(last)) = (level.members.first(), level.members.last()) else {
                continue;
            };
            let expected = level.concatenation_tile_count()?;
            let total = level.concatenation.total;
            let members = level.members.len() as i64;
            if members < 2
                || first.number != 1
                || members != last.number
                || (total > 0 && members != total)
                || level.frame_count != expected
            {
                return Err(Error::invalid("incomplete TILED_FULL concatenation"));
            }
        }
        Ok(())
    }
}
