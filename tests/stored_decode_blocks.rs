#![forbid(unsafe_code)]
use dicom_rust::display::{
    Frame, PixelFormat, StoredSampleReader, StoredTarget, decode_stored, decode_stored_into,
};
fn pixels(count: usize) -> Vec<u8> {
    let mut state = 0x2545_f491u32;
    let mut out = Vec::with_capacity(count * 2);
    for i in 0..count {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let word = match i % 97 {
            0 => 0x0000,
            1 => 0xffff,
            2 => 0x8000,
            3 => 0x7fff,
            4 => 0x0800,
            5 => 0x0fff,
            _ => state as u16,
        };
        out.extend_from_slice(&word.to_le_bytes());
    }
    out
}
fn frame<'a>(pixels: &'a [u8], columns: usize, format: PixelFormat) -> Frame<'a> {
    Frame {
        rows: (pixels.len() / 2 / columns) as i64,
        columns: columns as i64,
        pixels,
        format,
        ..Frame::default()
    }
}
fn reference<T: StoredTarget>(pixels: &[u8], format: PixelFormat) -> Result<Vec<i64>, usize> {
    let reader = StoredSampleReader::legacy(
        format.bits_allocated,
        format.bits_stored,
        format.high_bit,
        format.signed,
        format.big_endian,
    );
    let mut out = Vec::new();
    for (index, sample) in pixels.as_chunks::<2>().0.iter().enumerate() {
        let value = reader.value(sample);
        if value < T::MINIMUM || value > T::MAXIMUM {
            return Err(index);
        }
        out.push(value);
    }
    Ok(out)
}
fn check<T>(pixels: &[u8], columns: usize, format: PixelFormat, widen: fn(T) -> i64, fill: T)
where
    T: StoredTarget + PartialEq + std::fmt::Debug,
{
    let count = pixels.len() / 2;
    let mut destination = vec![fill; count + 3];
    let result = decode_stored_into(&mut destination, &frame(pixels, columns, format));
    match reference::<T>(pixels, format) {
        Ok(expected) => {
            assert!(result.is_ok(), "{format:?}: {result:?}");
            let got: Vec<i64> = destination[..count].iter().map(|&v| widen(v)).collect();
            assert_eq!(got, expected, "{format:?}");
            assert!(destination[count..].iter().all(|v| *v == fill));
        }
        Err(refused) => {
            assert!(
                result.is_err(),
                "{format:?}: sample {refused} is out of range"
            );
            let reader_values = reference::<T>(&pixels[..refused * 2], format);
            let got: Vec<i64> = destination[..refused].iter().map(|&v| widen(v)).collect();
            assert_eq!(
                Ok(got),
                reader_values,
                "{format:?}: before sample {refused}"
            );
            assert!(
                destination[refused..].iter().all(|v| *v == fill),
                "{format:?}: written from the refused sample {refused} on"
            );
        }
    }
}
#[test]
fn blocks_give_the_samples_of_the_reader() {
    let data = pixels(53 * 61);
    for big_endian in [false, true] {
        for signed in [false, true] {
            for (bits_stored, high_bit) in [
                (16, 15),
                (12, 11),
                (12, 15),
                (10, 12),
                (1, 0),
                (15, 14),
                (0, 0),
                (12, 3),
                (20, 15),
            ] {
                let format = PixelFormat {
                    bits_allocated: 16,
                    bits_stored,
                    high_bit,
                    signed,
                    big_endian,
                };
                check::<i16>(&data, 53, format, i64::from, 0x5a5a);
                check::<u16>(&data, 53, format, i64::from, 0xa5a5);
            }
        }
    }
}
#[test]
fn a_refused_sample_in_a_later_block_keeps_the_blocks_before_it() {
    let mut data: Vec<u8> = (0..4000u16)
        .flat_map(|v| (v % 30000).to_le_bytes())
        .collect();
    let refused = 2 * 1024 + 517;
    data[refused * 2..refused * 2 + 2].copy_from_slice(&40000u16.to_le_bytes());
    let format = PixelFormat {
        bits_allocated: 16,
        bits_stored: 16,
        high_bit: 15,
        signed: false,
        big_endian: false,
    };
    assert_eq!(reference::<i16>(&data, format), Err(refused));
    check::<i16>(&data, 40, format, i64::from, -1);
    check::<u16>(&data, 40, format, i64::from, 7);
}
#[test]
fn the_widened_decode_agrees() {
    let data = pixels(64 * 64);
    let format = PixelFormat {
        bits_allocated: 16,
        bits_stored: 12,
        high_bit: 11,
        signed: true,
        big_endian: false,
    };
    let frame = frame(&data, 64, format);
    let wide = decode_stored(&frame, 64 * 64, true).expect("signed 12-bit fits i16");
    let mut narrow = vec![0i16; 64 * 64];
    decode_stored_into(&mut narrow, &frame).expect("decode");
    assert!(wide.iter().zip(&narrow).all(|(w, n)| *w == i32::from(*n)));
}
