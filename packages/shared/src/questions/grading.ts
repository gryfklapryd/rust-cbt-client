import {
  MANUAL_GRADED_TYPES,
  QUESTION_SCHEMAS,
  Scoring,
  type HotspotRegion,
  type NumericRule,
  type QuestionDefinition,
  type QuestionDefinitionOf,
  type QuestionResponse,
  type QuestionType,
  type ScoringInput,
  type TextMatchRule,
} from "./types.js";

/**
 * Status hasil penilaian satu jawaban.
 * - graded         : dinilai otomatis
 * - unanswered     : tidak dijawab (skor 0)
 * - pending_manual : menunggu penilaian manual (uraian, unggah berkas)
 * - ungraded       : soal tidak dinilai (poin 0 / angket)
 * - invalid        : format jawaban tidak sesuai skema jenis soal (skor 0)
 */
export type GradeStatus = "graded" | "unanswered" | "pending_manual" | "ungraded" | "invalid";

export interface GradeResult {
  status: GradeStatus;
  /** null bila masih menunggu penilaian manual. */
  score: number | null;
  maxScore: number;
  /** Proporsi benar (0..1) sebelum mode skor & penalti diterapkan. */
  fraction: number | null;
  /** true = benar penuh, false = tidak benar penuh, null = tidak berlaku. */
  correct: boolean | null;
  /** Rincian per bagian (per opsi/blank/pasangan) untuk laporan & analisis butir. */
  details?: Record<string, unknown>;
  /** Pesan kesalahan untuk status `invalid`. */
  error?: string;
}

const round = (n: number) => Math.round(n * 10_000) / 10_000;
const clamp01 = (n: number) => Math.min(1, Math.max(0, n));

// ---------------------------------------------------------------------------
// Utilitas pencocokan
// ---------------------------------------------------------------------------

function normalizeText(s: string, rule: TextMatchRule): string {
  let out = s;
  if (rule.normalizeWhitespace) out = out.trim().replace(/\s+/g, " ");
  if (rule.ignoreAccents) out = out.normalize("NFD").replace(/\p{M}+/gu, "");
  if (rule.ignorePunctuation) out = out.replace(/[\p{P}\p{S}]+/gu, "").replace(/\s+/g, " ").trim();
  if (!rule.caseSensitive) out = out.toLocaleLowerCase("id");
  return out;
}

/** Skor (0..1) terbaik dari aturan yang cocok dengan teks jawaban. */
export function matchText(answer: string, rules: readonly TextMatchRule[]): number {
  let best = 0;
  for (const rule of rules) {
    let ok = false;
    if (rule.mode === "regex") {
      try {
        const re = new RegExp(rule.value, rule.caseSensitive ? "u" : "iu");
        const subject = rule.normalizeWhitespace ? answer.trim().replace(/\s+/g, " ") : answer;
        ok = re.test(subject);
      } catch {
        ok = false;
      }
    } else {
      const a = normalizeText(answer, rule);
      const v = normalizeText(rule.value, rule);
      ok = rule.mode === "exact" ? a === v : v.length > 0 && a.includes(v);
    }
    if (ok) best = Math.max(best, rule.score);
  }
  return best;
}

/** Ubah input angka dari client (angka atau string, mendukung koma desimal & pemisah ribuan) jadi number. */
export function parseNumericInput(value: number | string | null | undefined): number | null {
  if (value === null || value === undefined) return null;
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  let s = value.trim().replace(/\s+/g, "");
  if (!s) return null;
  const hasComma = s.includes(",");
  const hasDot = s.includes(".");
  if (hasComma && hasDot) {
    // Pemisah desimal = yang muncul terakhir; yang lain pemisah ribuan.
    if (s.lastIndexOf(",") > s.lastIndexOf(".")) s = s.replace(/\./g, "").replace(",", ".");
    else s = s.replace(/,/g, "");
  } else if (hasComma) {
    s = s.replace(",", ".");
  }
  if (!/^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i.test(s)) return null;
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
}

/** Skor (0..1) terbaik dari aturan numerik yang cocok. */
export function matchNumeric(value: number, rules: readonly NumericRule[]): number {
  const EPS = 1e-9;
  let best = 0;
  for (const r of rules) {
    let ok = false;
    if (r.value !== undefined) {
      const tol = r.toleranceMode === "percent" ? Math.abs(r.value) * (r.tolerance / 100) : r.tolerance;
      ok = Math.abs(value - r.value) <= tol + EPS;
    } else if (r.min !== undefined && r.max !== undefined) {
      ok = value >= r.min - EPS && value <= r.max + EPS;
    }
    if (ok) best = Math.max(best, r.score);
  }
  return best;
}

