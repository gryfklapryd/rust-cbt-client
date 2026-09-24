import { z } from "zod";
import { Choice, LocalId, RichText, findDuplicates } from "../common.js";

/**
 * Semua jenis soal yang didukung sistem.
 *
 * Setiap jenis soal terdiri dari tiga bagian:
 *  - `content`   : bagian publik yang dikirim ke client desktop (tanpa kunci).
 *  - `answerKey` : kunci jawaban / rubrik. HANYA disimpan di server pusat,
 *                  tidak pernah ikut paket ujian.
 *  - response    : bentuk jawaban peserta yang dikirim balik oleh client.
 */
export const QUESTION_TYPES = [
  "single_choice",
  "multiple_choice",
  "true_false",
  "multiple_true_false",
  "short_answer",
  "numeric",
  "essay",
  "matching",
  "ordering",
  "fill_blanks",
  "categorization",
  "hotspot",
  "hot_text",
  "matrix",
  "file_upload",
] as const;
export const QuestionType = z.enum(QUESTION_TYPES);
export type QuestionType = z.infer<typeof QuestionType>;

// ---------------------------------------------------------------------------
// Aturan pencocokan yang dipakai ulang oleh beberapa jenis soal
// ---------------------------------------------------------------------------

/** Aturan pencocokan jawaban teks (isian singkat, blank teks). */
export const TextMatchRule = z.object({
  value: z.string().min(1).max(2_000),
  /** exact: sama persis (setelah normalisasi); contains: jawaban memuat nilai; regex: ekspresi reguler. */
  mode: z.enum(["exact", "contains", "regex"]).default("exact"),
  caseSensitive: z.boolean().default(false),
  /** Rapikan spasi (trim + spasi ganda jadi satu). */
  normalizeWhitespace: z.boolean().default(true),
  /** Abaikan tanda baca di jawaban dan nilai kunci. */
  ignorePunctuation: z.boolean().default(false),
  /** Abaikan diakritik (é -> e). */
  ignoreAccents: z.boolean().default(false),
  /** Bobot nilai jika aturan ini cocok (0..1). Berguna untuk jawaban "setengah benar". */
  score: z.number().min(0).max(1).default(1),
});
export type TextMatchRule = z.infer<typeof TextMatchRule>;

/** Aturan jawaban numerik: nilai tunggal (+ toleransi) atau rentang. */
export const NumericRule = z
  .object({
    value: z.number().optional(),
    tolerance: z.number().min(0).default(0),
    toleranceMode: z.enum(["absolute", "percent"]).default("absolute"),
    min: z.number().optional(),
    max: z.number().optional(),
    score: z.number().min(0).max(1).default(1),
  })
  .refine((r) => r.value !== undefined || (r.min !== undefined && r.max !== undefined), {
    message: "Isi `value` atau pasangan `min` dan `max`",
  })
  .refine((r) => r.min === undefined || r.max === undefined || r.min <= r.max, {
    message: "`min` harus <= `max`",
  });
export type NumericRule = z.infer<typeof NumericRule>;

/** Rubrik penilaian manual (uraian, unggah berkas). */
export const RubricCriterion = z.object({
  id: LocalId,
  criterion: z.string().min(1).max(2_000),
  maxPoints: z.number().min(0),
});
export type RubricCriterion = z.infer<typeof RubricCriterion>;

const ManualKey = z.object({
  modelAnswer: RichText.optional(),
  rubric: z.array(RubricCriterion).max(50).optional(),
  graderNotes: z.string().max(10_000).optional(),
});

const Prompt = RichText.min(1);

// ---------------------------------------------------------------------------
// 1. Pilihan ganda (satu jawaban)
// ---------------------------------------------------------------------------
export const SingleChoiceContent = z.object({
  prompt: Prompt,
  options: z.array(Choice).min(2).max(26),
  shuffleOptions: z.boolean().default(true),
});
export const SingleChoiceKey = z.object({
  correctOptionId: LocalId,
  /** Opsional: bobot per opsi (0..1) untuk soal berbobot (mis. tes sikap). Menggantikan kunci tunggal. */
  optionWeights: z.record(LocalId, z.number().min(0).max(1)).optional(),
});
export const SingleChoiceResponse = z.object({ optionId: LocalId.nullable() });

