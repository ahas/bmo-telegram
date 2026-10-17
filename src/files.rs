//! What a sound's bytes are, read off their first bytes: the extension it is named with and the type it goes
//! up under. Telegram wants OGG/Opus for a voice, which is what a synthesiser writes.

/// The extension and the type of a sound, by its first bytes; OGG where none other matches.
pub fn audio_type(bytes: &[u8]) -> (&'static str, &'static str) {
  if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE".as_slice()) {
    return ("wav", "audio/wav");
  }

  if bytes.starts_with(b"ID3")
    || (bytes.first() == Some(&0xff) && bytes.get(1).is_some_and(|byte| byte & 0xe0 == 0xe0))
  {
    return ("mp3", "audio/mpeg");
  }

  if bytes.get(4..8) == Some(b"ftyp".as_slice()) {
    return ("m4a", "audio/mp4");
  }

  if bytes.starts_with(b"fLaC") {
    return ("flac", "audio/flac");
  }

  ("ogg", "audio/ogg")
}
