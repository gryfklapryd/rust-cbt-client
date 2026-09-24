//! Tes logika ujian memakai paket asli hasil server pusat (tests/fixtures/demo-package.json):
//! 30 peserta (password "123456"), token sesi "DEMO01", 15 soal (satu per jenis), 1 media.

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};

use super::crypto::sha256_hex;
use super::exam::{LoginRequest, SaveAnswerRequest};
use super::test_support::temp_core;
use super::Core;

const FIXTURE: &str = include_str!("../../tests/fixtures/demo-package.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

/// Simpan paket (opsional dimodifikasi) dan media dummy dengan ukuran yang benar.
fn install(core: &Core, mutate: impl FnOnce(&mut Value)) -> Value {
    let mut pkg = fixture();
    mutate(&mut pkg);
    let bytes = serde_json::to_vec(&pkg).unwrap();
    core.store_package(&bytes, &sha256_hex(&bytes)).unwrap();
    for a in pkg["assets"].as_array().unwrap() {
        let size = a["size"].as_u64().unwrap() as usize;
        std::fs::write(core.asset_path(a["id"].as_str().unwrap()), vec![0u8; size]).unwrap();
    }
    pkg
}

fn start_of(pkg: &Value) -> DateTime<Utc> {
    pkg["schedule"]["startAt"].as_str().unwrap().parse().unwrap()
}

fn login_req(pkg: &Value, number: &str) -> LoginRequest {
    LoginRequest {
        schedule_id: pkg["schedule"]["id"].as_str().unwrap().into(),
        number: number.into(),
        password: "123456".into(),
        token: Some("demo01".into()),
    }
}

fn qid_of(pkg: &Value, qtype: &str) -> String {
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

#[test]
fn store_package_verifies_checksum() {
    let (core, _d) = temp_core();
    let bytes = FIXTURE.as_bytes();
    assert!(core.store_package(bytes, "salah").is_err());
    let pkg = core.store_package(bytes, &sha256_hex(bytes)).unwrap();
    assert_eq!(pkg.questions.len(), 15);
    let local = core.local_schedules().unwrap();
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].assets_missing, 1);
    assert!(local[0].requires_token);
}

#[test]
fn login_rules() {
    let (core, _d) = temp_core();
    let pkg = install(&core, |_| {});
    let start = start_of(&pkg);
    let now = start + Duration::minutes(5);

    let early = core.login(&login_req(&pkg, "DEMO-0001"), start - Duration::minutes(1));
    assert!(early.unwrap_err().to_string().contains("belum dimulai"));

    let mut bad = login_req(&pkg, "DEMO-0001");
    bad.password = "000000".into();
    assert!(core.login(&bad, now).is_err());

    let mut no_token = login_req(&pkg, "DEMO-0001");
    no_token.token = None;
    assert!(core.login(&no_token, now).unwrap_err().to_string().contains("Token"));

    let unknown = core.login(&login_req(&pkg, "TIDAK-ADA"), now);
    assert!(unknown.is_err());

    // Nomor tidak peka huruf besar-kecil; login kedua melanjutkan attempt yang sama.
    let a1 = core.login(&login_req(&pkg, "demo-0001"), now).unwrap();
    let a2 = core.login(&login_req(&pkg, "DEMO-0001"), now + Duration::minutes(1)).unwrap();
    assert_eq!(a1, a2);

    let end: DateTime<Utc> = pkg["schedule"]["endAt"].as_str().unwrap().parse().unwrap();
    assert!(core.login(&login_req(&pkg, "DEMO-0002"), end).is_err());
}

#[test]
fn late_entry_and_missing_media() {
    let (core, _d) = temp_core();
    let pkg = install(&core, |p| p["schedule"]["lateEntryMinutes"] = json!(10));
    let start = start_of(&pkg);
    assert!(core
        .login(&login_req(&pkg, "DEMO-0003"), start + Duration::minutes(11))
        .is_err());
    assert!(core
        .login(&login_req(&pkg, "DEMO-0003"), start + Duration::minutes(9))
        .is_ok());

    let asset = pkg["assets"][0]["id"].as_str().unwrap();
    std::fs::remove_file(core.asset_path(asset)).unwrap();
    let err = core.login(&login_req(&pkg, "DEMO-0004"), start).unwrap_err();
    assert!(err.to_string().contains("media"));
}

