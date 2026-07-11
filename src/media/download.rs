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

pub async fn download_clip(
    clip: &Clip,
    output_dir: &str,
    video: bool,
    force: bool,
    quiet: bool,
) -> Result<String, CliError> {
    let url = if video {
        clip.video_url
            .as_deref()
            .ok_or_else(|| CliError::Download("no video URL available".into()))?
    } else {
        clip.audio_url
            .as_deref()
            .ok_or_else(|| CliError::Download("no audio URL available".into()))?
    };

    let ext = if video { "mp4" } else { "mp3" };
    download_clip_url(clip, output_dir, url, ext, force, quiet).await
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

async fn stage_clip_url_with_idle_timeout(
    clip: &Clip,
    output_dir: &str,
    url: &str,
    ext: &str,
    force: bool,
    quiet: bool,
    idle_timeout: Duration,
) -> Result<StagedDownload, CliError> {
    let filename = download_filename(clip, ext);
    let output_dir = Path::new(output_dir);
    tokio::fs::create_dir_all(output_dir).await?;
    let path = output_dir.join(&filename);
    reject_existing_output(&path, force).await?;

    let resp = tokio::time::timeout(idle_timeout, http::download_client()?.get(url).send())
        .await
        .map_err(|_| CliError::Download(format!("download stalled before response: {filename}")))?
        .map_err(CliError::Http)?
        .error_for_status()
        .map_err(CliError::Http)?;

    let total = resp.content_length().unwrap_or(0);
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
    let result = async {
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = tokio::time::timeout(idle_timeout, stream.next())
            .await
            .map_err(|_| CliError::Download(format!("download stalled: {filename}")))?
        {
            let chunk = chunk.map_err(CliError::Http)?;
            pb.inc(chunk.len() as u64);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok::<(), CliError>(())
    }
    .await;
    drop(file);

    if let Err(error) = result {
        pb.abandon_with_message("failed");
        return Err(error);
    }

    pb.finish_with_message("downloaded");
    Ok(StagedDownload {
        temp_path: Some(temp_path),
        destination_path: path,
        force,
    })
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
        stage_clip_url, stage_clip_url_with_idle_timeout,
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
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
        }
    }

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sunox-{name}-{}", uuid::Uuid::new_v4()))
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

    async fn audio_server(body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind audio server");
        let address = listener.local_addr().expect("audio server address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write headers");
            stream.write_all(body).await.expect("write body");
        });
        format!("http://{address}/track.mp3")
    }

    async fn truncated_audio_server(body: &'static [u8]) -> String {
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
            stream.write_all(body).await.expect("write truncated body");
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
        let url = audio_server(b"audio").await;

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

        assert_eq!(std::fs::read(path).expect("downloaded audio"), b"audio");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn download_preserves_existing_file_without_force() {
        let dir = test_dir("download-preserves-existing");
        std::fs::create_dir_all(&dir).expect("create output directory");
        let destination = dir.join("track-clip-a.mp3");
        std::fs::write(&destination, b"original").expect("write existing file");
        let url = audio_server(b"replacement").await;

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
        let url = audio_server(b"replacement").await;

        let path = download_clip_url(&clip(), &dir.to_string_lossy(), &url, "mp3", true, true)
            .await
            .expect("force should replace existing output");

        assert_eq!(
            std::fs::read(path).expect("replacement file"),
            b"replacement"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn failed_postprocessing_preserves_a_forced_destination() {
        let dir = test_dir("download-force-postprocess-failure");
        std::fs::create_dir_all(&dir).expect("create output directory");
        let destination = dir.join("track-clip-a.mp3");
        std::fs::write(&destination, b"original").expect("write existing file");
        let url = audio_server(b"replacement").await;
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
        let url = audio_server(b"replacement").await;
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
        let url = audio_server(b"replacement").await;

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
        let url = truncated_audio_server(b"audio").await;

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
        let url = audio_server(b"audio").await;
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
