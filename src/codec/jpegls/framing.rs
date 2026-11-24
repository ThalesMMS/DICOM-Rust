#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::{Error, ErrorKind};
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Soi,
    SoiCode,
    Marker,
    MarkerCode,
    LengthHigh,
    LengthLow,
    Payload,
    Entropy,
    EntropyCode,
    End,
}
struct Scanner {
    state: State,
    length: usize,
    remaining: usize,
    marker: u8,
    resume_entropy: bool,
}
fn layout(detail: &str) -> Error {
    Error::new(ErrorKind::EncapsulatedLayout, detail)
}
impl Scanner {
    fn feed(&mut self, b: u8) -> Result<bool, Error> {
        match self.state {
            State::Soi => {
                if b != 0xff {
                    return Err(layout("missing SOI"));
                }
                self.state = State::SoiCode;
            }
            State::SoiCode => {
                if b != 0xd8 {
                    return Err(layout("missing SOI"));
                }
                self.state = State::Marker;
            }
            State::Marker => {
                if b != 0xff {
                    return Err(layout("expected marker"));
                }
                self.state = State::MarkerCode;
            }
            State::MarkerCode => {
                if b == 0xff {
                    return Ok(false);
                }
                return self.begin_marker(b, false);
            }
            State::LengthHigh => {
                self.length = (b as usize) << 8;
                self.state = State::LengthLow;
            }
            State::LengthLow => {
                self.length |= b as usize;
                if self.length < 2 {
                    return Err(layout("invalid marker length"));
                }
                self.remaining = self.length.saturating_sub(2);
                self.state = State::Payload;
                if self.remaining == 0 {
                    self.end_segment();
                }
            }
            State::Payload => {
                self.remaining = self.remaining.saturating_sub(1);
                if self.remaining == 0 {
                    self.end_segment();
                }
            }
            State::Entropy => {
                if b == 0xff {
                    self.state = State::EntropyCode;
                }
            }
            State::EntropyCode => {
                if b == 0xff {
                    return Ok(false);
                }
                if b < 0x80 {
                    self.state = State::Entropy;
                    return Ok(false);
                }
                return self.begin_marker(b, true);
            }
            State::End => return Err(layout("data after EOI")),
        }
        Ok(false)
    }
    fn begin_marker(&mut self, b: u8, entropy: bool) -> Result<bool, Error> {
        if b == 0 || b == 0xd8 {
            return Err(layout("invalid/nested marker"));
        }
        if b == 0xd9 {
            self.state = State::End;
            return Ok(true);
        }
        if b == 0x01 || (0xd0..=0xd7).contains(&b) {
            if !entropy {
                return Err(layout("standalone marker outside scan"));
            }
            self.state = State::Entropy;
            return Ok(false);
        }
        self.marker = b;
        self.resume_entropy = entropy && b == 0xdc;
        self.state = State::LengthHigh;
        Ok(false)
    }
    fn end_segment(&mut self) {
        self.state = State::Marker;
        if self.marker == 0xda || self.resume_entropy {
            self.state = State::Entropy;
        }
    }
}
pub(crate) fn validate_frame(data: &[u8]) -> Result<(), Error> {
    let mut s = Scanner {
        state: State::Soi,
        length: 0,
        remaining: 0,
        marker: 0,
        resume_entropy: false,
    };
    for (i, &b) in data.iter().enumerate() {
        if s.feed(b)? {
            let remaining = data.get(i.saturating_add(1)..).unwrap_or_default();
            if remaining.len() > 1
                || matches!(remaining, [pad] if !(data.len().is_multiple_of(2) || *pad == 0))
            {
                return Err(layout("trailing bytes after EOI"));
            }
            return Ok(());
        }
    }
    Err(layout("missing EOI"))
}
