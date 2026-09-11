use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use tempfile::TempPath;
use tokio::io::AsyncWriteExt;

use crate::api::types::Clip;
use crate::core::CliError;
use crate::net::http;

const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TOTAL_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const MEDIA_VALIDATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_DOWNLOAD_FILENAME_BYTES: usize = 240;

#[derive(Debug)]
pub struct StagedDownload {
    temp_path: Option<TempPath>,
    destination_path: PathBuf,
    force: bool,
}

impl StagedDownload {
    pub fn path(&self) -> &Path {
        self.temp_path
            .as_deref()
            .expect("staged download path is available before commit")
    }

    pub fn commit_after<F>(self, postprocess: F) -> Result<String, CliError>
    where
        F: FnOnce(&Path) -> Result<(), CliError>,
    {
        postprocess(self.path())?;
        self.commit()
    }

    fn commit(mut self) -> Result<String, CliError> {
        let temp_path = self
            .temp_path
            .take()
            .expect("staged download path is available before commit");
        commit_download(temp_path, &self.destination_path, self.force)?;
        Ok(self.destination_path.display().to_string())
    }
}

fn download_progress_bar(total: u64, quiet: bool) -> ProgressBar {
    if quiet {
        ProgressBar::hidden()
    } else {
        ProgressBar::new(total)
    }
}

pub async fn download_clip_url(
    clip: &Clip,
    output_dir: &str,
    url: &str,
    ext: &str,
    force: bool,
    quiet: bool,
) -> Result<String, CliError> {
    stage_clip_url(clip, output_dir, url, ext, force, quiet)
        .await?
        .commit()
}

pub async fn stage_clip_url(
    clip: &Clip,
    output_dir: &str,
    url: &str,
    ext: &str,
    force: bool,
    quiet: bool,
) -> Result<StagedDownload, CliError> {
    stage_clip_url_with_idle_timeout(
        clip,
        output_dir,
        url,
        ext,
        force,
        quiet,
        DOWNLOAD_IDLE_TIMEOUT,
    )
    .await
}

pub async fn preflight_clip_download(
    clip: &Clip,
    output_dir: &str,
    ext: &str,
    force: bool,
) -> Result<(), CliError> {
    let output_dir = Path::new(output_dir);
    ensure_output_directory(output_dir).await?;
    reject_existing_output(&planned_clip_download_path(clip, output_dir, ext), force).await?;
    verify_output_directory_writable(output_dir).await
}

pub(crate) fn planned_clip_download_path(
    clip: &Clip,
    output_dir: impl AsRef<Path>,
    ext: &str,
) -> PathBuf {
    output_dir.as_ref().join(download_filename(clip, ext))
}

async fn stage_clip_url_with_idle_timeout(
    clip: &Clip,
    output_dir: &str,
    url: &str,
    ext: &str,
    force: bool,
    quiet: bool,
    idle_timeout: Duration,
) -> Result<StagedDownload, CliError> {
    stage_clip_url_with_limits(
        clip,
        output_dir,
        url,
        ext,
        force,
        quiet,
        idle_timeout,
        DOWNLOAD_TOTAL_TIMEOUT,
        MAX_DOWNLOAD_BYTES,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn stage_clip_url_with_limits(
    clip: &Clip,
    output_dir: &str,
    url: &str,
    ext: &str,
    force: bool,
    quiet: bool,
    idle_timeout: Duration,
    total_timeout: Duration,
    max_bytes: u64,
) -> Result<StagedDownload, CliError> {
    let filename = download_filename(clip, ext);
    let output_dir = Path::new(output_dir);
    ensure_output_directory(output_dir).await?;
    let path = planned_clip_download_path(clip, output_dir, ext);
    reject_existing_output(&path, force).await?;

    let resp = tokio::time::timeout(idle_timeout, http::download_client()?.get(url).send())
        .await
        .map_err(|_| CliError::Download(format!("download stalled before response: {filename}")))?
        .map_err(CliError::Http)?
        .error_for_status()
        .map_err(CliError::Http)?;

    let total = resp.content_length().unwrap_or(0);
    if total > max_bytes {
        return Err(CliError::Download(format!(
            "download is {total} bytes, exceeding the {max_bytes}-byte safety limit: {filename}"
        )));
    }
    let pb = download_progress_bar(total, quiet);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40}] {bytes}/{total_bytes} ({eta})")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=> "),
    );
    pb.set_message(filename.clone());

    let temp_path = TempPath::try_from_path(temporary_path(output_dir))?;
    let temp_file_path: &Path = temp_path.as_ref();
    let mut file = tokio::fs::File::create(temp_file_path).await?;
    let result = tokio::time::timeout(total_timeout, async {
        let mut stream = resp.bytes_stream();
        let mut downloaded = 0_u64;
        while let Some(chunk) = tokio::time::timeout(idle_timeout, stream.next())
            .await
            .map_err(|_| CliError::Download(format!("download stalled: {filename}")))?
        {
            let chunk = chunk.map_err(CliError::Http)?;
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            if downloaded > max_bytes {
                return Err(CliError::Download(format!(
                    "download exceeded the {max_bytes}-byte safety limit: {filename}"
                )));
            }
            pb.inc(chunk.len() as u64);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok::<(), CliError>(())
    })
    .await
    .map_err(|_| CliError::Download(format!("download exceeded its total deadline: {filename}")))?;
    drop(file);

    if let Err(error) = result {
        pb.abandon_with_message("failed");
        return Err(error);
    }

    let validation_path = temp_file_path.to_path_buf();
    let validation_ext = ext.to_owned();
    let (validation_sender, validation_receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("sunox-media-validation".into())
        .spawn(move || {
            let _ = validation_sender
                .send(validate_downloaded_media(&validation_path, &validation_ext));
        })
        .map_err(CliError::Io)?;
    let validation = tokio::time::timeout(MEDIA_VALIDATION_TIMEOUT, validation_receiver)
        .await
        .map_err(|_| CliError::Download(format!("media validation timed out: {filename}")))?
        .map_err(|error| CliError::Download(format!("media validation task failed: {error}")))?;
    if let Err(error) = validation {
        pb.abandon_with_message("invalid media");
        return Err(error);
    }

    pb.finish_with_message("downloaded");
    Ok(StagedDownload {
        temp_path: Some(temp_path),
        destination_path: path,
        force,
    })
}

/// Check the downloaded container before the staging file can replace a destination.
/// CDN content types are advisory: valid media is also served as octet-stream.
/// This is a bounded structural check, not a complete codec decode.
fn validate_downloaded_media(path: &Path, ext: &str) -> Result<(), CliError> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size == 0 {
        return Err(CliError::Download("download response was empty".into()));
    }
    let prefix = media_bytes(&mut file, 0, size.min(512) as usize)?;
    let text = String::from_utf8_lossy(&prefix);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.starts_with('<') || text.starts_with('{') || text.starts_with('[') {
        return Err(CliError::Download(
            "download response contains a text error document instead of media".into(),
        ));
    }
    let valid = match ext {
        "mp3" => is_mp3(&mut file, size)?,
        "m4a" | "mp4" => is_iso_media(&mut file, size, ext)?,
        "wav" => is_wave(&mut file, size)?,
        "opus" => is_ogg_opus(&mut file, size)?,
        _ => false,
    };
    if !valid {
        return Err(CliError::Download(format!(
            "download response is not a recognizable {ext} media file"
        )));
    }
    Ok(())
}

