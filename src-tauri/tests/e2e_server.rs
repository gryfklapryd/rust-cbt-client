//! Tes end-to-end melawan server pusat sungguhan (repo rust-cbt-master) yang sudah
//! di-seed dengan `db:seed --demo`. Dilewati bila `CBT_E2E_SERVER` tidak diset.
//!
//!   CBT_E2E_SERVER=http://localhost:3000 cargo test --test e2e_server -- --nocapture
//!
//! Variabel opsional: CBT_E2E_SITE (DEMO-01), CBT_E2E_SECRET (demo-secret-ganti-saya),
//! CBT_E2E_ADMIN_USER / CBT_E2E_ADMIN_PASSWORD (admin / admin12345) untuk memeriksa nilai.

use std::time::Duration;

use cbt_client_lib::core::exam::{LoginRequest, SaveAnswerRequest};
use cbt_client_lib::core::{sync, ConfigInput, Core};
use chrono::Utc;
use serde_json::{json, Value};

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Jawaban benar untuk soal contoh (template server) per jenis soal.
fn correct_response(qtype: &str) -> Value {
    match qtype {
        "single_choice" => json!({ "optionId": "A" }),
        "multiple_choice" => json!({ "optionIds": ["A", "C"] }),
        "true_false" => json!({ "value": true }),
        "multiple_true_false" => json!({ "values": { "s1": true, "s2": false, "s3": true } }),
        "short_answer" => json!({ "text": "Soekarno" }),
        "numeric" => json!({ "value": "3,14" }),
        "essay" => json!({ "text": "Tumbuhan memakai air dan CO2 dengan bantuan cahaya." }),
        "matching" => json!({ "pairs": [
            { "leftId": "L1", "rightId": "R1" }, { "leftId": "L2", "rightId": "R2" }, { "leftId": "L3", "rightId": "R3" }
        ] }),
        "ordering" => json!({ "order": ["b", "c", "a"] }),
        "fill_blanks" => json!({ "values": { "b1": "1945", "b2": "o1", "b3": "w1" } }),
        "categorization" => json!({ "mapping": { "i1": "mamalia", "i2": "unggas", "i3": "mamalia", "i4": "unggas" } }),
        "hotspot" => json!({ "points": [{ "x": 0.4, "y": 0.35 }] }),
        "hot_text" => json!({ "segmentIds": ["t2", "t4"] }),
        "matrix" => json!({ "selections": { "r1": ["cair"], "r2": ["padat"], "r3": ["gas"] } }),
        _ => Value::Null,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn full_cycle_against_real_server() {
    let Ok(server) = std::env::var("CBT_E2E_SERVER") else {
        eprintln!("CBT_E2E_SERVER tidak diset, tes e2e dilewati");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    core.save_config(ConfigInput {
        server_url: server.clone(),
        site_code: env("CBT_E2E_SITE", "DEMO-01"),
        secret: Some(env("CBT_E2E_SECRET", "demo-secret-ganti-saya")),
        operator_pin: Some("1234".into()),
        device_name: Some("E2E".into()),
        auto_sync: Some(false),
    })
    .unwrap();
    let api = cbt_client_lib::core::api::SyncApi::new(core.credentials().unwrap()).unwrap();

    // 1. Jadwal & paket
    api.auth().await.expect("auth lokasi");
    let schedules = api.schedules().await.unwrap();
    let sched = schedules
        .iter()
        .find(|s| s.package.is_some() && s.name == "Sesi Demo")
        .expect("jadwal demo dengan paket siap");
    let dl = sync::download_schedule(&core, &api, &sched.id).await.unwrap();
    assert!(dl.updated);
    assert_eq!(dl.assets_downloaded, dl.assets_total);
    // Unduh ulang: 304, tidak ada yang diunduh lagi.
    let again = sync::download_schedule(&core, &api, &sched.id).await.unwrap();
    assert!(!again.updated);
    assert_eq!(again.assets_downloaded, 0);

    // 2. Ujian: peserta yang belum pernah ujian di server
    let (pkg, _) = core.latest_package(&sched.id).unwrap().unwrap();
    let number = env("CBT_E2E_PARTICIPANT", "DEMO-0030");
    let now = Utc::now();
    let attempt_id = core
        .login(
            &LoginRequest {
                schedule_id: sched.id.clone(),
                number: number.clone(),
                password: "123456".into(),
                token: Some("DEMO01".into()),
            },
            now,
        )
        .expect("login peserta");
    let session = core.session(&attempt_id, now).unwrap();
    for q in session.questions.values() {
        let mut response = correct_response(&q.qtype);
        if q.qtype == "file_upload" {
            let att = core
                .save_attachment(&attempt_id, &q.id, "kerja.png", "image/png", b"\x89PNG-demo", now)
                .unwrap();
            response =
                json!({ "files": [{ "attachmentId": att.attachment_id, "name": att.name, "size": att.size, "mime": att.mime }] });
        }
        core.save_answer(
            &SaveAnswerRequest {
                attempt_id: attempt_id.clone(),
                question_id: q.id.clone(),
                response,
                flagged: false,
                time_spent_delta: 10,
                current_index: None,
            },
            now,
        )
        .unwrap();
    }
    core.log_event(&attempt_id, "violation", Some(json!({ "reason": "focus_lost" })), now)
        .unwrap();
    core.submit(&attempt_id, true, now).unwrap();

    // 3. Kirim hasil & tunggu diproses server
    let up = sync::upload_results(&core, &api).await.unwrap();
    assert_eq!(up.attachments_uploaded, 1);
    assert_eq!(up.attempts_sent, 1);
    assert_eq!(core.attempt_counts(None).unwrap().unsynced, 0);
    let mut processed = false;
    for _ in 0..30 {
        let acks = sync::refresh_batches(&core, &api).await.unwrap();
        if let Some(ack) = acks.iter().find(|a| a.status == "processed") {
            let outcome = &ack.attempts.as_ref().unwrap()[0];
            assert!(outcome.accepted, "attempt ditolak: {:?}", outcome.reason);
            processed = true;
            break;
        }
        if acks.is_empty() {
            processed = true; // sudah diproses pada pemanggilan sebelumnya
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(processed, "batch tidak kunjung diproses (worker server berjalan?)");

    // 4. Periksa nilai di server (butuh akun admin)
    let http = reqwest::Client::new();
    let login: Value = http
        .post(format!("{server}/api/auth/login"))
        .json(&json!({ "username": env("CBT_E2E_ADMIN_USER", "admin"), "password": env("CBT_E2E_ADMIN_PASSWORD", "admin12345") }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let Some(token) = login["token"].as_str() else {
        eprintln!("login admin gagal, pemeriksaan nilai dilewati: {login}");
        return;
    };
    let mut attempt = Value::Null;
    for _ in 0..30 {
        attempt = http
            .get(format!("{server}/api/results/attempts/{attempt_id}"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if attempt["gradingStatus"] == "partial" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    eprintln!(
        "hasil di server: status={} nilai={} skor={}/{}",
        attempt["status"], attempt["gradingStatus"], attempt["score"], attempt["maxScore"]
    );
    assert_eq!(attempt["status"], "submitted");
    assert_eq!(attempt["violationCount"], 1);
    // 13 soal otomatis benar; uraian & unggah berkas menunggu koreksi manual.
    assert_eq!(attempt["gradingStatus"], "partial");
    assert_eq!(attempt["score"].as_f64(), Some(13.0));
    assert_eq!(attempt["maxScore"].as_f64(), Some(pkg.questions.len() as f64));
}
