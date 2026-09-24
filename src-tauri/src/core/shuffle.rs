//! Pengacakan deterministik per attempt: urutan soal (termasuk ambil-acak N soal
//! per bagian) dan urutan opsi. Seed diturunkan dari ID attempt, sehingga hasilnya
//! sama bila sesi dilanjutkan setelah aplikasi ditutup/crash.

use std::collections::BTreeMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::package::{ExamPackage, PackageQuestion};

/// PRNG SplitMix64: kecil, cepat, dan cukup untuk mengacak urutan.
pub struct Rng(u64);

impl Rng {
    pub fn from_seed(seed: &str) -> Self {
        let digest = Sha256::digest(seed.as_bytes());
        let mut b = [0u8; 8];
        b.copy_from_slice(&digest[..8]);
        Rng(u64::from_le_bytes(b))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Bilangan acak seragam di 0..n (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// Fisher-Yates.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

/// Susunan soal satu bagian untuk seorang peserta.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SectionPlan {
    pub section_id: String,
    pub question_ids: Vec<String>,
}

/// Urutan soal per bagian setelah ambil-acak & acak urutan.
pub fn plan_questions(pkg: &ExamPackage, attempt_id: &str) -> Vec<SectionPlan> {
    let mut rng = Rng::from_seed(&format!("{attempt_id}:questions"));
    let mut sections = pkg.exam.sections.clone();
    sections.sort_by_key(|s| s.order);
    sections
        .into_iter()
        .map(|s| {
            let mut ids: Vec<String> = s
                .question_ids
                .iter()
                .filter(|id| pkg.question(id).is_some())
                .cloned()
                .collect();
            if let Some(n) = s.pick_count.filter(|n| *n > 0) {
                let n = (n as usize).min(ids.len());
                // Pilih N soal secara acak, pertahankan urutan asli di antara yang terpilih.
                let mut idx: Vec<usize> = (0..ids.len()).collect();
                rng.shuffle(&mut idx);
                let mut chosen: Vec<usize> = idx.into_iter().take(n).collect();
                chosen.sort_unstable();
                ids = chosen.into_iter().map(|i| ids[i].clone()).collect();
            }
            if s.shuffle_questions || pkg.exam.settings.shuffle_questions {
                rng.shuffle(&mut ids);
            }
            SectionPlan {
                section_id: s.id,
                question_ids: ids,
            }
        })
        .collect()
}

/// Nama daftar yang diacak untuk tiap jenis soal, beserta flag di konten yang mengizinkannya.
/// `None` sebagai flag = selalu diacak (mis. soal mengurutkan: urutan awal tidak boleh sama dengan kunci).
fn shuffle_lists(qtype: &str) -> &'static [(&'static str, Option<&'static str>)] {
    match qtype {
        "single_choice" | "multiple_choice" => &[("options", Some("shuffleOptions"))],
        "multiple_true_false" => &[("statements", Some("shuffleStatements"))],
        "matching" => &[("left", Some("shuffle")), ("right", Some("shuffle"))],
        "categorization" => &[("items", Some("shuffleItems"))],
        "ordering" => &[("items", None)],
        _ => &[],
    }
}

fn ids_of(content: &Value, list: &str) -> Vec<String> {
    content
        .get(list)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|o| o.get("id").and_then(Value::as_str).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Urutan opsi per soal: `questionId -> { namaDaftar -> [id] }`. Hanya soal yang diacak yang dicatat.
pub fn plan_options(
    pkg: &ExamPackage,
    attempt_id: &str,
    question_ids: &[String],
) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
    let mut out = BTreeMap::new();
    for qid in question_ids {
        let Some(q) = pkg.question(qid) else { continue };
        let lists = plan_for_question(q, attempt_id, pkg.exam.settings.shuffle_options);
        if !lists.is_empty() {
            out.insert(qid.clone(), lists);
        }
    }
    out
}

fn plan_for_question(q: &PackageQuestion, attempt_id: &str, exam_allows: bool) -> BTreeMap<String, Vec<String>> {
    let mut rng = Rng::from_seed(&format!("{attempt_id}:{}", q.id));
    let mut lists = BTreeMap::new();
    for (list, flag) in shuffle_lists(&q.qtype) {
        let enabled = match flag {
            None => true,
            Some(f) => exam_allows && q.content.get(*f).and_then(Value::as_bool).unwrap_or(true),
        };
        if !enabled {
            continue;
        }
        let mut ids = ids_of(&q.content, list);
        if ids.len() < 2 {
            continue;
        }
        let original = ids.clone();
        rng.shuffle(&mut ids);
        // Untuk soal mengurutkan, hindari urutan awal yang kebetulan sama dengan urutan asli.
        if flag.is_none() && ids == original {
            ids.rotate_left(1);
        }
        lists.insert((*list).to_string(), ids);
    }
    lists
}

/// Bentuk `optionOrders` untuk dikirim ke server (`questionId -> [id]`), memakai daftar utama.
pub fn flatten_option_orders(orders: &BTreeMap<String, BTreeMap<String, Vec<String>>>) -> BTreeMap<String, Vec<String>> {
    orders
        .iter()
        .filter_map(|(qid, lists)| {
            let primary = ["options", "statements", "items", "right", "left"]
                .iter()
                .find_map(|k| lists.get(*k))?;
            Some((qid.clone(), primary.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_deterministic_and_permutation() {
        let mut a = Rng::from_seed("x");
        let mut b = Rng::from_seed("x");
        assert_eq!(a.next_u64(), b.next_u64());
        let mut v: Vec<u32> = (0..50).collect();
        Rng::from_seed("abc").shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
        assert_ne!(v, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn below_in_range() {
        let mut r = Rng::from_seed("s");
        for n in 1..100 {
            assert!(r.below(n) < n);
        }
    }
}
