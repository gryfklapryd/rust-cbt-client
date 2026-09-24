//! Tes server lokal + PC peserta lewat HTTP sungguhan (API LAN di port acak), memakai
//! paket asli dari server pusat (tests/fixtures/demo-package.json).

use std::sync::Arc;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use cbt_client_lib::core::api::ProctorAccount;
use cbt_client_lib::core::crypto::sha256_hex;
use cbt_client_lib::core::exam::{LoginRequest, SaveAnswerRequest};
use cbt_client_lib::core::participant::ParticipantLink;
use cbt_client_lib::core::proctor::{Proctor, DEVICE_APPROVED};
use cbt_client_lib::core::{lan, Core, ParticipantConfigInput, ServerConfigInput};
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};

const FIXTURE: &str = include_str!("fixtures/demo-package.json");

struct Server {
    core: Arc<Core>,
    port: u16,
    task: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

/// Server lokal dengan paket demo yang jadwalnya sedang berlangsung.
async fn server() -> (Server, Value) {
    let dir = tempfile::tempdir().unwrap();
    let core = Arc::new(Core::open(dir.path()).unwrap());
    core.save_server_config(ServerConfigInput {
        server_url: "http://pusat.invalid".into(),
        site_code: "DEMO-01".into(),
        secret: Some("rahasia".into()),
        device_name: Some("SERVER-LAB1".into()),
        auto_sync: Some(false),
        lan_port: None,
    })
    .unwrap();

    let mut pkg: Value = serde_json::from_str(FIXTURE).unwrap();
    let now = Utc::now();
    pkg["schedule"]["startAt"] = json!((now - Duration::minutes(5)).to_rfc3339());
    pkg["schedule"]["endAt"] = json!((now + Duration::hours(3)).to_rfc3339());
    pkg["schedule"]["lateEntryMinutes"] = Value::Null;
    pkg["exam"]["settings"]["minTimeBeforeSubmitMinutes"] = json!(0);
    // Media dummy: isi nol dengan ukuran dan checksum yang cocok.
    for a in pkg["assets"].as_array_mut().unwrap() {
        let size = a["size"].as_u64().unwrap() as usize;
        let bytes = vec![0u8; size];
        a["sha256"] = json!(sha256_hex(&bytes));
        std::fs::write(core.asset_path(a["id"].as_str().unwrap()), &bytes).unwrap();
    }
    let bytes = serde_json::to_vec(&pkg).unwrap();
    core.store_package(&bytes, &sha256_hex(&bytes)).unwrap();

    let hash = Argon2::default()
        .hash_password(b"proktor123", &SaltString::generate(&mut OsRng))
        .unwrap()
        .to_string();
    core.store_proctors(&[ProctorAccount {
        id: uuid::Uuid::new_v4().to_string(),
        username: "proktor".into(),
        name: "Proktor Demo".into(),
        role: "proctor".into(),
        password_hash: hash,
    }])
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(start(core.clone(), listener));
    (
        Server {
            core,
            port,
            task,
            _dir: dir,
        },
        pkg,
    )
}

async fn start(core: Arc<Core>, listener: tokio::net::TcpListener) {
    lan::serve_on(core, listener).await.unwrap();
}

/// PC peserta yang terhubung ke server lokal di `port`.
fn participant(port: u16, name: &str) -> (Arc<Core>, ParticipantLink, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let core = Arc::new(Core::open(dir.path()).unwrap());
    core.save_participant_config(ParticipantConfigInput {
        lan_url: format!("127.0.0.1:{port}"),
        device_name: Some(name.into()),
    })
    .unwrap();
    let link = ParticipantLink::new(core.clone()).unwrap();
    (core, link, dir)
}

fn proctor(core: &Core) -> Proctor {
    core.verify_proctor("PROKTOR", "proktor123").unwrap()
}

async fn approve(server: &Server, pc: &Core, link: &ParticipantLink) {
    let paired = link.pair().await.unwrap();
    assert_eq!(paired.status, "pending");
    let device_id = pc.config().unwrap().device_id;
    server
        .core
        .set_device_status(&device_id, DEVICE_APPROVED, &proctor(&server.core), Utc::now())
        .unwrap();
    assert_eq!(link.pair().await.unwrap().status, DEVICE_APPROVED);
}

fn login_req(pkg: &Value, number: &str) -> LoginRequest {
    LoginRequest {
        schedule_id: pkg["schedule"]["id"].as_str().unwrap().into(),
        number: number.into(),
        password: "123456".into(),
        token: Some("demo01".into()),
    }
}

fn answer(attempt_id: &str, question_id: &str, response: Value) -> SaveAnswerRequest {
    SaveAnswerRequest {
        attempt_id: attempt_id.into(),
        question_id: question_id.into(),
        response,
        flagged: false,
        time_spent_delta: 5,
        current_index: Some(1),
    }
}

fn qid(pkg: &Value, qtype: &str) -> String {
    pkg["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["type"] == qtype)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn server_answer(core: &Core, attempt_id: &str, question_id: &str) -> Option<Value> {
    core.db()
        .conn
        .query_row(
            "SELECT response FROM answers WHERE attempt_id = ?1 AND question_id = ?2",
            [attempt_id, question_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .map(|s| serde_json::from_str(&s).unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_login_answers_and_proctor_actions() {
    let (srv, pkg) = server().await;
    let (pc1, link1, _d1) = participant(srv.port, "LAB1-PC01");

    // Info tanpa token; jadwal butuh PC yang disetujui.
    let info = link1.client.info().await.unwrap();
    assert_eq!(info.site_code.as_deref(), Some("DEMO-01"));
    link1.pair().await.unwrap();
    let err = link1.schedules().await.unwrap_err();
    assert_eq!(err.lan_code(), Some("pending"));
    approve(&srv, &pc1, &link1).await;
    let schedules = link1.schedules().await.unwrap();
    assert_eq!(schedules.len(), 1);
    assert!(schedules[0].ready && schedules[0].requires_token);

    // Login: password/token diverifikasi server lokal, media disalin ke PC peserta.
    let mut bad = login_req(&pkg, "DEMO-0001");
    bad.password = "salah".into();
    assert!(link1.login(&bad).await.unwrap_err().to_string().contains("password salah"));
    let session = link1.login(&login_req(&pkg, "DEMO-0001")).await.unwrap();
    let attempt = session.attempt_id.clone();
    let asset_id = pkg["assets"][0]["id"].as_str().unwrap();
    assert!(pc1.asset_path(asset_id).exists());
    assert_eq!(pc1.asset_mime(asset_id).as_deref(), Some("image/svg+xml"));

    let sc = qid(&pkg, "single_choice");
    let st = link1
        .save_answer(&answer(&attempt, &sc, json!({ "optionId": "A" })))
        .await
        .unwrap();
    assert!(st.connected && st.pending == 0 && st.status == "in_progress");
    assert_eq!(server_answer(&srv.core, &attempt, &sc), Some(json!({ "optionId": "A" })));

    // PC lain tidak bisa memakai attempt yang sama sampai proktor mengizinkan pindah komputer.
    let (pc2, link2, _d2) = participant(srv.port, "LAB1-PC02");
    approve(&srv, &pc2, &link2).await;
    let err = link2.login(&login_req(&pkg, "DEMO-0001")).await.unwrap_err();
    assert!(err.to_string().contains("komputer lain"), "{err}");

    // Tambah waktu dari proktor terlihat di PC peserta.
    let before = link1.attempt_state(&attempt).await.unwrap().deadline;
    srv.core.extend_time(&attempt, 10, Utc::now()).unwrap();
    let after = link1.attempt_state(&attempt).await.unwrap();
    assert_eq!(after.deadline - before, Duration::minutes(10));

    // Pindah komputer: PC lama ditolak, PC baru melanjutkan dengan jawaban yang sama.
    srv.core.release_device(&attempt, Utc::now()).unwrap();
    let moved = link2.login(&login_req(&pkg, "DEMO-0001")).await.unwrap();
    assert_eq!(moved.attempt_id, attempt);
    assert_eq!(moved.answers[&sc].response, json!({ "optionId": "A" }));
    let err = link1
        .save_answer(&answer(&attempt, &sc, json!({ "optionId": "B" })))
        .await
        .unwrap_err();
    assert_eq!(err.lan_code(), Some("other_device"));
    assert_eq!(server_answer(&srv.core, &attempt, &sc), Some(json!({ "optionId": "A" })));
    assert_eq!(pc1.outbox_count(None).unwrap(), 0);

    // Proktor menghentikan ujian: PC peserta melihat status dihentikan.
    srv.core.terminate(&attempt, Some("tes"), Utc::now()).unwrap();
    assert_eq!(link2.attempt_state(&attempt).await.unwrap().status, "terminated");
    let rows = srv.core.monitor(pkg["schedule"]["id"].as_str().unwrap(), Utc::now()).unwrap();
    assert_eq!(rows.len(), 30);
    let row = rows.iter().find(|r| r.number == "DEMO-0001").unwrap();
    assert_eq!(row.status, "terminated");
    assert_eq!(row.device_name.as_deref(), Some("LAB1-PC02"));
    assert_eq!(rows.iter().filter(|r| r.status == "not_started").count(), 29);

    // Buka kunci: peserta lanjut lagi.
    srv.core.unlock(&attempt, Some(5), Utc::now()).unwrap();
    assert_eq!(link2.attempt_state(&attempt).await.unwrap().status, "in_progress");

    // Login proktor dari PC peserta diverifikasi server lokal.
    assert_eq!(
        link2.proctor_verify("proktor", "proktor123").await.unwrap().username,
        "proktor"
    );
    assert!(link2.proctor_verify("proktor", "salah").await.is_err());
    srv.task.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_are_queued_while_server_is_unreachable() {
    let (srv, pkg) = server().await;
    let (pc, link, _d) = participant(srv.port, "LAB1-PC03");
    approve(&srv, &pc, &link).await;
    let session = link.login(&login_req(&pkg, "DEMO-0002")).await.unwrap();
    let attempt = session.attempt_id.clone();
    let sc = qid(&pkg, "single_choice");
    let tf = qid(&pkg, "true_false");
    let fu = qid(&pkg, "file_upload");

    // Kabel dicabut: server lokal berhenti.
    srv.task.abort();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let st = link
        .save_answer(&answer(&attempt, &sc, json!({ "optionId": "C" })))
        .await
        .unwrap();
    assert!(!st.connected);
    assert_eq!(st.pending, 1);
    assert_eq!(st.status, "in_progress");
    let st = link
        .save_answer(&answer(&attempt, &tf, json!({ "value": true })))
        .await
        .unwrap();
    assert_eq!(st.pending, 2);
    let saved = link
        .save_attachment(&attempt, &fu, "kerja.png", "image/png", b"\x89PNG-offline")
        .await
        .unwrap();
    // Sesi tetap bisa dibuka dari salinan di PC, dengan jawaban terbaru.
    let offline = link.session(&attempt).await.unwrap();
    assert_eq!(offline.answers[&sc].response, json!({ "optionId": "C" }));
    assert!(offline.remaining_seconds > 0);
    let st = link.submit(&attempt, true).await.unwrap();
    assert_eq!(st.status, "submitted");
    assert_eq!(st.pending, 4);

    // Server hidup lagi di port yang sama: antrean terkirim berurutan.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", srv.port)).await.unwrap();
    let task = tokio::spawn(start(srv.core.clone(), listener));
    let report = link.flush().await;
    assert!(report.blocked.is_none());
    assert_eq!(report.sent, 4);
    assert_eq!(pc.outbox_count(None).unwrap(), 0);
    assert_eq!(server_answer(&srv.core, &attempt, &sc), Some(json!({ "optionId": "C" })));
    assert_eq!(server_answer(&srv.core, &attempt, &tf), Some(json!({ "value": true })));
    let (upload, _) = srv.core.attempt_upload(&attempt, "server").unwrap();
    assert_eq!(upload["status"], "submitted");
    assert!(upload["client"]["deviceId"].as_str().unwrap().starts_with("LAB1-PC03"));
    // Lampiran tersimpan di server lokal dengan ID dari PC peserta.
    let n: i64 = srv
        .core
        .db()
        .conn
        .query_row(
            "SELECT count(*) FROM attachments WHERE id = ?1",
            [&saved.attachment_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    task.abort();
}

#[test]
fn queued_answers_after_timeout_are_accepted_until_deadline() {
    let t0: DateTime<Utc> = Utc::now();
    assert_eq!(
        lan::effective_time(Some(t0 - Duration::minutes(3)), t0),
        t0 - Duration::minutes(3)
    );
    assert_eq!(lan::effective_time(Some(t0 + Duration::minutes(3)), t0), t0);
    assert_eq!(lan::effective_time(Some(t0 - Duration::hours(13)), t0), t0);
    assert_eq!(lan::effective_time(None, t0), t0);
}