// ---------------------------------------------------------------------------
// 2. Pilihan ganda kompleks (lebih dari satu jawaban)
// ---------------------------------------------------------------------------
export const MultipleChoiceContent = z.object({
  prompt: Prompt,
  options: z.array(Choice).min(2).max(26),
  minSelections: z.number().int().min(0).optional(),
  maxSelections: z.number().int().min(1).optional(),
  shuffleOptions: z.boolean().default(true),
});
export const MultipleChoiceKey = z.object({
  correctOptionIds: z.array(LocalId).min(1),
});
export const MultipleChoiceResponse = z.object({ optionIds: z.array(LocalId).max(26) });

// ---------------------------------------------------------------------------
// 3. Benar / Salah
// ---------------------------------------------------------------------------
const TrueFalseLabels = z.object({
  true: z.string().min(1).max(100).default("Benar"),
  false: z.string().min(1).max(100).default("Salah"),
});
export const TrueFalseContent = z.object({
  prompt: Prompt,
  labels: TrueFalseLabels.optional(),
});
export const TrueFalseKey = z.object({ value: z.boolean() });
export const TrueFalseResponse = z.object({ value: z.boolean().nullable() });

// ---------------------------------------------------------------------------
// 4. Benar/Salah majemuk (tabel pernyataan, gaya AKM)
// ---------------------------------------------------------------------------
export const MultipleTrueFalseContent = z.object({
  prompt: Prompt,
  statements: z.array(Choice).min(1).max(50),
  labels: TrueFalseLabels.optional(),
  shuffleStatements: z.boolean().default(false),
});
export const MultipleTrueFalseKey = z.object({ values: z.record(LocalId, z.boolean()) });
export const MultipleTrueFalseResponse = z.object({ values: z.record(LocalId, z.boolean().nullable()) });

// ---------------------------------------------------------------------------
// 5. Isian singkat
// ---------------------------------------------------------------------------
export const ShortAnswerContent = z.object({
  prompt: Prompt,
  maxLength: z.number().int().min(1).max(2_000).default(200),
  placeholder: z.string().max(200).optional(),
});
export const ShortAnswerKey = z.object({ accepted: z.array(TextMatchRule).min(1).max(100) });
export const ShortAnswerResponse = z.object({ text: z.string().max(2_000).nullable() });

// ---------------------------------------------------------------------------
// 6. Isian angka
// ---------------------------------------------------------------------------
export const NumericContent = z.object({
  prompt: Prompt,
  unit: z.string().max(50).optional(),
  /** Petunjuk tampilan jumlah desimal; tidak mempengaruhi penilaian. */
  decimalPlaces: z.number().int().min(0).max(10).optional(),
});
export const NumericKey = z.object({ accepted: z.array(NumericRule).min(1).max(20) });
/** Client boleh mengirim angka atau string (mis. "3,14" dengan koma desimal). */
export const NumericResponse = z.object({ value: z.union([z.number(), z.string().max(100)]).nullable() });

// ---------------------------------------------------------------------------
// 7. Uraian / esai (dinilai manual)
// ---------------------------------------------------------------------------
export const EssayContent = z.object({
  prompt: Prompt,
  minWords: z.number().int().min(0).optional(),
  maxWords: z.number().int().min(1).optional(),
  /** Izinkan format teks (tebal, daftar, dsb) di editor jawaban. */
  richText: z.boolean().default(false),
});
export const EssayKey = ManualKey;
export const EssayResponse = z.object({ text: z.string().max(100_000).nullable() });

// ---------------------------------------------------------------------------
// 8. Menjodohkan
// ---------------------------------------------------------------------------
export const MatchingContent = z.object({
  prompt: Prompt,
  left: z.array(Choice).min(1).max(50),
  right: z.array(Choice).min(1).max(50),
  /** Boleh satu item kanan dipakai oleh beberapa item kiri. */
  allowReuse: z.boolean().default(false),
  shuffle: z.boolean().default(true),
});
const Pair = z.object({ leftId: LocalId, rightId: LocalId });
export const MatchingKey = z.object({ pairs: z.array(Pair).min(1) });
export const MatchingResponse = z.object({ pairs: z.array(Pair).max(50) });