export function pointInRegion(p: { x: number; y: number }, region: HotspotRegion): boolean {
  switch (region.shape) {
    case "rect":
      return p.x >= region.x && p.x <= region.x + region.width && p.y >= region.y && p.y <= region.y + region.height;
    case "circle":
      return (p.x - region.cx) ** 2 + (p.y - region.cy) ** 2 <= region.r ** 2;
    case "polygon": {
      // Ray casting.
      let inside = false;
      const pts = region.points;
      for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
        const [xi, yi] = pts[i]!;
        const [xj, yj] = pts[j]!;
        const intersect = yi > p.y !== yj > p.y && p.x < ((xj - xi) * (p.y - yi)) / (yj - yi) + xi;
        if (intersect) inside = !inside;
      }
      return inside;
    }
  }
}

/** Proporsi untuk pilihan jamak: (benar dipilih - salah dipilih) / jumlah kunci. */
function selectionFraction(selected: readonly string[], correct: readonly string[]) {
  const correctSet = new Set(correct);
  const sel = new Set(selected);
  let hits = 0;
  let wrong = 0;
  for (const id of sel) {
    if (correctSet.has(id)) hits++;
    else wrong++;
  }
  const exact = hits === correctSet.size && wrong === 0;
  return { fraction: exact ? 1 : clamp01((hits - wrong) / correctSet.size), exact, hits, wrong };
}

// ---------------------------------------------------------------------------
// Deteksi jawaban kosong
// ---------------------------------------------------------------------------

export function isEmptyResponse(type: QuestionType, response: unknown): boolean {
  if (response === null || response === undefined) return true;
  const r = response as Record<string, unknown>;
  const emptyText = (v: unknown) => v === null || v === undefined || (typeof v === "string" && v.trim() === "");
  const emptyRecord = (v: unknown) =>
    !v || typeof v !== "object" || Object.values(v).every((x) => x === null || (Array.isArray(x) && x.length === 0));
  const emptyArray = (v: unknown) => !Array.isArray(v) || v.length === 0;
  switch (type) {
    case "single_choice":
      return emptyText(r.optionId);
    case "multiple_choice":
      return emptyArray(r.optionIds);
    case "true_false":
      return r.value === null || r.value === undefined;
    case "multiple_true_false":
      return emptyRecord(r.values);
    case "short_answer":
    case "essay":
      return emptyText(r.text);
    case "numeric":
      return emptyText(r.value);
    case "matching":
      return emptyArray(r.pairs);
    case "ordering":
      return emptyArray(r.order);
    case "fill_blanks":
      return !r.values || typeof r.values !== "object" || Object.values(r.values).every(emptyText);
    case "categorization":
      return emptyRecord(r.mapping);
    case "hotspot":
      return emptyArray(r.points);
    case "hot_text":
      return emptyArray(r.segmentIds);
    case "matrix":
      return emptyRecord(r.selections);
    case "file_upload":
      return emptyArray(r.files);
  }
}

// ---------------------------------------------------------------------------
// Penilai per jenis soal: menghasilkan proporsi benar (0..1)
// ---------------------------------------------------------------------------

interface Partial {
  fraction: number;
  /** Benar penuh? Untuk mode all_or_nothing. */
  exact: boolean;
  details?: Record<string, unknown>;
}

type Grader<T extends QuestionType> = (def: QuestionDefinitionOf<T>, response: QuestionResponse<T>) => Partial;

