//! Loudness-normalized copies of the uploaded songs.
//!
//! After an upload, a background job runs ffmpeg's `loudnorm` filter (EBU
//! R128, two passes) and writes an mp3 at [`TARGET_LUFS`] next to the
//! original. The music players use the copy by default and can fall back to
//! the original. ffmpeg must be installed on the server (see the README); the
//! `FFMPEG` environment variable can point to another binary.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use serde::{Deserialize, Serialize};
use tokio::{process::Command, sync::Semaphore};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::reloadable_sqlite::ReloadableSqlite;

/// Integrated loudness of the copies.
pub const TARGET_LUFS: f64 = -14.0;
/// Maximum true peak of the copies.
const TARGET_TRUE_PEAK: f64 = -1.0;
const TARGET_LRA: f64 = 11.0;

/// ffmpeg is CPU-heavy; one job at a time keeps the server responsive.
static JOBS: Semaphore = Semaphore::const_new(1);

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema, sqlx::Type,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum NormalizationStatus {
    Pending,
    Done,
    Failed,
}

/// The file name of a new normalized copy. It is new for every upload, so a
/// job for an older upload can't overwrite the copy of a newer one.
pub fn normalized_file_name(act_id: Uuid) -> String {
    format!("{act_id}.{}.normalized.mp3", Uuid::now_v7())
}

/// Normalizes `source` into `target` in the background and records the
/// result for `act_id`, unless the act got another upload meanwhile.
pub fn spawn(
    db: ReloadableSqlite,
    data_path: PathBuf,
    act_id: Uuid,
    source: String,
    target: String,
) {
    tokio::spawn(async move {
        let _permit = JOBS.acquire().await.expect("semaphore is never closed");
        let target_path = data_path.join(&target);
        let started = std::time::Instant::now();
        let result = normalize(&data_path.join(&source), &target_path).await;
        crate::metrics::metrics()
            .normalization_finished(result.is_ok(), started.elapsed().as_secs_f64());
        let status = match &result {
            Ok(()) => NormalizationStatus::Done,
            Err(e) => {
                warn!("Couldn't normalize the song of act {act_id}: {e}");
                NormalizationStatus::Failed
            }
        };
        let pool = db.get().await.clone();
        let updated = sqlx::query!(
            r#"
            UPDATE acts SET normalization_status = ?
            WHERE id = ? AND song_file_normalized = ?
            "#,
            status,
            act_id,
            target,
        )
        .execute(&pool)
        .await;
        match updated {
            Ok(done) if done.rows_affected() == 1 => {
                info!("Normalized the song of act {act_id}: {status:?}");
            }
            Ok(_) => {
                // A newer upload replaced this one meanwhile.
                remove_file(&target_path).await;
            }
            Err(e) => error!("Couldn't save the normalization of act {act_id}: {e}"),
        }
    });
}

/// Starts the jobs that didn't run or didn't finish, e.g. because the server
/// stopped meanwhile or the songs were uploaded before this feature existed.
pub async fn resume(db: &ReloadableSqlite, data_path: &Path) -> sqlx::Result<()> {
    let pool = db.get().await.clone();
    let acts = sqlx::query!(
        r#"
        SELECT id as "id!: Uuid", song_file as "song_file!"
        FROM acts
        WHERE song_file IS NOT NULL
            AND (normalization_status IS NULL OR normalization_status = 'pending')
        "#
    )
    .fetch_all(&pool)
    .await?;
    if !acts.is_empty() {
        info!("Normalizing {} song(s) in the background", acts.len());
    }
    for act in acts {
        let target = normalized_file_name(act.id);
        sqlx::query!(
            "UPDATE acts SET song_file_normalized = ?, normalization_status = 'pending' WHERE id = ?",
            target,
            act.id
        )
        .execute(&pool)
        .await?;
        spawn(
            db.clone(),
            data_path.to_owned(),
            act.id,
            act.song_file,
            target,
        );
    }

    Ok(())
}

/// Deletes a file that may already be gone.
pub async fn remove_file(path: &Path) {
    if let Err(e) = tokio::fs::remove_file(path).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!("Couldn't delete {}: {e}", path.display());
    }
}

/// The measurements of loudnorm's first pass.
#[derive(Debug, Deserialize)]
struct Measurement {
    input_i: String,
    input_tp: String,
    input_lra: String,
    input_thresh: String,
    target_offset: String,
}