// ---------------------------------------------------------------------------
// 9. Mengurutkan
// ---------------------------------------------------------------------------
export const OrderingContent = z.object({
  prompt: Prompt,
  items: z.array(Choice).min(2).max(50),
});
export const OrderingKey = z.object({ order: z.array(LocalId).min(2) });
export const OrderingResponse = z.object({ order: z.array(LocalId).max(50) });

// ---------------------------------------------------------------------------
// 10. Isian rumpang (cloze): blank teks, angka, dropdown, atau bank kata (drag)
// ---------------------------------------------------------------------------
export const BlankKind = z.enum(["text", "numeric", "dropdown", "word_bank"]);
export const Blank = z.object({
  id: LocalId,
  kind: BlankKind,
  /** Opsi untuk blank `dropdown`. */
  options: z.array(Choice).max(26).optional(),
  /** Lebar tampilan (jumlah karakter) untuk blank teks/angka. */
  width: z.number().int().min(1).max(100).optional(),
});
export const FILL_BLANK_MARKER = /\[\[([A-Za-z0-9_-]+)\]\]/g;
export const FillBlanksContent = z.object({
  prompt: Prompt,
  /** Teks berisi penanda blank `[[idBlank]]`. */
  text: RichText.min(1),
  blanks: z.array(Blank).min(1).max(100),
  /** Bank kata untuk blank `word_bank` (drag & drop kata ke rumpang). */
  wordBank: z.array(Choice).max(100).optional(),
  /** Satu kata di bank boleh dipakai di lebih dari satu blank. */
  reuseWordBank: z.boolean().default(false),
});
export const BlankKey = z.object({
  /** Untuk blank `text`. */
  accepted: z.array(TextMatchRule).optional(),
  /** Untuk blank `numeric`. */
  numeric: z.array(NumericRule).optional(),
  /** Untuk blank `dropdown` / `word_bank`: id opsi yang dianggap benar. */
  optionIds: z.array(LocalId).optional(),
});
export const FillBlanksKey = z.object({ blanks: z.record(LocalId, BlankKey) });
export const FillBlanksResponse = z.object({ values: z.record(LocalId, z.string().max(2_000).nullable()) });

// ---------------------------------------------------------------------------
// 11. Pengelompokan / kategorisasi (drag & drop ke kategori)
// ---------------------------------------------------------------------------
export const CategorizationContent = z.object({
  prompt: Prompt,
  items: z.array(Choice).min(1).max(100),
  categories: z.array(Choice).min(2).max(20),
  shuffleItems: z.boolean().default(true),
});
/** Item yang tidak ada di `mapping` dianggap pengecoh (seharusnya tidak ditempatkan). */
export const CategorizationKey = z.object({ mapping: z.record(LocalId, LocalId) });
export const CategorizationResponse = z.object({ mapping: z.record(LocalId, LocalId.nullable()) });

// ---------------------------------------------------------------------------
// 12. Hotspot (klik area pada gambar). Koordinat dinormalisasi 0..1.
// ---------------------------------------------------------------------------
const Unit = z.number().min(0).max(1);
export const HotspotRegion = z.discriminatedUnion("shape", [
  z.object({ id: LocalId, shape: z.literal("rect"), x: Unit, y: Unit, width: Unit, height: Unit }),
  z.object({ id: LocalId, shape: z.literal("circle"), cx: Unit, cy: Unit, r: Unit }),
  z.object({
    id: LocalId,
    shape: z.literal("polygon"),
    points: z.array(z.tuple([Unit, Unit])).min(3).max(100),
  }),
]);
export type HotspotRegion = z.infer<typeof HotspotRegion>;
export const HotspotContent = z.object({
  prompt: Prompt,
  image: z.object({
    /** `asset://<uuid>` */
    src: z.string().startsWith("asset://"),
    width: z.number().int().positive(),
    height: z.number().int().positive(),
    alt: z.string().max(500).optional(),
  }),
  maxSelections: z.number().int().min(1).max(50).default(1),
});
export const HotspotKey = z.object({ regions: z.array(HotspotRegion).min(1).max(50) });
export const HotspotResponse = z.object({ points: z.array(z.object({ x: Unit, y: Unit })).max(50) });