const graders: { [T in QuestionType]?: Grader<T> } = {
  single_choice(def, res) {
    const { answerKey } = def;
    if (answerKey.optionWeights) {
      const w = res.optionId ? (answerKey.optionWeights[res.optionId] ?? 0) : 0;
      return { fraction: w, exact: w === 1 };
    }
    const ok = res.optionId === answerKey.correctOptionId;
    return { fraction: ok ? 1 : 0, exact: ok };
  },

  multiple_choice(def, res) {
    const r = selectionFraction(res.optionIds, def.answerKey.correctOptionIds);
    return { fraction: r.fraction, exact: r.exact, details: { hits: r.hits, wrong: r.wrong } };
  },

  true_false(def, res) {
    const ok = res.value === def.answerKey.value;
    return { fraction: ok ? 1 : 0, exact: ok };
  },

  multiple_true_false(def, res) {
    const ids = def.content.statements.map((s) => s.id);
    const perItem: Record<string, boolean> = {};
    let correct = 0;
    for (const id of ids) {
      const ok = res.values[id] !== undefined && res.values[id] === def.answerKey.values[id];
      perItem[id] = ok;
      if (ok) correct++;
    }
    return { fraction: correct / ids.length, exact: correct === ids.length, details: { perItem } };
  },

  short_answer(def, res) {
    const f = matchText(res.text ?? "", def.answerKey.accepted);
    return { fraction: f, exact: f === 1 };
  },

  numeric(def, res) {
    const n = parseNumericInput(res.value);
    if (n === null) return { fraction: 0, exact: false, details: { parsed: null } };
    const f = matchNumeric(n, def.answerKey.accepted);
    return { fraction: f, exact: f === 1, details: { parsed: n } };
  },

  matching(def, res) {
    const key = new Map(def.answerKey.pairs.map((p) => [p.leftId, p.rightId]));
    const seen = new Set<string>();
    let correct = 0;
    let wrong = 0;
    const perItem: Record<string, boolean> = {};
    for (const p of res.pairs) {
      if (seen.has(p.leftId)) continue; // hanya pasangan pertama untuk tiap item kiri
      seen.add(p.leftId);
      const ok = key.get(p.leftId) === p.rightId;
      perItem[p.leftId] = ok;
      if (ok) correct++;
      else if (!key.has(p.leftId)) wrong++; // memasangkan item kiri pengecoh
    }
    const exact = correct === key.size && wrong === 0;
    return { fraction: clamp01((correct - wrong) / key.size), exact, details: { perItem } };
  },

  ordering(def, res) {
    const key = def.answerKey.order;
    let inPlace = 0;
    key.forEach((id, i) => {
      if (res.order[i] === id) inPlace++;
    });
    const exact = inPlace === key.length && res.order.length === key.length;
    return { fraction: inPlace / key.length, exact, details: { inPlace } };
  },

  fill_blanks(def, res) {
    const perBlank: Record<string, number> = {};
    let total = 0;
    for (const blank of def.content.blanks) {
      const key = def.answerKey.blanks[blank.id];
      const value = res.values[blank.id];
      let f = 0;
      if (key && value !== null && value !== undefined && value !== "") {
        switch (blank.kind) {
          case "text":
            f = matchText(value, key.accepted ?? []);
            break;
          case "numeric": {
            const n = parseNumericInput(value);
            f = n === null ? 0 : matchNumeric(n, key.numeric ?? []);
            break;
          }
          case "dropdown":
          case "word_bank":
            f = key.optionIds?.includes(value) ? 1 : 0;
            break;
        }
      }
      perBlank[blank.id] = f;
      total += f;
    }
    const n = def.content.blanks.length;
    return { fraction: total / n, exact: total === n, details: { perBlank } };
  },

  categorization(def, res) {
    const key = def.answerKey.mapping;
    const keyed = Object.keys(key);
    let correct = 0;
    let wrong = 0;
    const perItem: Record<string, boolean> = {};
    for (const item of def.content.items) {
      const placed = res.mapping[item.id] ?? null;
      const expected = key[item.id] ?? null;
      const ok = placed === expected;
      perItem[item.id] = ok;
      if (expected !== null && ok) correct++;
      else if (expected === null && placed !== null) wrong++; // pengecoh ikut ditempatkan
    }
    const exact = correct === keyed.length && wrong === 0;
    const denom = Math.max(1, keyed.length);
    return { fraction: clamp01((correct - wrong) / denom), exact, details: { perItem } };
  },

  hotspot(def, res) {
    const regions = def.answerKey.regions;
    const points = res.points.slice(0, def.content.maxSelections);
    const hit = new Set<string>();
    let misses = 0;
    for (const p of points) {
      const region = regions.find((r) => !hit.has(r.id) && pointInRegion(p, r));
      if (region) hit.add(region.id);
      else misses++;
    }
    const exact = hit.size === regions.length && misses === 0;
    return {
      fraction: clamp01((hit.size - misses) / regions.length),
      exact,
      details: { hitRegions: [...hit], misses },
    };
  },

  hot_text(def, res) {
    const r = selectionFraction(res.segmentIds, def.answerKey.correctSegmentIds);
    return { fraction: r.fraction, exact: r.exact, details: { hits: r.hits, wrong: r.wrong } };
  },

  matrix(def, res) {
    const correct = def.answerKey.correct!;
    const perRow: Record<string, boolean> = {};
    let ok = 0;
    for (const row of def.content.rows) {
      const expected = [...(correct[row.id] ?? [])].sort();
      const got = [...new Set(res.selections[row.id] ?? [])].sort();
      const same = expected.length === got.length && expected.every((v, i) => v === got[i]);
      perRow[row.id] = same;
      if (same) ok++;
    }
    const n = def.content.rows.length;
    return { fraction: ok / n, exact: ok === n, details: { perRow } };
  },
};

