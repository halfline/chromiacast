use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum AudioCodec {
    Opus,
    Aac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum VideoCodec {
    H264,
    Vp8,
    Hevc,
    Vp9,
    Av1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum CastMode {
    Mirroring,
    Remoting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamType {
    Audio,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Framerate {
    pub numerator: u32,
    pub denominator: u32,
}

impl Framerate {
    pub fn new(numerator: u32, denominator: u32) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    pub fn as_f64(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }
}

impl Serialize for Framerate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{}/{}", self.numerator, self.denominator))
    }
}

impl<'de> Deserialize<'de> for Framerate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let (num, den) = s
            .split_once('/')
            .ok_or_else(|| serde::de::Error::custom("expected num/den"))?;
        Ok(Framerate {
            numerator: num.parse().map_err(serde::de::Error::custom)?,
            denominator: den.parse().map_err(serde::de::Error::custom)?,
        })
    }
}

impl std::fmt::Display for Framerate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

impl AudioCodec {
    pub fn rtp_payload_type(self) -> u8 {
        match self {
            AudioCodec::Opus => 96,
            AudioCodec::Aac => 97,
        }
    }

    pub fn codec_name(self) -> &'static str {
        match self {
            AudioCodec::Opus => "opus",
            AudioCodec::Aac => "aac",
        }
    }
}

impl VideoCodec {
    pub fn rtp_payload_type(self) -> u8 {
        match self {
            VideoCodec::H264 => 101,
            VideoCodec::Vp8 => 100,
            VideoCodec::Hevc => 102,
            VideoCodec::Vp9 => 103,
            VideoCodec::Av1 => 104,
        }
    }

    pub fn codec_name(self) -> &'static str {
        match self {
            VideoCodec::H264 => "h264",
            VideoCodec::Vp8 => "vp8",
            VideoCodec::Hevc => "hevc",
            VideoCodec::Vp9 => "vp9",
            VideoCodec::Av1 => "av1",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_codec_serde_roundtrip() {
        let json = serde_json::to_string(&AudioCodec::Opus).unwrap();
        assert_eq!(json, "\"opus\"");
        let codec: AudioCodec = serde_json::from_str(&json).unwrap();
        assert_eq!(codec, AudioCodec::Opus);
    }

    #[test]
    fn video_codec_serde_roundtrip() {
        let json = serde_json::to_string(&VideoCodec::H264).unwrap();
        assert_eq!(json, "\"h264\"");
        let codec: VideoCodec = serde_json::from_str(&json).unwrap();
        assert_eq!(codec, VideoCodec::H264);
    }

    #[test]
    fn framerate_serde_roundtrip() {
        let fr = Framerate::new(30, 1);
        let json = serde_json::to_string(&fr).unwrap();
        assert_eq!(json, "\"30/1\"");
        let parsed: Framerate = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, fr);
    }

    #[test]
    fn framerate_fractional() {
        let fr = Framerate::new(30000, 1001);
        assert!((fr.as_f64() - 29.97).abs() < 0.01);
    }

    #[test]
    fn cast_mode_serde() {
        assert_eq!(
            serde_json::to_string(&CastMode::Mirroring).unwrap(),
            "\"mirroring\""
        );
    }
}
