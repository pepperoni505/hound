// Hound -- A WAV encoding and decoding library in Rust
// Copyright (C) 2015 Ruud van Asseldonk
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, version 3 of the License only.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Hound, a WAV encoding and decoding library.
//!
//! TODO: Add some introductory text here.
//!
//! Examples
//! ========
//!
//! The following example renders a 440 Hz sine wave, and stores it as as a
//! mono wav file with a sample rate of 44.1 kHz and 16 bits per sample.
//!
//! ```
//! use std::f32::consts::PI;
//! use std::i16;
//! use hound;
//!
//! let spec = hound::WavSpec {
//!     channels: 1,
//!     sample_rate: 44100,
//!     bits_per_sample: 16
//! };
//! let mut writer = hound::WavWriter::create("sine.wav", spec).unwrap();
//! for t in (0 .. 44100).map(|x| x as f32 / 44100.0) {
//!     let sample = (t * 440.0 * 2.0 * PI).sin();
//!     let amplitude = i16::MAX as f32;
//!     writer.write_sample((sample * amplitude) as i16).unwrap();
//! }
//! writer.finalize().unwrap();
//! ```
//!
//! The following example computes the RMS (root mean square) of an audio file.
//!
//! ```
//! use hound;
//!
//! let mut reader = hound::WavReader::open("testsamples/pop.wav").unwrap();
//! let (sqr_sum, n) = reader.samples::<i16>()
//!                          .fold((0_f64, 0_u32), |(sqr_sum, n), s| {
//!     let sample = s.unwrap() as f64;
//!     (sqr_sum + sample * sample, n + 1)
//! });
//! println!("RMS is {}", (sqr_sum / n as f64).sqrt());
//! ```

#![warn(missing_docs)]

use std::error;
use std::fmt;
use std::io;
use std::io::Write;
use std::result;
use read::ReadExt;
use write::WriteExt;

mod read;
mod write;

pub use read::{WavReader, WavSamples};
pub use write::WavWriter;

/// A type that can be used to represent audio samples.
pub trait Sample {
    /// Writes the audio sample to the WAVE data chunk.
    fn write<W: io::Write>(self, writer: &mut W, bits: u16) -> Result<()>;

    /// Reads the audio sample from the WAVE data chunk.
    fn read<R: io::Read>(reader: &mut R, bytes: u16, bits: u16) -> Result<Self>;
}

/// Converts an unsigned integer in the range 0-255 to a signed one in the range -128-127.
///
/// Presumably, the designers of the WAVE format did not like consistency. For
/// all bit depths except 8, samples are stored as little-endian _signed_
/// integers. However, an 8-bit sample is instead stored as an _unsigned_
/// integer. Hound abstracts away this idiosyncrasy by providing only signed
/// sample types.
fn signed_from_u8(x: u8) -> i8 {
    (x as i16 - 128) as i8
}

/// Converts a signed integer in the range -128-127 to an unsigned one in the range 0-255.
fn u8_from_signed(x: i8) -> u8 {
    (x as i16 + 128) as u8
}

#[test]
fn u8_sign_conversion_is_bijective() {
    for x in (0 .. 255) {
        assert_eq!(x, u8_from_signed(signed_from_u8(x)));
    }
    for x in (-128 .. 127) {
        assert_eq!(x, signed_from_u8(u8_from_signed(x)));
    }
}

impl Sample for i8 {
    fn write<W: io::Write>(self, writer: &mut W, bits: u16) -> Result<()> {
        match bits {
            8 => Ok(try!(writer.write_u8(u8_from_signed(self as i8)))),
            16 => Ok(try!(writer.write_le_i16(self as i16))),
            24 => Ok(try!(writer.write_le_i24(self as i32))),
            32 => Ok(try!(writer.write_le_i32(self as i32))),
            _ => Err(Error::Unsupported)
        }
    }

    fn read<R: io::Read>(reader: &mut R, bytes: u16, bits: u16) -> Result<i8> {
        match (bytes, bits) {
            (1, 8) => Ok(try!(reader.read_u8().map(signed_from_u8))),
            // TODO: add a genric decoder for any bit depth.
            // TODO: differentiate between too wide and unsupported.
            _ => Err(Error::TooWide)
        }
    }
}

impl Sample for i16 {
    fn write<W: io::Write>(self, writer: &mut W, bits: u16) -> Result<()> {
        match bits {
            // TODO: do a bounds check on the downcast, or disallow writing
            // wider types than the bits per sample in the spec beforehand.
            8 => Ok(try!(writer.write_u8(u8_from_signed(self as i8)))),
            16 => Ok(try!(writer.write_le_i16(self))),
            24 => Ok(try!(writer.write_le_i24(self as i32))),
            32 => Ok(try!(writer.write_le_i32(self as i32))),
            _ => Err(Error::Unsupported)
        }
    }