// ---------------------------------------------------------------------------
// API utama
// ---------------------------------------------------------------------------

/** Validasi bentuk jawaban terhadap skema jenis soal. */
export function parseResponse<T extends QuestionType>(type: T, response: unknown) {
  return QUESTION_SCHEMAS[type].response.safeParse(response);
}

/**
 * Nilai satu jawaban peserta.
 *
 * Fungsi murni & deterministik: dipakai worker penilaian di server pusat
 * dan fitur "uji kunci" di panel admin.
 */
export function gradeResponse(
  definition: QuestionDefinition,
  scoringInput: ScoringInput | undefined,
  rawResponse: unknown,
): GradeResult {
  const scoring = Scoring.parse(scoringInput ?? {});
  const maxScore = scoring.points;
  const type = definition.type;

  const isSurvey = type === "matrix" && !(definition as QuestionDefinitionOf<"matrix">).answerKey.correct;
  if (maxScore === 0 || isSurvey) {
    return { status: "ungraded", score: 0, maxScore: isSurvey ? 0 : maxScore, fraction: null, correct: null };
  }

  if (isEmptyResponse(type, rawResponse)) {
    return { status: "unanswered", score: 0, maxScore, fraction: 0, correct: false };
  }

  const parsed = parseResponse(type, rawResponse);
  if (!parsed.success) {
    return {
      status: "invalid",
      score: 0,
      maxScore,
      fraction: 0,
      correct: false,
      error: parsed.error.issues.map((i) => `${i.path.join(".") || "(root)"}: ${i.message}`).join("; "),
    };
  }

  if (MANUAL_GRADED_TYPES.has(type)) {
    return { status: "pending_manual", score: null, maxScore, fraction: null, correct: null };
  }

  const grader = graders[type] as Grader<QuestionType> | undefined;
  if (!grader) {
    return { status: "pending_manual", score: null, maxScore, fraction: null, correct: null };
  }
  const partial = grader(definition, parsed.data as QuestionResponse);
  const fraction = clamp01(partial.fraction);
  const effective = scoring.mode === "all_or_nothing" ? (partial.exact ? 1 : 0) : fraction;

  let score = effective * maxScore;
  if (effective === 0 && scoring.penalty > 0) score -= scoring.penalty;
  if (!scoring.allowNegative) score = Math.max(0, score);

  return {
    status: "graded",
    score: round(score),
    maxScore,
    fraction: round(fraction),
    correct: partial.exact,
    ...(partial.details ? { details: partial.details } : {}),
  };
}

/**
 * Hitung skor manual dari rubrik. Skor tiap kriteria dibatasi 0..maxPoints, lalu
 * total rubrik diskalakan ke skor maksimum soal: (jumlah / total maxPoints) x maxScore.
 * Jadi rubrik boleh memakai skala bebas (mis. 0-4 per kriteria) untuk soal berbobot berapa pun.
 */
export function scoreFromRubric(
  rubric: readonly { id: string; maxPoints: number }[],
  rubricScores: Record<string, number>,
  maxScore: number,
): number {
  let total = 0;
  let totalMax = 0;
  for (const c of rubric) {
    const s = rubricScores[c.id] ?? 0;
    total += Math.min(Math.max(0, s), c.maxPoints);
    totalMax += c.maxPoints;
  }
  if (totalMax <= 0) return 0;
  return round((total / totalMax) * maxScore);
}