fn media_bytes(file: &mut std::fs::File, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn is_mp3(file: &mut std::fs::File, size: u64) -> std::io::Result<bool> {
    if size < 4 {
        return Ok(false);
    }
    let mut offset = 0;
    let prefix = media_bytes(file, 0, size.min(10) as usize)?;
    if prefix.starts_with(b"ID3") {
        if prefix.len() < 10
            || !(2..=4).contains(&prefix[3])
            || prefix[6..10].iter().any(|b| b & 0x80 != 0)
        {
            return Ok(false);
        }
        let tag_size = prefix[6..10]
            .iter()
            .fold(0_u64, |size, byte| (size << 7) | u64::from(*byte));
        let footer = if prefix[3] == 4 && prefix[5] & 0x10 != 0 {
            10
        } else {
            0
        };
        offset = 10 + tag_size + footer;
    }
    let mut frames = 0_u64;
    while offset < size {
        // ID3v1 is a fixed 128-byte footer and may legally follow the final frame.
        if size - offset == 128 && media_bytes(file, offset, 3)? == b"TAG" {
            offset = size;
            break;
        }
        if size - offset < 4 {
            return Ok(false);
        }
        let header = media_bytes(file, offset, 4)?;
        let version = (header[1] >> 3) & 3;
        let bitrate_index = usize::from(header[2] >> 4);
        let sample_index = usize::from((header[2] >> 2) & 3);
        if header[0] != 0xff
            || header[1] & 0xe0 != 0xe0
            || version == 1
            || header[1] & 6 != 2
            || bitrate_index == 0
            || bitrate_index == 15
            || sample_index == 3
        {
            return Ok(false);
        }
        let rates = if version == 3 {
            [
                0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
            ]
        } else {
            [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160]
        };
        let sample_rate = [44100_u64, 48000, 32000][sample_index]
            / match version {
                3 => 1,
                2 => 2,
                _ => 4,
            };
        let frame_size = (if version == 3 { 144 } else { 72 }) * rates[bitrate_index] * 1000
            / sample_rate
            + u64::from((header[2] >> 1) & 1);
        if frame_size < 4 || frame_size > size - offset {
            return Ok(false);
        }
        offset += frame_size;
        frames += 1;
    }
    Ok(offset == size && frames > 0)
}

fn is_iso_media(file: &mut std::fs::File, size: u64, ext: &str) -> std::io::Result<bool> {
    let (mut offset, mut file_type, mut movie, mut media) = (0, false, false, false);
    let mut matching_track = false;
    let mut pending_fragment = false;
    for _ in 0..4096 {
        if offset + 8 > size {
            break;
        }
        let header = media_bytes(file, offset, 8)?;
        let mut box_size = u64::from(u32::from_be_bytes(
            header[..4].try_into().expect("box size"),
        ));
        let mut header_size = 8;
        if box_size == 1 {
            if offset + 16 > size {
                return Ok(false);
            }
            box_size = u64::from_be_bytes(
                media_bytes(file, offset + 8, 8)?
                    .try_into()
                    .expect("large box size"),
            );
            header_size = 16;
        } else if box_size == 0 {
            box_size = size - offset;
        }
        if box_size < header_size || box_size > size - offset {
            return Ok(false);
        }
        match &header[4..8] {
            b"ftyp" => file_type |= box_size >= header_size + 8,
            b"moov" => {
                movie |= box_size > header_size;
                matching_track |= iso_has_track(
                    file,
                    offset + header_size,
                    box_size - header_size,
                    if ext == "m4a" { b"soun" } else { b"vide" },
                    0,
                )?;
            }
            b"moof" => {
                if pending_fragment {
                    return Ok(false);
                }
                movie |= box_size > header_size;
                pending_fragment = true;
            }
            b"mdat" => {
                media |= box_size > header_size;
                pending_fragment = false;
            }
            _ => {}
        }
        offset += box_size;
    }
    Ok(offset == size && file_type && movie && media && matching_track && !pending_fragment)
}

fn iso_has_track(
    file: &mut std::fs::File,
    start: u64,
    length: u64,
    wanted_handler: &[u8; 4],
    _depth: u8,
) -> std::io::Result<bool> {
    let Some(end) = start.checked_add(length) else {
        return Ok(false);
    };
    let mut offset = start;
    let mut matching_track = false;
    for _ in 0..4096 {
        if offset == end {
            return Ok(matching_track);
        }
        if end.saturating_sub(offset) < 8 {
            return Ok(false);
        }
        let header = media_bytes(file, offset, 8)?;
        let mut box_size = u64::from(u32::from_be_bytes(
            header[..4].try_into().expect("box size"),
        ));
        let mut header_size = 8;
        if box_size == 1 {
            if end.saturating_sub(offset) < 16 {
                return Ok(false);
            }
            box_size = u64::from_be_bytes(
                media_bytes(file, offset + 8, 8)?
                    .try_into()
                    .expect("large box size"),
            );
            header_size = 16;
        } else if box_size == 0 {
            box_size = end - offset;
        }
        if box_size < header_size || box_size > end - offset {
            return Ok(false);
        }
        let kind = &header[4..8];
        if kind == b"trak" {
            let (matching_handler, sample_description) = iso_track_features(
                file,
                offset + header_size,
                box_size - header_size,
                wanted_handler,
                0,
            )?;
            matching_track |= matching_handler && sample_description;
        }
        offset += box_size;
    }
    Ok(false)
}

fn iso_track_features(
    file: &mut std::fs::File,
    start: u64,
    length: u64,
    wanted_handler: &[u8; 4],
    depth: u8,
) -> std::io::Result<(bool, bool)> {
    if depth > 4 {
        return Ok((false, false));
    }
    let Some(end) = start.checked_add(length) else {
        return Ok((false, false));
    };
    let mut offset = start;
    let mut matching_handler = false;
    let mut sample_description = false;
    for _ in 0..4096 {
        if offset == end {
            return Ok((matching_handler, sample_description));
        }
        if end.saturating_sub(offset) < 8 {
            return Ok((false, false));
        }
        let header = media_bytes(file, offset, 8)?;
        let mut box_size = u64::from(u32::from_be_bytes(
            header[..4].try_into().expect("box size"),
        ));
        let mut header_size = 8;
        if box_size == 1 {
            if end.saturating_sub(offset) < 16 {
                return Ok((false, false));
            }
            box_size = u64::from_be_bytes(
                media_bytes(file, offset + 8, 8)?
                    .try_into()
                    .expect("large box size"),
            );
            header_size = 16;
        } else if box_size == 0 {
            box_size = end - offset;
        }
        if box_size < header_size || box_size > end - offset {
            return Ok((false, false));
        }
        let kind = &header[4..8];
        if kind == b"hdlr" && box_size >= header_size + 12 {
            matching_handler |=
                media_bytes(file, offset + header_size + 8, 4)?.as_slice() == wanted_handler;
        } else if kind == b"stsd" && box_size >= header_size + 16 {
            let description = media_bytes(file, offset + header_size, 16)?;
            let entries = u32::from_be_bytes(description[4..8].try_into().expect("entry count"));
            let first_size = u64::from(u32::from_be_bytes(
                description[8..12].try_into().expect("sample entry size"),
            ));
            sample_description |=
                entries > 0 && first_size >= 8 && first_size <= box_size - header_size - 8;
        } else if matches!(kind, b"mdia" | b"minf" | b"stbl") {
            let (child_handler, child_description) = iso_track_features(
                file,
                offset + header_size,
                box_size - header_size,
                wanted_handler,
                depth + 1,
            )?;
            matching_handler |= child_handler;
            sample_description |= child_description;
        }
        offset += box_size;
    }
    Ok((false, false))
}

fn is_wave(file: &mut std::fs::File, size: u64) -> std::io::Result<bool> {
    if size < 12 {
        return Ok(false);
    }
    let header = media_bytes(file, 0, 12)?;
    if !matches!(&header[..4], b"RIFF" | b"RIFX" | b"RF64" | b"BW64") || &header[8..12] != b"WAVE" {
        return Ok(false);
    }
    let big_endian = &header[..4] == b"RIFX";
    let extended = matches!(&header[..4], b"RF64" | b"BW64");
    let riff_size = u64::from(wave_u32(&header[4..8], big_endian));
    let mut sizes = if extended {
        if riff_size != u64::from(u32::MAX) {
            return Ok(false);
        }
        let Some(sizes) = wave_extended_sizes(file, size)? else {
            return Ok(false);
        };
        sizes
    } else {
        WaveSizes {
            riff_size,
            data_size: None,
            additional_chunks: Vec::new(),
            streaming: riff_size == u64::from(u32::MAX),
        }
    };
    if sizes.streaming {
        // With both RIFF and data lengths set to the sentinel, EOF is not
        // distinguishable from an aligned truncation. Do not commit it as complete.
        return Ok(false);
    }
    let end = {
        let Some(end) = sizes.riff_size.checked_add(8) else {
            return Ok(false);
        };
        if end != size {
            return Ok(false);
        }
        end
    };
    let (mut offset, mut format, mut data) = (12, None, false);
    for _ in 0..4096 {
        if offset == end {
            return Ok(format.is_some() && data);
        }
        if end.saturating_sub(offset) < 8 {
            return Ok(false);
        }
        let chunk = media_bytes(file, offset, 8)?;
        let id: [u8; 4] = chunk[..4].try_into().expect("wave chunk ID");
        let mut chunk_size = u64::from(wave_u32(&chunk[4..8], big_endian));
        let first_data = &id == b"data" && !data;
        let sentinel = chunk_size == u64::from(u32::MAX);
        let consumes_remainder = sentinel && first_data && sizes.streaming;
        if sentinel {
            chunk_size = if consumes_remainder {
                end - offset - 8
            } else if first_data && extended {
                sizes.data_size.expect("finite RF64 has a data size")
            } else if extended {
                let Some(index) = sizes
                    .additional_chunks
                    .iter()
                    .position(|(chunk_id, _)| *chunk_id == id)
                else {
                    return Ok(false);
                };
                sizes.additional_chunks.remove(index).1
            } else {
                return Ok(false);
            };
        } else if first_data
            && let Some(expected) = sizes.data_size
            && expected != chunk_size
        {
            return Ok(false);
        }
        if chunk_size > end - offset - 8 {
            return Ok(false);
        }
        let payload = offset + 8;
        match &id {
            b"fmt " => {
                if format.is_some() {
                    return Ok(false);
                }
                format = wave_format(file, payload, chunk_size, big_endian)?;
                if format.is_none() {
                    return Ok(false);
                }
            }
            b"data" => {
                let Some(alignment) = format else {
                    return Ok(false);
                };
                if chunk_size == 0 || alignment.is_some_and(|n| chunk_size % u64::from(n) != 0) {
                    return Ok(false);
                }
                data = true;
            }
            _ => {}
        }
        // A finite RIFF chunk includes its odd-byte pad, including at EOF.
        // Only a genuinely unknown-length streaming data chunk consumes EOF.
        offset = payload
            + chunk_size
            + if consumes_remainder {
                0
            } else {
                chunk_size & 1
            };
        if offset > end {
            return Ok(false);
        }
    }
    Ok(false)
}

struct WaveSizes {
    riff_size: u64,
    data_size: Option<u64>,
    additional_chunks: Vec<([u8; 4], u64)>,
    streaming: bool,
}

fn wave_extended_sizes(file: &mut std::fs::File, size: u64) -> std::io::Result<Option<WaveSizes>> {
    if size < 48 {
        return Ok(None);
    }
    let header = media_bytes(file, 12, 36)?;
    let length = u64::from(wave_u32(&header[4..8], false));
    if &header[..4] != b"ds64" || length < 28 || length > size - 20 {
        return Ok(None);
    }
    let riff_size = u64::from_le_bytes(header[8..16].try_into().expect("RF64 size"));
    let data_size = u64::from_le_bytes(header[16..24].try_into().expect("RF64 data size"));
    let table_length = u64::from(wave_u32(&header[32..36], false));
    if table_length > 4096 || table_length * 12 > length - 28 {
        return Ok(None);
    }
    let mut additional_chunks = Vec::new();
    for index in 0..table_length {
        let entry = media_bytes(file, 48 + index * 12, 12)?;
        additional_chunks.push((
            entry[..4].try_into().expect("RF64 chunk ID"),
            u64::from_le_bytes(entry[4..].try_into().expect("RF64 chunk size")),
        ));
    }
    // Unlike a streaming RIFF sentinel, ds64 contains real lengths. Even
    // zero means zero: an unfinalized RF64 header must not authorize bytes
    // beyond its declared container or turn an empty data chunk into audio.
    Ok(Some(WaveSizes {
        riff_size,
        data_size: Some(data_size),
        additional_chunks,
        streaming: false,
    }))
}

fn wave_u32(bytes: &[u8], big_endian: bool) -> u32 {
    let bytes = bytes.try_into().expect("four-byte WAVE integer");
    if big_endian {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    }
}

/// Return PCM frame alignment where it is defined, without decoding codecs.
fn wave_format(
    file: &mut std::fs::File,
    offset: u64,
    length: u64,
    big_endian: bool,
) -> std::io::Result<Option<Option<u16>>> {
    if length < 16 || length == 17 {
        return Ok(None);
    }
    let header = media_bytes(file, offset, length.min(40) as usize)?;
    let word = |offset| {
        let bytes = header[offset..offset + 2].try_into().expect("WAVE word");
        if big_endian {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        }
    };
    let code = word(0);
    let alignment = word(12);
    if word(2) == 0 || wave_u32(&header[4..8], big_endian) == 0 || alignment == 0 {
        return Ok(None);
    }
    if length >= 18 && u64::from(word(16)) > length - 18 {
        return Ok(None);
    }
    let mut pcm = matches!(code, 1 | 3);
    if code == 0xfffe {
        if length < 40 || word(16) < 22 {
            return Ok(None);
        }
        pcm = matches!(wave_u32(&header[24..28], false), 1 | 3)
            && header[28..40] == [0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71];
    }
    Ok(Some(pcm.then_some(alignment)))
}

fn is_ogg_opus(file: &mut std::fs::File, size: u64) -> std::io::Result<bool> {
    let mut offset = 0;
    let mut stream: Option<OpusStream> = None;
    while offset < size {
        let Some(page) = ogg_page(file, offset, size)? else {
            return Ok(false);
        };
        if stream.as_ref().is_none_or(|stream| stream.ended) {
            // RFC 7845: the ID header is one complete packet alone on BOS.
            if page.flags != 2
                || page.sequence != 0
                || page.lacing.is_empty()
                || page.lacing.last() == Some(&255)
                || page.lacing[..page.lacing.len() - 1]
                    .iter()
                    .any(|n| *n != 255)
            {
                return Ok(false);
            }
            let identification =
                media_bytes(file, page.payload, (page.end - page.payload) as usize)?;
            if !opus_identification(&identification) {
                return Ok(false);
            }
            stream = Some(OpusStream {
                serial: page.serial,
                next_sequence: 1,
                comments: Some(OpusComments::default()),
                pending_size: 0,
                pending: false,
                audio: false,
                ended: false,
            });
        } else {
            let stream = stream.as_mut().expect("active Opus stream");
            if page.serial != stream.serial
                || page.sequence != stream.next_sequence
                || page.flags & 2 != 0
                || (!page.lacing.is_empty() && (page.flags & 1 != 0) != stream.pending)
            {
                return Ok(false);
            }
            stream.next_sequence = stream.next_sequence.wrapping_add(1);
            let comment_payload = if stream.comments.is_some() {
                Some(media_bytes(
                    file,
                    page.payload,
                    (page.end - page.payload) as usize,
                )?)
            } else {
                None
            };
            let mut payload = 0;
            for (index, length) in page.lacing.iter().copied().enumerate() {
                stream.pending_size += u64::from(length);
                if let Some(comments) = stream.comments.as_mut() {
                    let bytes = comment_payload.as_ref().expect("comment page payload");
                    if !comments.consume(&bytes[payload..payload + usize::from(length)]) {
                        return Ok(false);
                    }
                    if length < 255 {
                        // The complete comment packet ends its page. Audio
                        // begins on a later page, even for very large tags.
                        if !comments.complete() || index + 1 != page.lacing.len() {
                            return Ok(false);
                        }
                        stream.comments = None;
                    }
                } else if length < 255 {
                    if stream.pending_size == 0 {
                        return Ok(false);
                    }
                    stream.audio = true;
                }
                stream.pending = length == 255;
                if !stream.pending {
                    stream.pending_size = 0;
                }
                payload += usize::from(length);
            }
            if page.flags & 4 != 0 {
                if stream.pending || stream.comments.is_some() || !stream.audio {
                    return Ok(false);
                }
                stream.ended = true;
            }
        }
        offset = page.end;
    }
    // A captured stream need not have an EOS flag, but every declared page
    // and packet must be complete. Never stop after the first audio page.
    Ok(stream.is_some_and(|stream| stream.comments.is_none() && stream.audio && !stream.pending))
}

struct OpusStream {
    serial: u32,
    next_sequence: u32,
    comments: Option<OpusComments>,
    pending_size: u64,
    pending: bool,
    audio: bool,
    ended: bool,
}

fn opus_identification(header: &[u8]) -> bool {
    if header.len() < 19 || &header[..8] != b"OpusHead" || header[8] > 15 || header[9] == 0 {
        return false;
    }
    if header[18] == 0 {
        header[9] <= 2 && (header[8] > 1 || header.len() == 19)
    } else {
        // Nonzero mapping families include stream counts and one mapping
        // byte per output channel. Later minor versions may append fields.
        if header.len() < 21 + usize::from(header[9]) {
            return false;
        }
        let streams = u16::from(header[19]);
        let coupled = u16::from(header[20]);
        streams > 0
            && coupled <= streams
            && streams + coupled <= 255
            && header[21..21 + usize::from(header[9])]
                .iter()
                .all(|channel| *channel == 255 || u16::from(*channel) < streams + coupled)
    }
}

#[derive(Default)]
struct OpusComments {
    phase: CommentPhase,
    signature: usize,
    word: [u8; 4],
    word_bytes: usize,
    remaining: u32,
    comments: u32,
}

#[derive(Default)]
enum CommentPhase {
    #[default]
    Signature,
    VendorLength,
    Vendor,
    Count,
    CommentLength,
    Comment,
    Padding,
}

impl OpusComments {
    // Validate length-prefixed fields across arbitrary Ogg page boundaries,
    // retaining only a four-byte integer instead of allocating the tag size.
    fn consume(&mut self, mut bytes: &[u8]) -> bool {
        while !bytes.is_empty() {
            match self.phase {
                CommentPhase::Signature => {
                    let n = bytes.len().min(8 - self.signature);
                    if bytes[..n] != b"OpusTags"[self.signature..self.signature + n] {
                        return false;
                    }
                    self.signature += n;
                    bytes = &bytes[n..];
                    if self.signature == 8 {
                        self.phase = CommentPhase::VendorLength;
                    }
                }
                CommentPhase::VendorLength | CommentPhase::Count | CommentPhase::CommentLength => {
                    let n = bytes.len().min(4 - self.word_bytes);
                    self.word[self.word_bytes..self.word_bytes + n].copy_from_slice(&bytes[..n]);
                    self.word_bytes += n;
                    bytes = &bytes[n..];
                    if self.word_bytes == 4 {
                        let value = u32::from_le_bytes(self.word);
                        self.word_bytes = 0;
                        match self.phase {
                            CommentPhase::VendorLength => {
                                self.remaining = value;
                                self.phase = if value == 0 {
                                    CommentPhase::Count
                                } else {
                                    CommentPhase::Vendor
                                };
                            }
                            CommentPhase::Count => {
                                self.comments = value;
                                self.next_comment();
                            }
                            CommentPhase::CommentLength => {
                                self.remaining = value;
                                if value == 0 {
                                    self.comments -= 1;
                                    self.next_comment();
                                } else {
                                    self.phase = CommentPhase::Comment;
                                }
                            }
                            _ => unreachable!(),
                        }
                    }
                }
                CommentPhase::Vendor | CommentPhase::Comment => {
                    let n = bytes.len().min(self.remaining as usize);
                    bytes = &bytes[n..];
                    self.remaining -= n as u32;
                    if self.remaining == 0 {
                        if matches!(self.phase, CommentPhase::Vendor) {
                            self.phase = CommentPhase::Count;
                        } else {
                            self.comments -= 1;
                            self.next_comment();
                        }
                    }
                }
                CommentPhase::Padding => return true,
            }
        }
        true
    }

    fn next_comment(&mut self) {
        self.phase = if self.comments == 0 {
            CommentPhase::Padding
        } else {
            CommentPhase::CommentLength
        };
    }

    fn complete(&self) -> bool {
        matches!(self.phase, CommentPhase::Padding)
    }
}

struct OggPage {
    flags: u8,
    serial: u32,
    sequence: u32,
    lacing: Vec<u8>,
    payload: u64,
    end: u64,
}

fn ogg_page(file: &mut std::fs::File, offset: u64, size: u64) -> std::io::Result<Option<OggPage>> {
    if size.saturating_sub(offset) < 27 {
        return Ok(None);
    }
    let header = media_bytes(file, offset, 27)?;
    if &header[..4] != b"OggS" || header[4] != 0 || header[5] & !7 != 0 {
        return Ok(None);
    }
    let segments = usize::from(header[26]);
    let payload_offset = offset + 27 + segments as u64;
    if payload_offset > size {
        return Ok(None);
    }
    let lacing = media_bytes(file, offset + 27, segments)?;
    let payload_size: u64 = lacing.iter().map(|value| u64::from(*value)).sum();
    if payload_size > size - payload_offset {
        return Ok(None);
    }
    let end = payload_offset + payload_size;
    let expected_crc = u32::from_le_bytes(header[22..26].try_into().expect("Ogg page CRC"));
    let mut page = media_bytes(file, offset, (end - offset) as usize)?;
    page[22..26].fill(0);
    if ogg_crc(&page) != expected_crc {
        return Ok(None);
    }
    Ok(Some(OggPage {
        flags: header[5],
        serial: u32::from_le_bytes(header[14..18].try_into().expect("Ogg stream serial")),
        sequence: u32::from_le_bytes(header[18..22].try_into().expect("Ogg page sequence")),
        lacing,
        payload: payload_offset,
        end,
    }))
}

fn ogg_crc(bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = ogg_crc_table();
    let mut crc = 0_u32;
    for byte in bytes {
        crc = (crc << 8) ^ TABLE[usize::from((crc >> 24) as u8 ^ *byte)];
    }
    crc
}

const fn ogg_crc_table() -> [u32; 256] {
    let mut table = [0_u32; 256];
    let mut index = 0;
    while index < table.len() {
        let mut value = (index as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000_0000 != 0 {
                (value << 1) ^ 0x04c1_1db7
            } else {
                value << 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

fn download_filename(clip: &Clip, ext: &str) -> String {
    let slug = clip
        .title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .replace("--", "-")
        .trim_matches('-')
        .to_string();
    let short_id = clip.id.chars().take(8).collect::<String>();
    let suffix = format!("-{short_id}.{ext}");
    let max_slug_bytes = MAX_DOWNLOAD_FILENAME_BYTES.saturating_sub(suffix.len());
    let mut slug_end = slug.len().min(max_slug_bytes);
    while !slug.is_char_boundary(slug_end) {
        slug_end -= 1;
    }
    let slug = slug[..slug_end].trim_matches('-');
    let slug = if slug.is_empty() { "untitled" } else { slug };
    format!("{slug}{suffix}")
}

async fn ensure_output_directory(output_dir: &Path) -> Result<(), CliError> {
    match tokio::fs::metadata(output_dir).await {
        Ok(metadata) if metadata.is_dir() => return Ok(()),
        Ok(_) => {
            return Err(CliError::Download(format!(
                "output path exists but is not a directory: {}",
                output_dir.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    tokio::fs::create_dir_all(output_dir).await?;
    let metadata = tokio::fs::metadata(output_dir).await?;
    if !metadata.is_dir() {
        return Err(CliError::Download(format!(
            "output path exists but is not a directory: {}",
            output_dir.display()
        )));
    }
    Ok(())
}

async fn verify_output_directory_writable(output_dir: &Path) -> Result<(), CliError> {
    let metadata = tokio::fs::metadata(output_dir).await?;
    if metadata.permissions().readonly() {
        return Err(output_directory_not_writable(output_dir, None));
    }

    let probe_path = TempPath::try_from_path(temporary_path(output_dir))?;
    let probe_file_path: &Path = probe_path.as_ref();
    let probe_result = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(probe_file_path)
        .await;
    match probe_result {
        Ok(probe) => {
            drop(probe);
            drop(probe_path);
            Ok(())
        }
        Err(error) => Err(output_directory_not_writable(output_dir, Some(&error))),
    }
}

fn output_directory_not_writable(output_dir: &Path, error: Option<&std::io::Error>) -> CliError {
    let reason = error.map(|error| format!(" ({error})")).unwrap_or_default();
    CliError::Download(format!(
        "output directory is not writable: {}{reason}",
        output_dir.display()
    ))
}

async fn reject_existing_output(path: &Path, force: bool) -> Result<(), CliError> {
    if output_exists_as_regular_file(path).await? && !force {
        return Err(existing_output_error(path));
    }
    Ok(())
}

fn commit_download(temp_path: TempPath, path: &Path, force: bool) -> Result<(), CliError> {
    let result = if force {
        temp_path.persist(path)
    } else {
        temp_path.persist_noclobber(path)
    };
    result.map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            existing_output_error(path)
        } else {
            error.error.into()
        }
    })
}

async fn output_exists_as_regular_file(path: &Path) -> Result<bool, CliError> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err(CliError::Download(format!(
            "output path exists but is not a file: {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn existing_output_error(path: &Path) -> CliError {
    CliError::Download(format!(
        "output file already exists: {} (pass --force to replace it)",
        path.display()
    ))
}

fn temporary_path(output_dir: &Path) -> PathBuf {
    output_dir.join(format!(".sunox-{}.part", uuid::Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use crate::api::types::Clip;
    use crate::core::CliError;

    use super::{
        DOWNLOAD_IDLE_TIMEOUT, download_clip_url, download_filename, download_progress_bar,
        preflight_clip_download, stage_clip_url, stage_clip_url_with_idle_timeout,
        stage_clip_url_with_limits, validate_downloaded_media,
    };

    fn clip() -> Clip {
        Clip {
            id: "clip-a".into(),
            title: "Track".into(),
            status: "complete".into(),
            model_name: "chirp-fenix".into(),
            audio_url: None,
            video_url: None,
            image_url: None,
            created_at: "2026-07-10T00:00:00Z".into(),
            is_trashed: None,
            is_download_unlocked: None,
            action_config: None,
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
            extra: Default::default(),
        }
    }

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sunox-{name}-{}", uuid::Uuid::new_v4()))
    }

    fn mp3_fixture() -> Vec<u8> {
        include_bytes!("../../tests/fixtures/download/silence.mp3").to_vec()
    }

    fn iso_fixture() -> Vec<u8> {
        include_bytes!("../../tests/fixtures/download/silence.m4a").to_vec()
    }

    fn wave_fixture() -> Vec<u8> {
        include_bytes!("../../tests/fixtures/download/silence.wav").to_vec()
    }

    fn opus_fixture() -> Vec<u8> {
        include_bytes!("../../tests/fixtures/download/silence.opus").to_vec()
    }

    fn rf64_fixture() -> Vec<u8> {
        include_bytes!("../../tests/fixtures/download/silence-rf64.wav").to_vec()
    }

    fn assert_media_fixture(bytes: &[u8], ext: &str, valid: bool, name: &str) {
        let dir = tempfile::tempdir().expect("media validation fixture");
        let path = dir.path().join(format!("fixture.{ext}"));
        std::fs::write(&path, bytes).expect("fixture bytes");
        let result = validate_downloaded_media(&path, ext);
        assert_eq!(result.is_ok(), valid, "{name}: {result:?}");
    }

    fn ogg_page_end(bytes: &[u8], offset: usize) -> usize {
        let segments = usize::from(bytes[offset + 26]);
        offset
            + 27
            + segments
            + bytes[offset + 27..offset + 27 + segments]
                .iter()
                .map(|length| usize::from(*length))
                .sum::<usize>()
    }

    #[test]
    fn opus_requires_complete_headers_and_audio_through_the_last_page() {
        let opus = opus_fixture();
        let id_end = ogg_page_end(&opus, 0);
        let tags_end = ogg_page_end(&opus, id_end);
        let large = include_bytes!("../../tests/fixtures/download/silence-large-tags.opus");
        let large_first_tags_end = ogg_page_end(large, ogg_page_end(large, 0));
        let multi = include_bytes!("../../tests/fixtures/download/silence-multi-page.opus");
        let first_audio_end = ogg_page_end(multi, ogg_page_end(multi, ogg_page_end(multi, 0)));
        assert!(
            first_audio_end < multi.len(),
            "fixture needs more than one audio page"
        );
        for (name, bytes) in [
            ("headers without audio", opus[..tags_end].to_vec()),
            (
                "truncated first audio header",
                opus[..tags_end + 4].to_vec(),
            ),
            (
                "unfinished comment continuation",
                large[..large_first_tags_end].to_vec(),
            ),
            (
                "truncated later audio header",
                multi[..first_audio_end + 4].to_vec(),
            ),
            (
                "truncated last audio payload",
                multi[..multi.len() - 1].to_vec(),
            ),
        ] {
            assert_media_fixture(&bytes, "opus", false, name);
        }
        let mut vendor_too_long = opus.clone();
        let tags_payload = id_end + 27 + usize::from(opus[id_end + 26]);
        vendor_too_long[tags_payload + 8..tags_payload + 12]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert_media_fixture(
            &vendor_too_long,
            "opus",
            false,
            "declared vendor string exceeds packet",
        );
        let mut comments_too_many = opus.clone();
        let vendor_length = u32::from_le_bytes(
            opus[tags_payload + 8..tags_payload + 12]
                .try_into()
                .unwrap(),
        ) as usize;
        let comment_count = tags_payload + 12 + vendor_length;
        comments_too_many[comment_count..comment_count + 4]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert_media_fixture(
            &comments_too_many,
            "opus",
            false,
            "declared comment count exceeds packet",
        );
        let mut missing_mapping = opus.clone();
        missing_mapping[28 + 18] = 1;
        assert_media_fixture(
            &missing_mapping,
            "opus",
            false,
            "truncated identification mapping table",
        );
        let mut broken_continuation = large.to_vec();
        broken_continuation[large_first_tags_end + 5] &= !1;
        assert_media_fixture(
            &broken_continuation,
            "opus",
            false,
            "missing continuation flag",
        );
        let mut skipped_page = multi.to_vec();
        skipped_page[first_audio_end + 18..first_audio_end + 22]
            .copy_from_slice(&100_u32.to_le_bytes());
        assert_media_fixture(&skipped_page, "opus", false, "missing audio page sequence");
        let mut trailing_header = opus;
        trailing_header.extend_from_slice(b"OggS");
        assert_media_fixture(&trailing_header, "opus", false, "truncated page after EOS");
        assert_media_fixture(multi, "opus", true, "complete multiple audio pages");
        // A captured stream with complete pages/packets need not have an EOS marker.
        let mut captured = multi.to_vec();
        let mut last_page = 0;
        while ogg_page_end(&captured, last_page) < captured.len() {
            last_page = ogg_page_end(&captured, last_page);
        }
        captured[last_page + 5] &= !4;
        captured[last_page + 22..last_page + 26].fill(0);
        let crc = super::ogg_crc(&captured[last_page..]).to_le_bytes();
        captured[last_page + 22..last_page + 26].copy_from_slice(&crc);
        assert_media_fixture(
            &captured,
            "opus",
            true,
            "complete captured stream without EOS",
        );
    }

    #[test]
    fn wave_uses_declared_lengths_and_checks_chunks_after_audio() {
        assert_media_fixture(
            include_bytes!("../../tests/fixtures/download/silence-rf64-unfinalized.wav"),
            "wav",
            false,
            "unfinalized RF64 ds64 declares no audio",
        );
        let rf64 = rf64_fixture();
        let data_header = rf64.windows(4).position(|bytes| bytes == b"data").unwrap();
        let mut truncated = rf64[..data_header + 9].to_vec();
        assert_media_fixture(
            &truncated,
            "wav",
            false,
            "RF64 declared RIFF length exceeds file",
        );
        let size = (truncated.len() as u64 - 8).to_le_bytes();
        truncated[20..28].copy_from_slice(&size);
        assert_media_fixture(
            &truncated,
            "wav",
            false,
            "RF64 declared data length exceeds file",
        );
        truncated[28..36].copy_from_slice(&1_u64.to_le_bytes());
        assert_media_fixture(&truncated, "wav", false, "RF64 incomplete PCM sample");
        let mut bw64 = rf64.clone();
        bw64[..4].copy_from_slice(b"BW64");
        assert_media_fixture(&bw64, "wav", true, "complete BW64");
        bw64.truncate(data_header + 9);
        assert_media_fixture(&bw64, "wav", false, "truncated BW64");
        let mut missing_ds64 = rf64.clone();
        missing_ds64[12..16].copy_from_slice(b"JUNK");
        assert_media_fixture(&missing_ds64, "wav", false, "missing mandatory ds64");
        let mut incomplete_table = rf64;
        incomplete_table[44..48].copy_from_slice(&1_u32.to_le_bytes());
        assert_media_fixture(&incomplete_table, "wav", false, "truncated ds64 table");
        for (name, suffix) in [
            ("partial chunk header after data", &b"JUN"[..]),
            ("missing odd chunk pad after data", &b"JUNK\x01\0\0\0x"[..]),
            (
                "truncated chunk payload after data",
                &b"JUNK\x05\0\0\0xy"[..],
            ),
        ] {
            let mut bytes = wave_fixture();
            bytes.extend_from_slice(suffix);
            let size = (bytes.len() as u32 - 8).to_le_bytes();
            bytes[4..8].copy_from_slice(&size);
            assert_media_fixture(&bytes, "wav", false, name);
        }
    }

    #[test]
    fn rf64_resolves_additional_sentinel_chunks_from_the_ds64_table() {
        let mut bytes = rf64_fixture();
        bytes[16..20].copy_from_slice(&40_u32.to_le_bytes());
        bytes[44..48].copy_from_slice(&1_u32.to_le_bytes());
        let mut extra = b"JUNK".to_vec();
        extra.extend_from_slice(&3_u64.to_le_bytes());
        bytes.splice(48..48, extra);
        bytes.extend_from_slice(b"JUNK\xff\xff\xff\xffabc\0");
        let size = (bytes.len() as u64 - 8).to_le_bytes();
        bytes[20..28].copy_from_slice(&size);
        assert_media_fixture(&bytes, "wav", true, "table-sized odd JUNK chunk");
        bytes[52..60].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_media_fixture(&bytes, "wav", false, "table-sized chunk exceeds file");
    }

    #[tokio::test]
    async fn valid_media_containers_are_accepted_with_octet_stream_content_type() {
        // A real TIT2 frame plus 2 KiB of padding; the syncsafe tag length
        // includes the frame and padding, but not the ten-byte ID3 header.
        let mut tagged_mp3 = b"ID3\x04\0\0\0\0\x10\x0cTIT2\0\0\0\x02\0\0\x03x".to_vec();
        tagged_mp3.resize(10 + 2060, 0);
        tagged_mp3.extend(mp3_fixture());
        let mut wave_with_unknown_chunk = wave_fixture();
        wave_with_unknown_chunk.splice(12..12, *b"JUNK\x03\0\0\0abc\0");
        let riff_size = (wave_with_unknown_chunk.len() as u32 - 8).to_le_bytes();
        wave_with_unknown_chunk[4..8].copy_from_slice(&riff_size);
        for (ext, bytes) in [
            ("mp3", mp3_fixture()),
            ("mp3", tagged_mp3),
            ("m4a", iso_fixture()),
            (
                "mp4",
                include_bytes!("../../tests/fixtures/download/black.mp4").to_vec(),
            ),
            ("wav", wave_fixture()),
            ("wav", wave_with_unknown_chunk),
            (
                "wav",
                include_bytes!("../../tests/fixtures/download/silence-rf64.wav").to_vec(),
            ),
            ("opus", opus_fixture()),
            (
                "opus",
                include_bytes!("../../tests/fixtures/download/silence-large-tags.opus").to_vec(),
            ),
        ] {
            let dir = tempfile::tempdir().expect("output directory");
            let url = media_server(&bytes, "application/octet-stream").await;
            let path = download_clip_url(
                &clip(),
                &dir.path().to_string_lossy(),
                &url,
                ext,
                false,
                true,
            )
            .await
            .expect("valid container must survive generic CDN content type");
            assert_eq!(std::fs::read(path).expect("downloaded media"), bytes);
        }
        assert_media_fixture(
            include_bytes!("../../tests/fixtures/download/silence-streaming.wav"),
            "wav",
            false,
            "unknown-length streaming RIFF cannot prove completeness",
        );
        let streaming = include_bytes!("../../tests/fixtures/download/silence-streaming.wav");
        assert_media_fixture(
            &streaming[..streaming.len() - 2],
            "wav",
            false,
            "aligned truncation of streaming RIFF",
        );
    }

    #[tokio::test]
    async fn invalid_downloads_preserve_forced_destinations_and_remove_staging_files() {
        for ext in ["mp3", "m4a", "wav", "opus", "mp4"] {
            for (body, content_type) in [
                (&b""[..], "application/octet-stream"),
                (
                    &b"  <!DOCTYPE html><html>Maintenance</html>"[..],
                    "text/html",
                ),
                (
                    &b"{\"error\":\"upstream unavailable\"}"[..],
                    "application/octet-stream",
                ),
                (&b"not media"[..], "audio/mpeg"),
            ] {
                let dir = tempfile::tempdir().expect("output directory");
                let destination = dir.path().join(download_filename(&clip(), ext));
                std::fs::write(&destination, b"existing good download").expect("existing media");
                let url = media_server(body, content_type).await;
                let error = download_clip_url(
                    &clip(),
                    &dir.path().to_string_lossy(),
                    &url,
                    ext,
                    true,
                    true,
                )
                .await
                .expect_err("invalid response must never replace existing media");
                assert!(matches!(error, CliError::Download(_)), "{error}");
                assert_eq!(
                    std::fs::read(&destination).expect("existing media"),
                    b"existing good download"
                );
                let remaining: Vec<_> = std::fs::read_dir(dir.path())
                    .expect("output files")
                    .map(|entry| entry.expect("file").path())
                    .collect();
                assert_eq!(
                    remaining,
                    vec![destination],
                    "invalid response left a temporary file"
                );
            }
        }
    }

    #[tokio::test]
    async fn incomplete_or_mismatched_media_containers_are_rejected() {
        let mut truncated_mp3 = mp3_fixture();
        truncated_mp3.truncate(100);
        let mut mp3_after_first_frame = mp3_fixture();
        mp3_after_first_frame.truncate(418);
        let mut bad_iso = iso_fixture();
        bad_iso[0..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let mut iso_with_truncated_tail = iso_fixture();
        iso_with_truncated_tail.extend_from_slice(b"moof\0\0\0");
        let mut empty_wave = wave_fixture();
        empty_wave.truncate(44);
        let opus = opus_fixture();
        let first_page_end = 27
            + usize::from(opus[26])
            + opus[27..27 + usize::from(opus[26])]
                .iter()
                .map(|byte| usize::from(*byte))
                .sum::<usize>();
        for (ext, bytes) in [
            ("mp3", b"ID3\x04\0\0\0\0\0\0".to_vec()),
            ("mp3", truncated_mp3),
            ("mp3", mp3_after_first_frame),
            ("m4a", bad_iso),
            ("m4a", iso_with_truncated_tail),
            (
                "m4a",
                include_bytes!("../../tests/fixtures/download/black.mp4").to_vec(),
            ),
            ("mp4", iso_fixture()),
            ("wav", empty_wave),
            ("opus", b"OggS".to_vec()),
            ("opus", opus[..first_page_end - 1].to_vec()),
            ("opus", opus[..first_page_end + 1].to_vec()),
            ("opus", opus[..first_page_end + 27].to_vec()),
            ("mp3", wave_fixture()),
        ] {
            let dir = tempfile::tempdir().expect("output directory");
            let url = audio_server(&bytes).await;
            download_clip_url(
                &clip(),
                &dir.path().to_string_lossy(),
                &url,
                ext,
                false,
                true,
            )
            .await
            .expect_err("incomplete or mismatched container must fail");
            assert_eq!(
                std::fs::read_dir(dir.path()).expect("output files").count(),
                0
            );
        }
    }

    #[test]
    fn opus_rejects_payload_corruption_detected_by_page_crc() {
        let mut opus = opus_fixture();
        let tags_end = ogg_page_end(&opus, ogg_page_end(&opus, 0));
        let payload = tags_end + 27 + usize::from(opus[tags_end + 26]);
        opus[payload] ^= 1;
        assert_media_fixture(&opus, "opus", false, "audio payload CRC mismatch");
    }

    #[test]
    fn production_idle_timeout_is_not_reduced_for_the_test_build() {
        assert_eq!(DOWNLOAD_IDLE_TIMEOUT, std::time::Duration::from_secs(60));
    }

    #[test]
    fn quiet_download_uses_a_hidden_progress_bar() {
        assert!(download_progress_bar(1024, true).is_hidden());
    }

    #[test]
    fn download_filename_bounds_long_unicode_titles() {
        let mut long_title_clip = clip();
        long_title_clip.title = "界".repeat(100);

        let filename = download_filename(&long_title_clip, "mp3");

        assert!(
            filename.len() <= 240,
            "filename was {} bytes",
            filename.len()
        );
        assert!(filename.ends_with("-clip-a.mp3"));
    }

    #[tokio::test]
    async fn download_preflight_rejects_an_output_directory_that_is_a_file() {
        let dir = test_dir("download-preflight-output-file");
        std::fs::create_dir_all(&dir).expect("create test directory");
        let output_file = dir.join("not-a-directory");
        std::fs::write(&output_file, b"file").expect("write output path fixture");

        let error = preflight_clip_download(&clip(), &output_file.to_string_lossy(), "mp3", false)
            .await
            .expect_err("a regular file cannot be used as an output directory");

        assert!(
            matches!(error, CliError::Download(message) if message.contains("not a directory"))
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn download_preflight_rejects_a_non_writable_output_directory() {
        use std::os::unix::fs::PermissionsExt;

        let dir = test_dir("download-preflight-read-only");
        std::fs::create_dir_all(&dir).expect("create output directory");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))
            .expect("make output directory read-only");

        let result = preflight_clip_download(&clip(), &dir.to_string_lossy(), "mp3", false).await;

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
            .expect("restore output directory permissions");
        let error = result.expect_err("read-only output directory must fail preflight");
        assert!(matches!(error, CliError::Download(message) if message.contains("not writable")));
        let _ = std::fs::remove_dir_all(dir);
    }

    async fn audio_server(body: &[u8]) -> String {
        media_server(body, "application/octet-stream").await
    }

    async fn media_server(body: &[u8], content_type: &str) -> String {
        let body = body.to_vec();
        let content_type = content_type.to_string();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind audio server");
        let address = listener.local_addr().expect("audio server address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write headers");
            stream.write_all(&body).await.expect("write body");
        });
        format!("http://{address}/track.mp3")
    }

    async fn truncated_audio_server(body: &[u8]) -> String {
        let body = body.to_vec();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind truncated audio server");
        let address = listener
            .local_addr()
            .expect("truncated audio server address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len() + 1
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write headers");
            stream.write_all(&body).await.expect("write truncated body");
        });
        format!("http://{address}/track.mp3")
    }

    async fn stalled_audio_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stalled audio server");
        let address = listener.local_addr().expect("stalled audio server address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\n")
                .await
                .expect("write headers");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let _ = stream.write_all(b"audio").await;
        });
        format!("http://{address}/track.mp3")
    }

    async fn header_stalled_audio_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind header-stalled audio server");
        let address = listener
            .local_addr()
            .expect("header-stalled audio server address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });
        format!("http://{address}/track.mp3")
    }

    #[tokio::test]
    async fn download_creates_missing_output_directory() {
        let dir = test_dir("download-creates-dir");
        let output_dir = dir.join("nested").join("songs");
        let url = audio_server(&mp3_fixture()).await;

        let path = download_clip_url(
            &clip(),
            &output_dir.to_string_lossy(),
            &url,
            "mp3",
            false,
            true,
        )
        .await
        .expect("download into a new output directory");

        assert_eq!(
            std::fs::read(path).expect("downloaded audio"),
            mp3_fixture()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn download_preserves_existing_file_without_force() {
        let dir = test_dir("download-preserves-existing");
        std::fs::create_dir_all(&dir).expect("create output directory");
        let destination = dir.join("track-clip-a.mp3");
        std::fs::write(&destination, b"original").expect("write existing file");
        let url = audio_server(&mp3_fixture()).await;

        let error = download_clip_url(&clip(), &dir.to_string_lossy(), &url, "mp3", false, true)
            .await
            .expect_err("existing output must not be overwritten by default");

        assert!(matches!(error, CliError::Download(message) if message.contains("already exists")));
        assert_eq!(
            std::fs::read(&destination).expect("existing file"),
            b"original"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn download_replaces_existing_file_only_when_forced() {
        let dir = test_dir("download-force-replaces");
        std::fs::create_dir_all(&dir).expect("create output directory");
        let destination = dir.join("track-clip-a.mp3");
        std::fs::write(&destination, b"original").expect("write existing file");
        let url = audio_server(&mp3_fixture()).await;

        let path = download_clip_url(&clip(), &dir.to_string_lossy(), &url, "mp3", true, true)
            .await
            .expect("force should replace existing output");

        assert_eq!(
            std::fs::read(path).expect("replacement file"),
            mp3_fixture()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn failed_postprocessing_preserves_a_forced_destination() {
        let dir = test_dir("download-force-postprocess-failure");
        std::fs::create_dir_all(&dir).expect("create output directory");
        let destination = dir.join("track-clip-a.mp3");
        std::fs::write(&destination, b"original").expect("write existing file");
        let url = audio_server(&mp3_fixture()).await;
        let staged = stage_clip_url(&clip(), &dir.to_string_lossy(), &url, "mp3", true, true)
            .await
            .expect("stage forced replacement");
        let temporary = staged.path().to_path_buf();

        let error = staged
            .commit_after(|_| Err(CliError::Download("post-processing failed".into())))
            .expect_err("post-processing must fail before commit");

        assert!(
            matches!(error, CliError::Download(message) if message.contains("post-processing"))
        );
        assert_eq!(
            std::fs::read(&destination).expect("original destination"),
            b"original"
        );
        assert!(!temporary.exists(), "failed staging file must be removed");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn successful_postprocessing_commits_a_forced_destination() {
        let dir = test_dir("download-force-postprocess-success");
        std::fs::create_dir_all(&dir).expect("create output directory");
        let destination = dir.join("track-clip-a.mp3");
        std::fs::write(&destination, b"original").expect("write existing file");
        let url = audio_server(&mp3_fixture()).await;
        let staged = stage_clip_url(&clip(), &dir.to_string_lossy(), &url, "mp3", true, true)
            .await
            .expect("stage forced replacement");

        let path = staged
            .commit_after(|temporary_path| {
                std::fs::write(temporary_path, b"processed")?;
                Ok(())
            })
            .expect("commit processed replacement");

        assert_eq!(std::fs::read(path).expect("processed file"), b"processed");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn forced_download_refuses_a_directory_at_the_output_path() {
        let dir = test_dir("download-force-refuses-directory");
        std::fs::create_dir_all(dir.join("track-clip-a.mp3")).expect("create output directory");
        let url = audio_server(&mp3_fixture()).await;

        let error = download_clip_url(&clip(), &dir.to_string_lossy(), &url, "mp3", true, true)
            .await
            .expect_err("a directory must not be moved aside as a forced download target");

        assert!(matches!(error, CliError::Download(message) if message.contains("not a file")));
        assert!(dir.join("track-clip-a.mp3").is_dir());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn interrupted_download_removes_partial_file() {
        let dir = test_dir("download-cleans-partial");
        let url = truncated_audio_server(&mp3_fixture()).await;

        let error = download_clip_url(&clip(), &dir.to_string_lossy(), &url, "mp3", false, true)
            .await
            .expect_err("truncated response must fail");

        assert!(matches!(error, CliError::Http(_)));
        let files = std::fs::read_dir(&dir)
            .expect("output directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read output directory");
        assert!(files.is_empty(), "partial file must be cleaned up");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn stalled_download_times_out_and_removes_partial_file() {
        let dir = test_dir("download-times-out-when-stalled");
        let url = stalled_audio_server().await;

        let error = stage_clip_url_with_idle_timeout(
            &clip(),
            &dir.to_string_lossy(),
            &url,
            "mp3",
            false,
            true,
            std::time::Duration::from_millis(10),
        )
        .await
        .expect_err("a stalled body must not wait forever");

        assert!(matches!(error, CliError::Download(message) if message.contains("stalled")));
        let files = std::fs::read_dir(&dir)
            .expect("output directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read output directory");
        assert!(
            files.is_empty(),
            "stalled download must clean temporary files"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn download_rejects_a_response_larger_than_the_safety_limit() {
        let dir = test_dir("download-size-limit");
        let url = audio_server(&mp3_fixture()).await;

        let error = stage_clip_url_with_limits(
            &clip(),
            &dir.to_string_lossy(),
            &url,
            "mp3",
            false,
            true,
            std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(1),
            4,
        )
        .await
        .expect_err("content length above the limit must be rejected");

        assert!(matches!(error, CliError::Download(message) if message.contains("safety limit")));
        let files = std::fs::read_dir(&dir)
            .expect("output directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read output directory");
        assert!(files.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn download_total_deadline_removes_the_staging_file() {
        let dir = test_dir("download-total-timeout");
        let url = stalled_audio_server().await;

        let error = stage_clip_url_with_limits(
            &clip(),
            &dir.to_string_lossy(),
            &url,
            "mp3",
            false,
            true,
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
            1024,
        )
        .await
        .expect_err("the total deadline must bound a trickle download");

        assert!(matches!(error, CliError::Download(message) if message.contains("total deadline")));
        let files = std::fs::read_dir(&dir)
            .expect("output directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read output directory");
        assert!(files.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn stalled_response_headers_respect_the_idle_timeout() {
        let dir = test_dir("download-header-timeout");
        let url = header_stalled_audio_server().await;

        let error = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            stage_clip_url_with_idle_timeout(
                &clip(),
                &dir.to_string_lossy(),
                &url,
                "mp3",
                false,
                true,
                std::time::Duration::from_millis(10),
            ),
        )
        .await
        .expect("response headers must use the configured idle timeout")
        .expect_err("stalled response headers must fail");

        assert!(matches!(error, CliError::Download(message) if message.contains("stalled")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cancelling_a_body_download_removes_the_staging_file() {
        let dir = test_dir("download-cancel-cleans-staging");
        let url = stalled_audio_server().await;

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(10),
            stage_clip_url_with_idle_timeout(
                &clip(),
                &dir.to_string_lossy(),
                &url,
                "mp3",
                false,
                true,
                std::time::Duration::from_secs(1),
            ),
        )
        .await;

        assert!(result.is_err(), "outer cancellation must win");
        let files = match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .collect::<Result<Vec<_>, _>>()
                .expect("read output directory"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("output directory: {error}"),
        };
        assert!(files.is_empty(), "cancellation must remove staging files");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn staging_does_not_expand_a_legal_destination_filename_past_fs_limits() {
        let dir = test_dir("download-long-title");
        let url = audio_server(&mp3_fixture()).await;
        let mut long_title_clip = clip();
        long_title_clip.title = "a".repeat(202);

        let staged = stage_clip_url(
            &long_title_clip,
            &dir.to_string_lossy(),
            &url,
            "mp3",
            false,
            true,
        )
        .await
        .expect("a legal destination filename must have a legal staging filename");

        assert!(
            staged
                .path()
                .file_name()
                .expect("temporary filename")
                .to_string_lossy()
                .len()
                < 255
        );
        drop(staged);
        let _ = std::fs::remove_dir_all(dir);
    }
}