    fn read<R: io::Read>(reader: &mut R, bytes: u16, bits: u16) -> Result<i16> {
        match (bytes, bits) {
            (1, 8) => Ok(try!(reader.read_u8().map(signed_from_u8).map(|x| x as i16))),
            (2, 16) => Ok(try!(reader.read_le_i16())),
            // TODO: add a generic decoder for any bit depth.
            // TODO: differentiate between too wide and unsupported.
            _ => Err(Error::TooWide)
        }
    }
}

/// Specifies properties of the audio data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WavSpec {
    /// The number of channels.
    pub channels: u16,

    /// The number of samples per second.
    ///
    /// A common value is 44100, this is 44.1 kHz which is used for CD audio.
    pub sample_rate: u32,

    /// The number of bits per sample.
    ///
    /// A common value is 16 bits per sample, which is used for CD audio.
    pub bits_per_sample: u16
}

/// The error type for operations on `WavReader` and `WavWriter`.
#[derive(Debug)]
pub enum Error {
    /// An IO error occured in the underlying reader or writer.
    IoError(io::Error),
    /// Ill-formed WAVE data was encountered.
    FormatError(&'static str),
    /// The sample has more bits than the data type of the sample iterator.
    TooWide,
    /// The number of samples written is not a multiple of the number of channels.
    UnfinishedSample,
    /// The format is not supported.
    Unsupported
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter)
           -> result::Result<(), fmt::Error> {
        match *self {
            Error::IoError(ref err) => err.fmt(formatter),
            Error::FormatError(reason) => {
                try!(formatter.write_str("Ill-formed WAVE file: "));
                formatter.write_str(reason)
            },
            Error::TooWide => {
                formatter.write_str("The sample has more bits than the data type of the sample iterator.")
            },
            Error::UnfinishedSample => {
                formatter.write_str("The number of samples written is not a multiple of the number of channels.")
            },
            Error::Unsupported => {
                formatter.write_str("The wave format of the file is not supported.")
            }
        }
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::IoError(ref err) => err.description(),
            Error::FormatError(reason) => reason,
            Error::TooWide => "the sample has more bits than the data type of the sample iterator",
            Error::UnfinishedSample => "the number of samples written is not a multiple of the number of channels",
            Error::Unsupported => "the wave format of the file is not supported"
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        match *self {
            Error::IoError(ref err) => Some(err),
            Error::FormatError(_) => None,
            Error::TooWide => None,
            Error::UnfinishedSample => None,
            Error::Unsupported => None
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::IoError(err)
    }
}

/// A type for results generated by Hound where the error type is hard-wired.
pub type Result<T> = result::Result<T, Error>;

#[test]
fn write_read_i16_is_lossless() {
    let mut buffer = io::Cursor::new(Vec::new());
    let write_spec = WavSpec {
        channels: 2,
        sample_rate: 44100,
        bits_per_sample: 16
    };

    {
        let mut writer = WavWriter::new(&mut buffer, write_spec);
        for s in (-1024_i16 .. 1024) {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
    }

    {
        buffer.set_position(0);
        let mut reader = WavReader::new(&mut buffer).unwrap();
        assert_eq!(&write_spec, reader.spec());
        for (expected, read) in (-1024_i16 .. 1024).zip(reader.samples()) {
            assert_eq!(expected, read.unwrap());
        }
    }
}

#[test]
fn write_read_i8_is_lossless() {
    let mut buffer = io::Cursor::new(Vec::new());
    let write_spec = WavSpec {
        channels: 16,
        sample_rate: 48000,
        bits_per_sample: 8
    };

    // Write `i8` samples.
    {
        let mut writer = WavWriter::new(&mut buffer, write_spec);
        // Iterate over i16 because we cannot specify the upper bound otherwise.
        for s in (-128_i16 .. 127 + 1) {
            writer.write_sample(s as i8).unwrap();
        }
        writer.finalize().unwrap();
    }

    // Then read them into `i16`.
    {
        buffer.set_position(0);
        let mut reader = WavReader::new(&mut buffer).unwrap();
        assert_eq!(&write_spec, reader.spec());
        for (expected, read) in (-128_i16 .. 127 + 1).zip(reader.samples()) {
            assert_eq!(expected, read.unwrap());
        }
    }
}