#[test]
fn session_is_stable_and_shuffles_ordering() {
    let (core, _d) = temp_core();
    let pkg = install(&core, |p| {
        p["exam"]["settings"]["shuffleQuestions"] = json!(true);
    });
    let now = start_of(&pkg) + Duration::minutes(1);
    let id = core.login(&login_req(&pkg, "DEMO-0005"), now).unwrap();
    let s1 = core.session(&id, now).unwrap();
    let s2 = core.session(&id, now + Duration::minutes(3)).unwrap();
    let order = |s: &super::exam::ExamSession| s.sections.iter().flat_map(|x| x.question_ids.clone()).collect::<Vec<_>>();
    assert_eq!(order(&s1), order(&s2));
    assert_eq!(order(&s1).len(), 15);
    assert_eq!(s1.remaining_seconds, 90 * 60);
    assert_eq!(s2.remaining_seconds, 87 * 60);

    // Soal mengurutkan selalu diacak dan tidak sama dengan urutan asli.
    let ordering = qid_of(&pkg, "ordering");
    let items = &s1.option_orders[&ordering]["items"];
    assert_eq!(items.len(), 3);
    assert_ne!(items, &vec!["a".to_string(), "b".into(), "c".into()]);
    // Stimulus soal uraian ikut dikirim ke UI.
    assert_eq!(s1.stimuli.len(), 1);
}

#[test]
fn pick_count_takes_subset() {
    let (core, _d) = temp_core();
    let pkg = install(&core, |p| p["exam"]["sections"][0]["pickCount"] = json!(3));
    let now = start_of(&pkg) + Duration::minutes(1);
    let id = core.login(&login_req(&pkg, "DEMO-0006"), now).unwrap();
    let s = core.session(&id, now).unwrap();
    assert_eq!(s.sections[0].question_ids.len(), 3);
    assert_eq!(s.sections[1].question_ids.len(), 8);
}