fn ffmpeg() -> Command {
    let mut command = Command::new(std::env::var("FFMPEG").unwrap_or_else(|_| "ffmpeg".into()));
    command
        .arg("-hide_banner")
        .arg("-nostdin")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    command
}

/// Runs the two `loudnorm` passes: the first measures, the second applies a
/// linear gain where possible, so the music isn't compressed.
pub async fn normalize(source: &Path, target: &Path) -> Result<(), String> {
    let filter = format!("loudnorm=I={TARGET_LUFS}:TP={TARGET_TRUE_PEAK}:LRA={TARGET_LRA}");
    let output = ffmpeg()
        .arg("-i")
        .arg(source)
        .args([
            "-af",
            &format!("{filter}:print_format=json"),
            "-f",
            "null",
            "-",
        ])
        .output()
        .await
        .map_err(|e| format!("Couldn't run ffmpeg: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!("ffmpeg failed to measure: {}", last_line(&stderr)));
    }
    let json = stderr
        .rfind('{')
        .map(|start| &stderr[start..])
        .and_then(|rest| rest.find('}').map(|end| &rest[..=end]))
        .ok_or("ffmpeg printed no loudness measurement")?;
    let measured: Measurement =
        serde_json::from_str(json).map_err(|e| format!("Unexpected loudnorm output: {e}"))?;

    let output = ffmpeg()
        .arg("-y")
        .arg("-i")
        .arg(source)
        .args([
            "-af",
            &format!(
                "{filter}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true",
                measured.input_i,
                measured.input_tp,
                measured.input_lra,
                measured.input_thresh,
                measured.target_offset,
            ),
            // loudnorm works at 192 kHz; mp3 players expect 44.1 kHz.
            "-ar",
            "44100",
            "-map_metadata",
            "-1",
            "-vn",
            "-codec:a",
            "libmp3lame",
            "-b:a",
            "192k",
            "-f",
            "mp3",
        ])
        .arg(target)
        .output()
        .await
        .map_err(|e| format!("Couldn't run ffmpeg: {e}"))?;
    if !output.status.success() {
        remove_file(target).await;
        return Err(format!(
            "ffmpeg failed to normalize: {}",
            last_line(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    Ok(())
}

fn last_line(output: &str) -> &str {
    output.trim().lines().last().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("normalize-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Measures the integrated loudness of `file` with ffmpeg.
    async fn loudness(file: &Path) -> f64 {
        let output = ffmpeg()
            .arg("-i")
            .arg(file)
            .args(["-af", "ebur128", "-f", "null", "-"])
            .output()
            .await
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let summary = &stderr[stderr.rfind("Integrated loudness:").unwrap()..];
        let line = summary.lines().nth(1).unwrap();
        line.trim()
            .trim_start_matches("I:")
            .trim()
            .trim_end_matches("LUFS")
            .trim()
            .parse()
            .unwrap()
    }

    fn has_ffmpeg() -> bool {
        std::process::Command::new(std::env::var("FFMPEG").unwrap_or_else(|_| "ffmpeg".into()))
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[tokio::test]
    async fn quiet_song_is_raised_to_the_target() {
        if !has_ffmpeg() {
            eprintln!("Skipped: ffmpeg is not installed");
            return;
        }
        let dir = TempDir::new();
        let source = dir.0.join("quiet.mp3");
        // Ten seconds of a quiet sine tone.
        let status = ffmpeg()
            .args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=10",
                "-af",
                "volume=0.05",
            ])
            .arg(&source)
            .status()
            .await
            .unwrap();
        assert!(status.success());
        let target = dir.0.join("normalized.mp3");

        normalize(&source, &target).await.unwrap();

        let before = loudness(&source).await;
        let after = loudness(&target).await;
        assert!(before < TARGET_LUFS - 10.0, "{before}");
        assert!((after - TARGET_LUFS).abs() < 1.5, "{after}");
    }

    #[tokio::test]
    async fn broken_file_fails_without_output() {
        if !has_ffmpeg() {
            eprintln!("Skipped: ffmpeg is not installed");
            return;
        }
        let dir = TempDir::new();
        let source = dir.0.join("broken.mp3");
        std::fs::write(&source, b"not an mp3").unwrap();
        let target = dir.0.join("normalized.mp3");

        assert!(normalize(&source, &target).await.is_err());
        assert!(!target.exists());
    }
}