// ---------------------------------------------------------------------------
// 13. Pilih teks (hot text): pilih kata/kalimat di dalam bacaan
// ---------------------------------------------------------------------------
export const HotTextContent = z.object({
  prompt: Prompt,
  segments: z
    .array(
      z.object({
        id: LocalId,
        content: RichText,
        selectable: z.boolean().default(true),
      }),
    )
    .min(1)
    .max(500),
  maxSelections: z.number().int().min(1).optional(),
});
export const HotTextKey = z.object({ correctSegmentIds: z.array(LocalId).min(1) });
export const HotTextResponse = z.object({ segmentIds: z.array(LocalId).max(500) });

// ---------------------------------------------------------------------------
// 14. Matriks / grid (termasuk skala Likert / angket bila tanpa kunci)
// ---------------------------------------------------------------------------
export const MatrixContent = z.object({
  prompt: Prompt,
  rows: z.array(Choice).min(1).max(100),
  columns: z.array(Choice).min(2).max(20),
  multiplePerRow: z.boolean().default(false),
});
/** Tanpa `correct` → soal angket/survei (tidak dinilai). */
export const MatrixKey = z.object({ correct: z.record(LocalId, z.array(LocalId).min(1)).optional() });
export const MatrixResponse = z.object({ selections: z.record(LocalId, z.array(LocalId).max(20)) });

// ---------------------------------------------------------------------------
// 15. Unggah berkas (gambar kerja, rekaman audio, dokumen), dinilai manual
// ---------------------------------------------------------------------------
export const FileUploadContent = z.object({
  prompt: Prompt,
  /** MIME type atau ekstensi, mis. ["image/*", ".pdf", "audio/webm"]. Kosong = semua. */
  accept: z.array(z.string().min(1).max(100)).max(50).default([]),
  maxFiles: z.number().int().min(1).max(20).default(1),
  maxSizeMb: z.number().min(0.1).max(500).default(10),
});
export const FileUploadKey = ManualKey;
export const FileUploadResponse = z.object({
  files: z
    .array(
      z.object({
        attachmentId: z.uuid(),
        name: z.string().max(500),
        size: z.number().int().min(0),
        mime: z.string().max(200),
      }),
    )
    .max(20),
});

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------
export const QUESTION_SCHEMAS = {
  single_choice: { content: SingleChoiceContent, answerKey: SingleChoiceKey, response: SingleChoiceResponse },
  multiple_choice: { content: MultipleChoiceContent, answerKey: MultipleChoiceKey, response: MultipleChoiceResponse },
  true_false: { content: TrueFalseContent, answerKey: TrueFalseKey, response: TrueFalseResponse },
  multiple_true_false: {
    content: MultipleTrueFalseContent,
    answerKey: MultipleTrueFalseKey,
    response: MultipleTrueFalseResponse,
  },
  short_answer: { content: ShortAnswerContent, answerKey: ShortAnswerKey, response: ShortAnswerResponse },
  numeric: { content: NumericContent, answerKey: NumericKey, response: NumericResponse },
  essay: { content: EssayContent, answerKey: EssayKey, response: EssayResponse },
  matching: { content: MatchingContent, answerKey: MatchingKey, response: MatchingResponse },
  ordering: { content: OrderingContent, answerKey: OrderingKey, response: OrderingResponse },
  fill_blanks: { content: FillBlanksContent, answerKey: FillBlanksKey, response: FillBlanksResponse },
  categorization: { content: CategorizationContent, answerKey: CategorizationKey, response: CategorizationResponse },
  hotspot: { content: HotspotContent, answerKey: HotspotKey, response: HotspotResponse },
  hot_text: { content: HotTextContent, answerKey: HotTextKey, response: HotTextResponse },
  matrix: { content: MatrixContent, answerKey: MatrixKey, response: MatrixResponse },
  file_upload: { content: FileUploadContent, answerKey: FileUploadKey, response: FileUploadResponse },
} as const satisfies Record<
  QuestionType,
  { content: z.ZodType; answerKey: z.ZodType; response: z.ZodType }
>;

