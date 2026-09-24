//! Tipe paket ujian. Cermin dari `ExamPackage` di `packages/shared/src/sync.ts`
//! (sumber kebenaran ada di repo server `rust-cbt-master`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SUPPORTED_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExamPackage {
    pub format_version: u32,
    pub package_id: String,
    pub version: i64,
    pub built_at: DateTime<Utc>,
    pub site: SiteInfo,
    pub schedule: ScheduleInfo,
    pub exam: ExamInfo,
    pub questions: Vec<PackageQuestion>,
    pub stimuli: Vec<Stimulus>,
    pub participants: Vec<Participant>,
    pub assets: Vec<AssetInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteInfo {
    pub id: String,
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInfo {
    pub id: String,
    pub name: String,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub late_entry_minutes: Option<i64>,
    pub access_token_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExamInfo {
    pub id: String,
    pub code: String,
    pub title: String,
    pub description: Option<String>,
    pub instructions: Option<String>,
    pub duration_minutes: i64,
    pub settings: ExamSettings,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExamSettings {
    pub shuffle_questions: bool,
    pub shuffle_options: bool,
    pub allow_back_navigation: bool,
    pub allow_flagging: bool,
    pub auto_submit_on_timeout: bool,
    pub min_time_before_submit_minutes: i64,
    pub show_score_to_participant: bool,
    pub lockdown: bool,
    pub max_violations: i64,
    pub passing_score: Option<f64>,
}

impl Default for ExamSettings {
    fn default() -> Self {
        Self {
            shuffle_questions: false,
            shuffle_options: true,
            allow_back_navigation: true,
            allow_flagging: true,
            auto_submit_on_timeout: true,
            min_time_before_submit_minutes: 0,
            show_score_to_participant: false,
            lockdown: true,
            max_violations: 0,
            passing_score: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub id: String,
    pub title: String,
    pub instructions: Option<String>,
    pub order: i64,
    pub duration_minutes: Option<i64>,
    pub pick_count: Option<i64>,
    pub shuffle_questions: bool,
    pub question_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageQuestion {
    pub id: String,
    #[serde(rename = "type")]
    pub qtype: String,
    pub content: Value,
    pub points: f64,
    pub stimulus_id: Option<String>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stimulus {
    pub id: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub settings: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    pub id: String,
    pub number: String,
    pub name: String,
    pub group_name: Option<String>,
    pub gender: Option<String>,
    pub birth_date: Option<String>,
    pub password_hash: Option<String>,
    pub photo_asset_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetInfo {
    pub id: String,
    pub filename: String,
    pub mime: String,
    pub size: i64,
    pub sha256: String,
}

impl ExamPackage {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let pkg: ExamPackage = serde_json::from_slice(bytes).map_err(|e| format!("paket tidak valid: {e}"))?;
        if pkg.format_version != SUPPORTED_FORMAT_VERSION {
            return Err(format!(
                "format paket v{} belum didukung aplikasi ini (dukung v{}). Perbarui aplikasi.",
                pkg.format_version, SUPPORTED_FORMAT_VERSION
            ));
        }
        Ok(pkg)
    }

    pub fn participant_by_number(&self, number: &str) -> Option<&Participant> {
        let n = number.trim();
        self.participants.iter().find(|p| p.number.eq_ignore_ascii_case(n))
    }

    pub fn question(&self, id: &str) -> Option<&PackageQuestion> {
        self.questions.iter().find(|q| q.id == id)
    }
}
