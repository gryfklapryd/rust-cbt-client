//! Operasi sinkronisasi dengan server pusat (async). Kunci database tidak pernah
//! dipegang selama menunggu jaringan.

use chrono::Utc;
use rusqlite::params;
use serde::Serialize;
use serde_json::{json, Value};

use super::api::{BatchAck, SyncApi};
use super::error::{AppError, AppResult};
use super::Core;

/// Maksimum attempt per batch (batas server 500).
const BATCH_SIZE: usize = 200;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResult {
    pub schedule_id: String,
    pub package_version: i64,
    pub updated: bool,
    pub assets_downloaded: usize,
    pub assets_total: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    pub attachments_uploaded: usize,
    pub attempts_sent: usize,
    pub batches: Vec<String>,
}

/// Unduh (atau perbarui) paket sebuah jadwal beserta semua medianya.
pub async fn download_schedule(core: &Core, api: &SyncApi, schedule_id: &str) -> AppResult<DownloadResult> {
    let current = core.latest_package(schedule_id)?;
    let etag = current.as_ref().map(|(_, c)| c.clone());
    let (pkg, updated) = match api.package(schedule_id, etag.as_deref()).await? {
        Some(dl) => (core.store_package(&dl.bytes, &dl.checksum)?, true),
        None => (current.expect("304 hanya bila paket lokal ada").0, false),
    };
    let mut downloaded = 0;
    for asset in &pkg.assets {
        if core.asset_present(asset) {
            continue;
        }
        let bytes = api.asset(&asset.id).await?;
        core.save_asset(asset, &bytes)?;
        downloaded += 1;
    }
    Ok(DownloadResult {
        schedule_id: schedule_id.to_string(),
        package_version: pkg.version,
        updated,
        assets_downloaded: downloaded,
        assets_total: pkg.assets.len(),
    })
}

struct PendingAttachment {
    id: String,
    attempt_id: String,
    question_id: String,
    path: String,
    name: String,
    mime: String,
}

/// Kirim semua perubahan yang belum tersinkron: lampiran dulu, lalu hasil ujian.
pub async fn upload_results(core: &Core, api: &SyncApi) -> AppResult<UploadResult> {
    let mut result = UploadResult::default();

    let pending: Vec<PendingAttachment> = {
        let db = core.db();
        let mut stmt = db
            .conn
            .prepare("SELECT id, attempt_id, question_id, path, name, mime FROM attachments WHERE uploaded = 0")?;
        let rows = stmt.query_map([], |r| {
            Ok(PendingAttachment {
                id: r.get(0)?,
                attempt_id: r.get(1)?,
                question_id: r.get(2)?,
                path: r.get(3)?,
                name: r.get(4)?,
                mime: r.get(5)?,
            })
        })?;
        rows.collect::<Result<_, _>>()?
    };
    for att in pending {
        let bytes = std::fs::read(&att.path)?;
        api.upload_attachment(&att.id, &att.attempt_id, &att.question_id, &att.name, &att.mime, bytes)
            .await?;
        core.db()
            .conn
            .execute("UPDATE attachments SET uploaded = 1 WHERE id = ?1", [&att.id])?;
        result.attachments_uploaded += 1;
    }

    let device_id = api_device_id(core)?;
    let ids: Vec<String> = {
        let db = core.db();
        let mut stmt = db
            .conn
            .prepare("SELECT id FROM attempts WHERE sequence > synced_sequence ORDER BY started_at")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for chunk in ids.chunks(BATCH_SIZE) {
        let mut payloads = Vec::new();
        let mut sent: Vec<(String, i64)> = Vec::new();
        for id in chunk {
            let (payload, seq) = core.attempt_upload(id, &device_id)?;
            payloads.push(payload);
            sent.push((id.clone(), seq));
        }
        let batch_id = uuid::Uuid::new_v4().to_string();
        let body = json!({ "batchId": batch_id, "attempts": payloads });
        // Catat sebelum mengirim: bila koneksi putus di tengah, batch yang sama bisa dicek / dikirim ulang.
        core.db().conn.execute(
            "INSERT INTO batches (id, created_at, attempts, status) VALUES (?1, ?2, ?3, 'sending')",
            params![batch_id, Utc::now().to_rfc3339(), serde_json::to_string(&sent)?],
        )?;
        let ack = api.post_results(&body).await?;
        {
            let db = core.db();
            for (id, seq) in &sent {
                db.conn.execute(
                    "UPDATE attempts SET synced_sequence = max(synced_sequence, ?2), sync_error = NULL WHERE id = ?1",
                    params![id, seq],
                )?;
            }
        }
        record_ack(core, &ack)?;
        result.attempts_sent += sent.len();
        result.batches.push(batch_id);
    }
    Ok(result)
}

fn api_device_id(core: &Core) -> AppResult<String> {
    Ok(core.credentials()?.device_id)
}

/// Simpan status batch dan tandai attempt yang ditolak server.
pub fn record_ack(core: &Core, ack: &BatchAck) -> AppResult<()> {
    let db = core.db();
    db.conn.execute(
        "UPDATE batches SET status = ?2, response = ?3 WHERE id = ?1",
        params![ack.batch_id, ack.status, serde_json::to_string(ack)?],
    )?;
    if let Some(outcomes) = &ack.attempts {
        for o in outcomes.iter().filter(|o| !o.accepted) {
            db.conn.execute(
                "UPDATE attempts SET sync_error = ?2 WHERE id = ?1",
                params![o.attempt_id, o.reason.clone().unwrap_or_else(|| "ditolak server".into())],
            )?;
        }
    }
    Ok(())
}

/// Perbarui status batch yang belum selesai diproses server.
pub async fn refresh_batches(core: &Core, api: &SyncApi) -> AppResult<Vec<BatchAck>> {
    let open: Vec<String> = {
        let db = core.db();
        let mut stmt = db
            .conn
            .prepare("SELECT id FROM batches WHERE status IN ('sending', 'received', 'processing') ORDER BY created_at")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut out = Vec::new();
    for id in open {
        match api.batch_status(&id).await {
            Ok(ack) => {
                record_ack(core, &ack)?;
                out.push(ack);
            }
            // Batch tercatat lokal tapi tidak pernah sampai ke server: tandai agar tidak dicek terus.
            Err(AppError::Server { status: 404, .. }) => {
                core.db()
                    .conn
                    .execute("UPDATE batches SET status = 'lost' WHERE id = ?1", [&id])?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRow {
    pub id: String,
    pub created_at: String,
    pub status: String,
    pub attempt_count: usize,
    pub response: Option<Value>,
}

pub fn recent_batches(core: &Core, limit: i64) -> AppResult<Vec<BatchRow>> {
    let db = core.db();
    let mut stmt = db
        .conn
        .prepare("SELECT id, created_at, status, attempts, response FROM batches ORDER BY created_at DESC LIMIT ?1")?;
    let rows = stmt.query_map([limit], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, created_at, status, attempts, response) = row?;
        let count = serde_json::from_str::<Vec<Value>>(&attempts).map(|v| v.len()).unwrap_or(0);
        out.push(BatchRow {
            id,
            created_at,
            status,
            attempt_count: count,
            response: response.map(|s| serde_json::from_str(&s)).transpose()?,
        });
    }
    Ok(out)
}
