use super::{
    DEFAULT_TID_OFFSET, Error, ErrorKind, FramePixelShift, MASK_OPERATION_AVERAGE_SUBTRACTION,
    MASK_OPERATION_REVERSED_TID, MASK_OPERATION_TID, MaskSubtractionItem, Module, Shift,
};
use crate::textfmt::{float_v, quote, to_upper};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShiftSource {
    #[default]
    None,
    Module,
    SharedFunctionalGroup,
    PerFrameFunctionalGroup,
}
impl ShiftSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ShiftSource::None => "",
            ShiftSource::Module => "mask-module",
            ShiftSource::SharedFunctionalGroup => "shared-functional-group",
            ShiftSource::PerFrameFunctionalGroup => "per-frame-functional-group",
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanOptions {
    pub number_of_frames: i64,
    pub recover: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Recovery {
    pub err: Error,
    pub action: String,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub contrast_frame: i64,
    pub item: i64,
    pub operation: String,
    pub subtraction_item_id: Option<i64>,
    pub mask_frames: Vec<i64>,
    pub contrast_frames: Vec<i64>,
    pub shift: Shift,
    pub shift_source: ShiftSource,
    pub recoveries: Vec<Recovery>,
}
impl Module {
    pub fn plan(&self, frame: i64, opts: PlanOptions) -> Result<Plan, Error> {
        if self.items.is_empty() {
            return Err(Error::sentinel(ErrorKind::NoMaskModule));
        }
        let n = self.frame_count(opts)?;
        if frame < 1 || frame > n {
            return Err(Error::wrap(
                ErrorKind::FrameOutOfRange,
                format!("frame {frame}, object has {n} frames"),
            ));
        }
        for (index, item) in self.items.iter().enumerate() {
            match Planner::new(self, index, n, opts.recover) {
                Err(err) => {
                    if !item.applies_to_frame(frame) {
                        continue;
                    }
                    return Err(err);
                }
                Ok(p) => {
                    if ranges_contain(&p.ranges, frame) {
                        return p.plan(frame);
                    }
                }
            }
        }
        Err(Error::wrap(
            ErrorKind::FrameNotApplicable,
            format!("frame {frame}"),
        ))
    }
    pub fn plan_item(&self, item_index: i64, frame: i64, opts: PlanOptions) -> Result<Plan, Error> {
        if self.items.is_empty() {
            return Err(Error::sentinel(ErrorKind::NoMaskModule));
        }
        if item_index < 0 || item_index >= self.items.len() as i64 {
            return Err(Error::wrap(
                ErrorKind::ItemOutOfRange,
                format!("{item_index} of {}", self.items.len()),
            ));
        }
        let n = self.frame_count(opts)?;
        if frame < 1 || frame > n {
            return Err(Error::wrap(
                ErrorKind::FrameOutOfRange,
                format!("frame {frame}, object has {n} frames"),
            ));
        }
        let p = Planner::new(self, item_index as usize, n, opts.recover)?;
        if !ranges_contain(&p.ranges, frame) {
            return Err(Error::wrap(
                ErrorKind::FrameNotApplicable,
                format!("frame {frame}, item {item_index}"),
            ));
        }
        p.plan(frame)
    }
    fn frame_count(&self, opts: PlanOptions) -> Result<i64, Error> {
        let mut n = self.number_of_frames;
        if opts.number_of_frames > 0 {
            n = opts.number_of_frames;
        }
        if n < 1 {
            return Err(Error::wrap(ErrorKind::InvalidNumberOfFrames, n));
        }
        Ok(n)
    }
}
#[derive(Debug, Clone, Copy)]
struct FrameRange {
    start: i64,
    end: i64,
}
struct Planner<'a> {
    module: &'a Module,
    item: &'a MaskSubtractionItem,
    index: usize,
    frames: i64,
    recover: bool,
    op: &'static str,
    averaging: i64,
    offset: i64,
    ranges: Vec<FrameRange>,
    recoveries: Vec<Recovery>,
}
impl<'a> Planner<'a> {
    fn new(m: &'a Module, index: usize, frames: i64, recover: bool) -> Result<Planner<'a>, Error> {
        let mut p = Planner {
            module: m,
            item: &m.items[index],
            index,
            frames,
            recover,
            op: "",
            averaging: 0,
            offset: 0,
            ranges: Vec::new(),
            recoveries: Vec::new(),
        };
        p.op = p.operation()?;
        p.averaging = p.contrast_averaging()?;
        if p.op == MASK_OPERATION_TID || p.op == MASK_OPERATION_REVERSED_TID {
            p.offset = p.tid_offset()?;
        }
        p.ranges = p.applicable_ranges()?;
        Ok(p)
    }
    fn issue(&mut self, err: Error, action: &str) -> Result<(), Error> {
        let err = err.prefixed(format!("item {}", self.index));
        if !self.recover {
            return Err(err);
        }
        self.recoveries.push(Recovery {
            err,
            action: action.to_string(),
        });
        Ok(())
    }
    fn operation(&mut self) -> Result<&'static str, Error> {
        let op = to_upper(self.item.mask_operation.trim());
        for known in [
            MASK_OPERATION_AVERAGE_SUBTRACTION,
            MASK_OPERATION_TID,
            MASK_OPERATION_REVERSED_TID,
        ] {
            if op == known {
                return Ok(known);
            }
        }
        let err = Error::wrap(
            ErrorKind::UnsupportedMaskOperation,
            quote(&self.item.mask_operation),
        );
        if self.item.mask_frame_numbers.is_empty() {
            return Err(err.prefixed(format!("item {}", self.index)));
        }
        self.issue(err, "treated as AVG_SUB of its Mask Frame Numbers")?;
        Ok(MASK_OPERATION_AVERAGE_SUBTRACTION)
    }
    fn contrast_averaging(&mut self) -> Result<i64, Error> {
        let Some(averaging) = self.item.contrast_frame_averaging else {
            return Ok(1);
        };
        if averaging >= 1 {
            return Ok(averaging);
        }
        self.issue(
            Error::wrap(ErrorKind::InvalidContrastFrameAveraging, averaging),
            "no contrast frame averaging",
        )?;
        Ok(1)
    }
    fn tid_offset(&mut self) -> Result<i64, Error> {
        if let Some(offset) = self.item.tid_offset {
            return Ok(offset);
        }
        self.issue(
            Error::sentinel(ErrorKind::MissingTidOffset),
            &format!("used the default TID Offset {DEFAULT_TID_OFFSET}"),
        )?;
        Ok(DEFAULT_TID_OFFSET)
    }
    fn applicable_ranges(&mut self) -> Result<Vec<FrameRange>, Error> {
        if !self.item.applicable_frame_range.is_empty() {
            return self.explicit_ranges();
        }
        if self.op == MASK_OPERATION_TID {
            let start = 1.max(1 + self.offset);
            let end = self.frames.min(self.frames + self.offset) - self.averaging + 1;
            if start > end {
                return Ok(Vec::new());
            }
            Ok(vec![FrameRange { start, end }])
        } else if self.op == MASK_OPERATION_REVERSED_TID {
            self.issue(
                Error::sentinel(ErrorKind::MissingApplicableFrameRange),
                "applied the item to every frame",
            )?;
            Ok(vec![FrameRange {
                start: 1,
                end: self.frames,
            }])
        } else {
            let end = self.frames - self.averaging + 1;
            if end < 1 {
                return Ok(Vec::new());
            }
            Ok(vec![FrameRange { start: 1, end }])
        }
    }
    fn explicit_ranges(&mut self) -> Result<Vec<FrameRange>, Error> {
        let item = self.item;
        let mut values: &[i64] = &item.applicable_frame_range;
        if !values.len().is_multiple_of(2) {
            self.issue(
                Error::wrap(
                    ErrorKind::InvalidFrameRange,
                    format!("{} values", values.len()),
                ),
                "dropped the unpaired last value",
            )?;
            values = &values[..values.len() - 1];
        }
        let mut ranges = Vec::with_capacity(values.len() / 2);
        let mut i = 0;
        while i + 1 < values.len() {
            let mut r = FrameRange {
                start: values[i],
                end: values[i + 1],
            };
            i += 2;
            if r.start > r.end {
                self.issue(
                    Error::wrap(
                        ErrorKind::InvalidFrameRange,
                        format!("start {} after end {}", r.start, r.end),
                    ),
                    "swapped the pair",
                )?;
                std::mem::swap(&mut r.start, &mut r.end);
            }
            if r.start < 1 || r.end > self.frames {
                self.issue(
                    Error::wrap(
                        ErrorKind::InvalidFrameRange,
                        format!("{}-{} outside frames 1-{}", r.start, r.end, self.frames),
                    ),
                    "clipped the pair to the object's frames",
                )?;
                r.start = r.start.max(1);
                r.end = r.end.min(self.frames);
                if r.start > r.end {
                    continue;
                }
            }
            ranges.push(r);
        }
        Ok(ranges)
    }
    fn plan(mut self, frame: i64) -> Result<Plan, Error> {
        let mut plan = Plan {
            contrast_frame: frame,
            item: self.index as i64,
            operation: self.op.to_string(),
            subtraction_item_id: self.item.subtraction_item_id,
            ..Plan::default()
        };
        plan.contrast_frames = self.window(frame, self.averaging)?;
        plan.mask_frames = if self.op == MASK_OPERATION_AVERAGE_SUBTRACTION {
            self.average_mask_frames()?
        } else if self.op == MASK_OPERATION_TID {
            self.interval_mask(frame - self.offset)?
        } else {
            let ordinal = range_ordinal(&self.ranges, frame);
            self.interval_mask(frame - (self.offset - ordinal))?
        };
        (plan.shift, plan.shift_source) = self.shift(frame)?;
        plan.recoveries = self.recoveries;
        Ok(plan)
    }
    fn window(&mut self, first: i64, count: i64) -> Result<Vec<i64>, Error> {
        let mut last = first + count - 1;
        if last > self.frames {
            self.issue(
                Error::wrap(
                    ErrorKind::ContrastAveragingOverrun,
                    format!("frames {first}-{last}, object has {}", self.frames),
                ),
                "averaged only the contrast frames that exist",
            )?;
            last = self.frames;
        }
        Ok((first..=last).collect())
    }
    fn average_mask_frames(&mut self) -> Result<Vec<i64>, Error> {
        let item = self.item;
        if item.mask_frame_numbers.is_empty() {
            return Err(Error::sentinel(ErrorKind::MissingMaskFrameNumbers)
                .prefixed(format!("item {}", self.index)));
        }
        let mut frames = Vec::with_capacity(item.mask_frame_numbers.len());
        for &f in &item.mask_frame_numbers {
            if f >= 1 && f <= self.frames {
                frames.push(f);
                continue;
            }
            self.issue(
                Error::wrap(
                    ErrorKind::MaskFrameOutOfRange,
                    format!("mask frame {f}, object has {} frames", self.frames),
                ),
                "dropped the mask frame",
            )?;
        }
        if frames.is_empty() {
            return Err(Error::wrap(
                ErrorKind::MaskFrameOutOfRange,
                format!("no mask frame inside frames 1-{}", self.frames),
            )
            .prefixed(format!("item {}", self.index)));
        }
        Ok(frames)
    }
    fn interval_mask(&mut self, first: i64) -> Result<Vec<i64>, Error> {
        let last = first + self.averaging - 1;
        if first >= 1 && last <= self.frames {
            return Ok((first..=last).collect());
        }
        self.issue(
            Error::wrap(
                ErrorKind::MaskFrameOutOfRange,
                format!(
                    "mask frames {first}-{last}, object has {} frames",
                    self.frames
                ),
            ),
            "clamped the mask frames into the object",
        )?;
        let mut frames: Vec<i64> = Vec::new();
        for f in first..=last {
            let clamped = f.max(1).min(self.frames);
            if frames.last() != Some(&clamped) {
                frames.push(clamped);
            }
        }
        Ok(frames)
    }
    fn shift(&mut self, frame: i64) -> Result<(Shift, ShiftSource), Error> {
        let module = self.module;
        let per_frame: &[FramePixelShift] = module
            .per_frame_pixel_shifts
            .get((frame - 1) as usize)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut group = per_frame;
        let mut source = ShiftSource::PerFrameFunctionalGroup;
        if per_frame.is_empty() {
            group = &module.shared_pixel_shifts;
            source = ShiftSource::SharedFunctionalGroup;
        } else if !module.shared_pixel_shifts.is_empty() {
            self.issue(
                Error::wrap(ErrorKind::FunctionalGroupConflict, format!("frame {frame}")),
                "used the per-frame Frame Pixel Shift",
            )?;
        }
        if !group.is_empty()
            && let Some(values) = self.group_shift(group, frame)?
        {
            return self.parse_shift(values.as_deref().unwrap_or(&[]), source);
        }
        let item = self.item;
        if let Some(values) = &item.mask_sub_pixel_shift {
            return self.parse_shift(values, ShiftSource::Module);
        }
        Ok((Shift::default(), ShiftSource::None))
    }
    #[allow(clippy::type_complexity)]
    fn group_shift<'g>(
        &mut self,
        group: &'g [FramePixelShift],
        frame: i64,
    ) -> Result<Option<&'g Option<Vec<f64>>>, Error> {
        if let Some(id) = self.item.subtraction_item_id {
            for entry in group {
                if entry.subtraction_item_id == Some(id) {
                    return Ok(Some(&entry.mask_sub_pixel_shift));
                }
            }
            return Ok(None);
        }
        if group.len() == 1 {
            self.issue(
                Error::wrap(
                    ErrorKind::MissingSubtractionItemId,
                    format!("frame {frame} has a Frame Pixel Shift"),
                ),
                "used the frame's only Frame Pixel Shift",
            )?;
            return Ok(Some(&group[0].mask_sub_pixel_shift));
        }
        self.issue(
            Error::wrap(
                ErrorKind::MissingSubtractionItemId,
                format!("frame {frame} has {} Frame Pixel Shifts", group.len()),
            ),
            "used the Mask Module's shift",
        )?;
        Ok(None)
    }
    fn parse_shift(
        &mut self,
        values: &[f64],
        source: ShiftSource,
    ) -> Result<(Shift, ShiftSource), Error> {
        if values.len() == 2 && super::finite(values[0]) && super::finite(values[1]) {
            return Ok((
                Shift {
                    row: values[0],
                    column: values[1],
                },
                source,
            ));
        }
        let list: Vec<String> = values.iter().map(|v| float_v(*v)).collect();
        self.issue(
            Error::wrap(
                ErrorKind::InvalidPixelShift,
                format!("[{}] from {}", list.join(" "), source.as_str()),
            ),
            "used no shift",
        )?;
        Ok((Shift::default(), ShiftSource::None))
    }
}
fn ranges_contain(ranges: &[FrameRange], frame: i64) -> bool {
    ranges.iter().any(|r| frame >= r.start && frame <= r.end)
}
fn range_ordinal(ranges: &[FrameRange], frame: i64) -> i64 {
    (1..frame).filter(|&f| ranges_contain(ranges, f)).count() as i64
}