type Schemas = typeof QUESTION_SCHEMAS;
export type QuestionContent<T extends QuestionType = QuestionType> = z.infer<Schemas[T]["content"]>;
export type QuestionAnswerKey<T extends QuestionType = QuestionType> = z.infer<Schemas[T]["answerKey"]>;
export type QuestionResponse<T extends QuestionType = QuestionType> = z.infer<Schemas[T]["response"]>;

const QuestionDefinitionUnion = z.discriminatedUnion("type", [
  z.object({ type: z.literal("single_choice"), content: QUESTION_SCHEMAS.single_choice.content, answerKey: QUESTION_SCHEMAS.single_choice.answerKey }),
  z.object({ type: z.literal("multiple_choice"), content: QUESTION_SCHEMAS.multiple_choice.content, answerKey: QUESTION_SCHEMAS.multiple_choice.answerKey }),
  z.object({ type: z.literal("true_false"), content: QUESTION_SCHEMAS.true_false.content, answerKey: QUESTION_SCHEMAS.true_false.answerKey }),
  z.object({ type: z.literal("multiple_true_false"), content: QUESTION_SCHEMAS.multiple_true_false.content, answerKey: QUESTION_SCHEMAS.multiple_true_false.answerKey }),
  z.object({ type: z.literal("short_answer"), content: QUESTION_SCHEMAS.short_answer.content, answerKey: QUESTION_SCHEMAS.short_answer.answerKey }),
  z.object({ type: z.literal("numeric"), content: QUESTION_SCHEMAS.numeric.content, answerKey: QUESTION_SCHEMAS.numeric.answerKey }),
  z.object({ type: z.literal("essay"), content: QUESTION_SCHEMAS.essay.content, answerKey: QUESTION_SCHEMAS.essay.answerKey }),
  z.object({ type: z.literal("matching"), content: QUESTION_SCHEMAS.matching.content, answerKey: QUESTION_SCHEMAS.matching.answerKey }),
  z.object({ type: z.literal("ordering"), content: QUESTION_SCHEMAS.ordering.content, answerKey: QUESTION_SCHEMAS.ordering.answerKey }),
  z.object({ type: z.literal("fill_blanks"), content: QUESTION_SCHEMAS.fill_blanks.content, answerKey: QUESTION_SCHEMAS.fill_blanks.answerKey }),
  z.object({ type: z.literal("categorization"), content: QUESTION_SCHEMAS.categorization.content, answerKey: QUESTION_SCHEMAS.categorization.answerKey }),
  z.object({ type: z.literal("hotspot"), content: QUESTION_SCHEMAS.hotspot.content, answerKey: QUESTION_SCHEMAS.hotspot.answerKey }),
  z.object({ type: z.literal("hot_text"), content: QUESTION_SCHEMAS.hot_text.content, answerKey: QUESTION_SCHEMAS.hot_text.answerKey }),
  z.object({ type: z.literal("matrix"), content: QUESTION_SCHEMAS.matrix.content, answerKey: QUESTION_SCHEMAS.matrix.answerKey }),
  z.object({ type: z.literal("file_upload"), content: QUESTION_SCHEMAS.file_upload.content, answerKey: QUESTION_SCHEMAS.file_upload.answerKey }),
]);

/** Definisi lengkap soal (konten + kunci), divalidasi termasuk konsistensi antar-bagian. */
export const QuestionDefinition = QuestionDefinitionUnion.superRefine((def, ctx) => {
  for (const issue of checkConsistency(def)) {
    ctx.addIssue({ code: "custom", message: issue.message, path: issue.path });
  }
});
export type QuestionDefinition = z.infer<typeof QuestionDefinitionUnion>;
export type QuestionDefinitionOf<T extends QuestionType> = Extract<QuestionDefinition, { type: T }>;

// ---------------------------------------------------------------------------
// Pengaturan skor
// ---------------------------------------------------------------------------
export const Scoring = z.object({
  /** Skor maksimum soal. 0 = tidak dinilai (mis. angket). */
  points: z.number().min(0).max(1_000).default(1),
  /**
   * all_or_nothing: skor penuh hanya jika seluruh jawaban benar.
   * partial       : skor proporsional terhadap bagian yang benar.
   */
  mode: z.enum(["all_or_nothing", "partial"]).default("all_or_nothing"),
  /** Pengurangan nilai jika dijawab tetapi salah total (bukan untuk jawaban kosong). */
  penalty: z.number().min(0).max(1_000).default(0),
  /** Izinkan skor akhir negatif akibat penalti. */
  allowNegative: z.boolean().default(false),
});
export type Scoring = z.infer<typeof Scoring>;
export type ScoringInput = z.input<typeof Scoring>;

