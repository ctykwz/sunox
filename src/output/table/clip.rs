use crate::api::types::{Clip, ClipInfo, ClipInfoSupplementalError};

use super::{base_table, dynamic_table};

pub fn clips(clips: &[Clip]) {
    let mut table = dynamic_table();
    table.set_header(vec!["ID", "Title", "Status", "Model", "Duration", "Tags"]);

    for clip in clips {
        let duration = clip
            .metadata
            .duration
            .map(|duration| format!("{duration:.0}s"))
            .unwrap_or_default();
        let tags = clip.metadata.tags.as_deref().unwrap_or("-");
        let short_id = if clip.id.len() > 8 {
            &clip.id[..8]
        } else {
            &clip.id
        };
        table.add_row(vec![
            short_id,
            &clip.title,
            &clip.status,
            &clip.model_name,
            &duration,
            tags,
        ]);
    }
    println!("{table}");
}

pub fn clip_detail(info: &ClipInfo) {
    let clip = &info.clip;
    let mut table = base_table();
    table.set_header(vec!["Field", "Value"]);

    table.add_row(vec!["ID", &clip.id]);
    table.add_row(vec!["Title", &clip.title]);
    table.add_row(vec!["Status", &clip.status]);
    table.add_row(vec!["Model", &clip.model_name]);
    table.add_row(vec!["Created", &clip.created_at]);
    table.add_row(vec![
        "Duration",
        &clip
            .metadata
            .duration
            .map(|duration| format!("{duration:.1}s"))
            .unwrap_or_else(|| "-".into()),
    ]);
    table.add_row(vec!["Tags", clip.metadata.tags.as_deref().unwrap_or("-")]);
    table.add_row(vec![
        "BPM",
        &clip
            .metadata
            .avg_bpm
            .map(|bpm| format!("{bpm:.0}"))
            .unwrap_or_else(|| "-".into()),
    ]);
    table.add_row(vec!["Plays", &clip.play_count.to_string()]);
    table.add_row(vec!["Upvotes", &clip.upvote_count.to_string()]);
    table.add_row(vec![
        "Direct Children",
        &info.direct_children_count.to_string(),
    ]);
    table.add_row(vec![
        "Source Clips",
        &info.attribution.source_clips.len().to_string(),
    ]);
    table.add_row(vec!["Comments", &info.comments.total_count.to_string()]);
    table.add_row(vec![
        "Allow Comments",
        &info.comments.allow_comment.to_string(),
    ]);
    table.add_row(vec!["Similar Clips", &info.similar_clips.len().to_string()]);
    if let Some(summary) = supplemental_errors_summary(&info.supplemental_errors) {
        table.add_row(vec!["Supplemental Errors", &summary]);
    }
    table.add_row(vec!["Has Stems", &clip.metadata.has_stem.to_string()]);
    table.add_row(vec![
        "Instrumental",
        &clip
            .metadata
            .make_instrumental
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string()),
    ]);

    if let Some(ref url) = clip.audio_url {
        table.add_row(vec!["Audio URL", url]);
    }
    if let Some(ref url) = clip.video_url {
        table.add_row(vec!["Video URL", url]);
    }
    if let Some(ref prompt) = clip.metadata.prompt {
        let truncated = truncate_chars(prompt, 200);
        table.add_row(vec!["Lyrics", &truncated]);
    }

    println!("{table}");
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        value.to_string()
    }
}

fn supplemental_errors_summary(errors: &[ClipInfoSupplementalError]) -> Option<String> {
    if errors.is_empty() {
        return None;
    }
    Some(
        errors
            .iter()
            .map(|error| format!("{}({})", error.field, error.code))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{Clip, ClipAttribution, ClipComments};

    #[test]
    fn clip_detail_truncates_multibyte_prompt_without_panicking() {
        let mut clip = Clip {
            id: "clip-a".into(),
            title: "Demo".into(),
            status: "complete".into(),
            model_name: "chirp-fenix".into(),
            audio_url: None,
            video_url: None,
            image_url: None,
            created_at: "2026-07-03T00:00:00Z".into(),
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
        };
        clip.metadata.prompt = Some("广".repeat(80));

        let info = ClipInfo {
            clip,
            attribution: ClipAttribution::default(),
            comments: ClipComments::default(),
            direct_children_count: 0,
            similar_clips: Vec::new(),
            supplemental_errors: Vec::new(),
        };

        clip_detail(&info);
    }

    #[test]
    fn supplemental_errors_summary_names_failed_fields() {
        let errors = vec![
            ClipInfoSupplementalError {
                field: "comments".into(),
                code: "api_error".into(),
                message: "API error: HTTP 500".into(),
            },
            ClipInfoSupplementalError {
                field: "similar_clips".into(),
                code: "json_error".into(),
                message: "schema drift".into(),
            },
        ];

        assert_eq!(
            supplemental_errors_summary(&errors),
            Some("comments(api_error), similar_clips(json_error)".to_string())
        );
    }
}