#[test]
fn answers_deadline_and_upload_payload() {
    let (core, _d) = temp_core();
    let pkg = install(&core, |_| {});
    let t0 = start_of(&pkg) + Duration::minutes(1);
    let id = core.login(&login_req(&pkg, "DEMO-0007"), t0).unwrap();
    let q = qid_of(&pkg, "single_choice");
    let save = |resp: Value, at: DateTime<Utc>| {
        core.save_answer(
            &SaveAnswerRequest {
                attempt_id: id.clone(),
                question_id: q.clone(),
                response: resp,
                flagged: false,
                time_spent_delta: 20,
                current_index: Some(2),
            },
            at,
        )
    };
    save(json!({ "optionId": "B" }), t0 + Duration::minutes(1)).unwrap();
    save(json!({ "optionId": "A" }), t0 + Duration::minutes(2)).unwrap();
    let s = core.session(&id, t0 + Duration::minutes(2)).unwrap();
    assert_eq!(s.answers[&q].response, json!({ "optionId": "A" }));
    assert_eq!(s.answers[&q].time_spent, 40);
    assert_eq!(s.current_index, 2);

    // Soal di luar ujian ditolak.
    assert!(core
        .save_answer(
            &SaveAnswerRequest {
                attempt_id: id.clone(),
                question_id: "00000000-0000-4000-8000-000000000000".into(),
                response: json!(null),
                flagged: false,
                time_spent_delta: 0,
                current_index: None,
            },
            t0,
        )
        .is_err());

    // Lewat batas waktu: jawaban ditolak dan attempt otomatis `timed_out`.
    let late = t0 + Duration::minutes(91);
    assert!(save(json!({ "optionId": "C" }), late).is_err());
    let s = core.session(&id, late).unwrap();
    assert_eq!(s.status, "timed_out");
    assert_eq!(s.remaining_seconds, 0);

    let (payload, seq) = core.attempt_upload(&id, "PC-1").unwrap();
    assert!(seq >= 4);
    assert_eq!(payload["status"], "timed_out");
    assert_eq!(payload["questionOrder"].as_array().unwrap().len(), 15);
    let ans = &payload["answers"][0];
    assert_eq!(ans["response"], json!({ "optionId": "A" }));
    assert_eq!(ans["changeCount"], 2);
    let events: Vec<&str> = payload["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(events, vec!["start", "timeout"]);
    assert!(payload["finishedAt"].is_string());
}

#[test]
fn violations_terminate_and_min_time_blocks_submit() {
    let (core, _d) = temp_core();
    let pkg = install(&core, |p| {
        p["exam"]["settings"]["maxViolations"] = json!(2);
        p["exam"]["settings"]["minTimeBeforeSubmitMinutes"] = json!(30);
    });
    let t0 = start_of(&pkg) + Duration::minutes(1);
    let a = core.login(&login_req(&pkg, "DEMO-0008"), t0).unwrap();
    let err = core.submit(&a, true, t0 + Duration::minutes(10)).unwrap_err();
    assert!(err.to_string().contains("menit"));
    assert_eq!(core.submit(&a, true, t0 + Duration::minutes(31)).unwrap().status, "submitted");
    assert!(core.login(&login_req(&pkg, "DEMO-0008"), t0 + Duration::minutes(32)).is_err());

    let b = core.login(&login_req(&pkg, "DEMO-0009"), t0).unwrap();
    let st = core
        .log_event(&b, "violation", Some(json!({ "reason": "focus_lost" })), t0)
        .unwrap();
    assert_eq!((st.status.as_str(), st.violation_count), ("in_progress", 1));
    let st = core.log_event(&b, "violation", None, t0 + Duration::seconds(5)).unwrap();
    assert_eq!(st.status, "terminated");
    assert_eq!(core.session(&b, t0).unwrap().status, "terminated");
}

#[test]
fn attachments_only_for_file_upload_questions() {
    let (core, _d) = temp_core();
    let pkg = install(&core, |_| {});
    let t0 = start_of(&pkg) + Duration::minutes(1);
    let a = core.login(&login_req(&pkg, "DEMO-0010"), t0).unwrap();
    let fu = qid_of(&pkg, "file_upload");
    let saved = core
        .save_attachment(&a, &fu, "kerja saya.jpg", "image/jpeg", b"jpegdata", t0)
        .unwrap();
    assert_eq!(saved.size, 8);
    assert_eq!(saved.name, "kerja saya.jpg");
    let essay = qid_of(&pkg, "essay");
    assert!(core.save_attachment(&a, &essay, "x.txt", "text/plain", b"x", t0).is_err());
}

#[test]
fn config_requires_pin_and_secret() {
    let (core, _d) = temp_core();
    let input = |secret: Option<&str>, pin: Option<&str>| super::ConfigInput {
        server_url: "http://server:8080/".into(),
        site_code: "lab-1".into(),
        secret: secret.map(String::from),
        operator_pin: pin.map(String::from),
        device_name: Some("PC-01".into()),
        auto_sync: None,
    };
    assert!(core.save_config(input(Some("rahasia"), None)).is_err());
    assert!(core.save_config(input(None, Some("1234"))).is_err());
    assert!(core.save_config(input(Some("rahasia"), Some("12"))).is_err());
    let v = core.save_config(input(Some("rahasia"), Some("1234"))).unwrap();
    assert!(v.configured && v.has_pin && v.auto_sync);
    assert_eq!(v.server_url.as_deref(), Some("http://server:8080"));
    assert_eq!(v.site_code.as_deref(), Some("LAB-1"));
    // Simpan ulang tanpa secret/PIN mempertahankan yang lama.
    core.save_config(input(None, None)).unwrap();
    assert!(core.verify_pin("1234").unwrap());
    assert_eq!(core.credentials().unwrap().secret, "rahasia");
}