/** Jenis soal yang (selalu) membutuhkan penilaian manual. */
export const MANUAL_GRADED_TYPES: ReadonlySet<QuestionType> = new Set(["essay", "file_upload"]);

// ---------------------------------------------------------------------------
// Pemeriksaan konsistensi (id unik, kunci merujuk ke id yang ada, dst)
// ---------------------------------------------------------------------------
interface Issue {
  message: string;
  path: (string | number)[];
}

function checkConsistency(def: QuestionDefinition): Issue[] {
  const issues: Issue[] = [];
  const unique = (ids: string[], path: (string | number)[]) => {
    const d = findDuplicates(ids);
    if (d.length) issues.push({ message: `ID duplikat: ${d.join(", ")}`, path });
  };
  const exists = (known: Set<string>, ids: Iterable<string>, path: (string | number)[], what = "id") => {
    for (const id of ids) {
      if (!known.has(id)) issues.push({ message: `${what} tidak dikenal: ${id}`, path });
    }
  };
  const ids = (xs: { id: string }[]) => new Set(xs.map((x) => x.id));

  switch (def.type) {
    case "single_choice": {
      const { content, answerKey } = def;
      unique(content.options.map((o) => o.id), ["content", "options"]);
      exists(ids(content.options), [answerKey.correctOptionId], ["answerKey", "correctOptionId"], "Opsi");
      if (answerKey.optionWeights) {
        exists(ids(content.options), Object.keys(answerKey.optionWeights), ["answerKey", "optionWeights"], "Opsi");
      }
      break;
    }
    case "multiple_choice": {
      const { content, answerKey } = def;
      unique(content.options.map((o) => o.id), ["content", "options"]);
      unique(answerKey.correctOptionIds, ["answerKey", "correctOptionIds"]);
      exists(ids(content.options), answerKey.correctOptionIds, ["answerKey", "correctOptionIds"], "Opsi");
      if (
        content.minSelections !== undefined &&
        content.maxSelections !== undefined &&
        content.minSelections > content.maxSelections
      ) {
        issues.push({ message: "minSelections > maxSelections", path: ["content", "minSelections"] });
      }
      break;
    }
    case "multiple_true_false": {
      const { content, answerKey } = def;
      unique(content.statements.map((s) => s.id), ["content", "statements"]);
      const known = ids(content.statements);
      exists(known, Object.keys(answerKey.values), ["answerKey", "values"], "Pernyataan");
      for (const id of known) {
        if (!(id in answerKey.values)) {
          issues.push({ message: `Kunci untuk pernyataan ${id} belum diisi`, path: ["answerKey", "values"] });
        }
      }
      break;
    }
    case "matching": {
      const { content, answerKey } = def;
      unique(content.left.map((o) => o.id), ["content", "left"]);
      unique(content.right.map((o) => o.id), ["content", "right"]);
      exists(ids(content.left), answerKey.pairs.map((p) => p.leftId), ["answerKey", "pairs"], "Item kiri");
      exists(ids(content.right), answerKey.pairs.map((p) => p.rightId), ["answerKey", "pairs"], "Item kanan");
      unique(answerKey.pairs.map((p) => p.leftId), ["answerKey", "pairs"]);
      if (!content.allowReuse) unique(answerKey.pairs.map((p) => p.rightId), ["answerKey", "pairs"]);
      break;
    }
    case "ordering": {
      const { content, answerKey } = def;
      const itemIds = content.items.map((i) => i.id);
      unique(itemIds, ["content", "items"]);
      unique(answerKey.order, ["answerKey", "order"]);
      if (answerKey.order.length !== itemIds.length || !itemIds.every((id) => answerKey.order.includes(id))) {
        issues.push({ message: "Kunci urutan harus memuat semua item tepat satu kali", path: ["answerKey", "order"] });
      }
      break;
    }
    case "fill_blanks": {
      const { content, answerKey } = def;
      const blankIds = content.blanks.map((b) => b.id);
      unique(blankIds, ["content", "blanks"]);
      const markers = [...content.text.matchAll(FILL_BLANK_MARKER)].map((m) => m[1]!);
      unique(markers, ["content", "text"]);
      for (const id of blankIds) {
        if (!markers.includes(id)) issues.push({ message: `Penanda [[${id}]] tidak ada di teks`, path: ["content", "text"] });
      }
      for (const id of markers) {
        if (!blankIds.includes(id)) issues.push({ message: `Blank ${id} dipakai di teks tapi tidak didefinisikan`, path: ["content", "blanks"] });
      }
      const bank = ids(content.wordBank ?? []);
      content.blanks.forEach((blank, i) => {
        const key = answerKey.blanks[blank.id];
        const path = ["answerKey", "blanks", blank.id];
        if (!key) {
          issues.push({ message: `Kunci untuk blank ${blank.id} belum diisi`, path });
          return;
        }
        switch (blank.kind) {
          case "text":
            if (!key.accepted?.length) issues.push({ message: "Blank teks butuh `accepted`", path });
            break;
          case "numeric":
            if (!key.numeric?.length) issues.push({ message: "Blank angka butuh `numeric`", path });
            break;
          case "dropdown":
            if (!blank.options?.length) {
              issues.push({ message: "Blank dropdown butuh `options`", path: ["content", "blanks", i, "options"] });
            }
            if (!key.optionIds?.length) issues.push({ message: "Blank dropdown butuh `optionIds`", path });
            else exists(ids(blank.options ?? []), key.optionIds, path, "Opsi");
            break;
          case "word_bank":
            if (!content.wordBank?.length) {
              issues.push({ message: "Blank word_bank butuh `wordBank`", path: ["content", "wordBank"] });
            }
            if (!key.optionIds?.length) issues.push({ message: "Blank word_bank butuh `optionIds`", path });
            else exists(bank, key.optionIds, path, "Kata");
            break;
        }
      });
      exists(new Set(blankIds), Object.keys(answerKey.blanks), ["answerKey", "blanks"], "Blank");
      break;
    }
    case "categorization": {
      const { content, answerKey } = def;
      unique(content.items.map((o) => o.id), ["content", "items"]);
      unique(content.categories.map((o) => o.id), ["content", "categories"]);
      exists(ids(content.items), Object.keys(answerKey.mapping), ["answerKey", "mapping"], "Item");
      exists(ids(content.categories), Object.values(answerKey.mapping), ["answerKey", "mapping"], "Kategori");
      break;
    }
    case "hotspot": {
      unique(def.answerKey.regions.map((r) => r.id), ["answerKey", "regions"]);
      break;
    }
    case "hot_text": {
      const { content, answerKey } = def;
      unique(content.segments.map((s) => s.id), ["content", "segments"]);
      const selectable = new Set(content.segments.filter((s) => s.selectable).map((s) => s.id));
      exists(selectable, answerKey.correctSegmentIds, ["answerKey", "correctSegmentIds"], "Segmen (yang bisa dipilih)");
      break;
    }
    case "matrix": {
      const { content, answerKey } = def;
      unique(content.rows.map((o) => o.id), ["content", "rows"]);
      unique(content.columns.map((o) => o.id), ["content", "columns"]);
      if (answerKey.correct) {
        exists(ids(content.rows), Object.keys(answerKey.correct), ["answerKey", "correct"], "Baris");
        exists(ids(content.columns), Object.values(answerKey.correct).flat(), ["answerKey", "correct"], "Kolom");
        if (!content.multiplePerRow && Object.values(answerKey.correct).some((c) => c.length > 1)) {
          issues.push({ message: "Satu baris hanya boleh satu kunci bila multiplePerRow=false", path: ["answerKey", "correct"] });
        }
      }
      break;
    }
    case "essay":
    case "file_upload": {
      const rubric = def.answerKey.rubric ?? [];
      unique(rubric.map((r) => r.id), ["answerKey", "rubric"]);
      break;
    }
    case "true_false":
    case "short_answer":
    case "numeric":
      break;
  }
  return issues;
}
