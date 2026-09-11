# Download validation fixtures

These are real, short files generated locally with FFmpeg. Automated tests embed the bytes and do not require FFmpeg or network access.

- `silence.mp3`: 0.1 seconds of mono silence, MPEG-1 Layer III at 128 kbps, no ID3 or Xing header.
- `silence.m4a`: the same source encoded as AAC in an ISO BMFF container.
- `silence.wav`: 16-bit PCM WAV.
- `silence-rf64.wav`: 16-bit PCM in an RF64 WAV container, including `ds64` and sentinel chunk lengths.
- `silence-streaming.wav`: the same PCM written to FFmpeg's stdout as RIFF, with unknown RIFF/data lengths (`0xffffffff`).
- `silence-rf64-unfinalized.wav`: the same non-seekable output with `-rf64 always`, leaving the four `ds64` size/count fields zero. This is a negative fixture: FFmpeg exits successfully when reading it but produces zero PCM bytes, despite bytes following the empty declared data chunk.
- `silence.opus`: Opus in an Ogg container.
- `silence-large-tags.opus`: the same Opus audio with a 70,000-character comment, forcing the comment packet to continue across Ogg pages.
- `silence-multi-page.opus`: 2.1 seconds of Opus, with multiple audio pages so tests can truncate a later page after complete audio has already appeared.
- `black.mp4`: one second of 16x16 black H.264 video.

The audio source was `anullsrc=r=44100:cl=mono -t 0.1`; video was `color=c=black:s=16x16:r=1 -t 1`. All content is synthetic.

The streaming RIFF control and unfinalized RF64 negative fixture use `-c:a pcm_s16le -f wav -`, with `-rf64 always` added for RF64. The multiple-page Opus control uses the audio source above with `-t 2.1 -c:a libopus -f opus`. Tests do not run FFmpeg: they serve the embedded bytes through loopback HTTP or derive explicit truncations and changed length fields from these controls.

Ogg framing/header validation follows [RFC 7845 sections 3 and 5](https://www.rfc-editor.org/rfc/rfc7845.html#section-3). It checks complete pages/packets through EOF and length-prefixed comment fields without decoding Opus audio or allocating the declared comment size. RF64/BW64 sizes come from `ds64`; zero fields are zero lengths, unlike the unknown-length RIFF sentinel.
