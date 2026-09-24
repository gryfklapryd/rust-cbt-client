import type { QuestionDefinition, QuestionType, ScoringInput } from "./types.js";

export interface QuestionTypeMeta {
  label: string;
  description: string;
  /** Dinilai otomatis oleh sistem? */
  autoGraded: boolean;
  /** Mode skor bawaan saat membuat soal baru. */
  defaultScoringMode: "all_or_nothing" | "partial";
}

export const QUESTION_TYPE_META: Record<QuestionType, QuestionTypeMeta> = {
  single_choice: {
    label: "Pilihan ganda",
    description: "Satu jawaban benar dari beberapa opsi. Mendukung bobot per opsi.",
    autoGraded: true,
    defaultScoringMode: "all_or_nothing",
  },
  multiple_choice: {
    label: "Pilihan ganda kompleks",
    description: "Lebih dari satu jawaban benar.",
    autoGraded: true,
    defaultScoringMode: "partial",
  },
  true_false: {
    label: "Benar / Salah",
    description: "Satu pernyataan, jawab benar atau salah.",
    autoGraded: true,
    defaultScoringMode: "all_or_nothing",
  },
  multiple_true_false: {
    label: "Benar / Salah majemuk",
    description: "Tabel beberapa pernyataan, masing-masing dijawab benar/salah (gaya AKM).",
    autoGraded: true,
    defaultScoringMode: "partial",
  },
  short_answer: {
    label: "Isian singkat",
    description: "Jawaban teks pendek, dicocokkan dengan daftar jawaban yang diterima.",
    autoGraded: true,
    defaultScoringMode: "all_or_nothing",
  },
  numeric: {
    label: "Isian angka",
    description: "Jawaban berupa angka dengan toleransi atau rentang.",
    autoGraded: true,
    defaultScoringMode: "all_or_nothing",
  },
  essay: {
    label: "Uraian",
    description: "Jawaban panjang, dinilai manual oleh korektor (opsional dengan rubrik).",
    autoGraded: false,
    defaultScoringMode: "partial",
  },
  matching: {
    label: "Menjodohkan",
    description: "Pasangkan item kiri dengan item kanan.",
    autoGraded: true,
    defaultScoringMode: "partial",
  },
  ordering: {
    label: "Mengurutkan",
    description: "Susun item ke urutan yang benar.",
    autoGraded: true,
    defaultScoringMode: "all_or_nothing",
  },
  fill_blanks: {
    label: "Isian rumpang",
    description: "Teks dengan beberapa rumpang: isian teks, angka, dropdown, atau seret kata.",
    autoGraded: true,
    defaultScoringMode: "partial",
  },
  categorization: {
    label: "Pengelompokan",
    description: "Seret item ke kategori yang tepat.",
    autoGraded: true,
    defaultScoringMode: "partial",
  },
  hotspot: {
    label: "Hotspot gambar",
    description: "Klik area yang benar pada gambar.",
    autoGraded: true,
    defaultScoringMode: "all_or_nothing",
  },
  hot_text: {
    label: "Pilih teks",
    description: "Pilih kata/kalimat yang benar di dalam bacaan.",
    autoGraded: true,
    defaultScoringMode: "partial",
  },
  matrix: {
    label: "Matriks / Likert",
    description: "Grid baris x kolom. Tanpa kunci = angket (tidak dinilai).",
    autoGraded: true,
    defaultScoringMode: "partial",
  },
  file_upload: {
    label: "Unggah berkas",
    description: "Peserta mengunggah berkas (foto, dokumen, rekaman), dinilai manual.",
    autoGraded: false,
    defaultScoringMode: "partial",
  },
};

/** UUID placeholder untuk aset contoh (ganti dengan aset yang sudah diunggah). */
export const PLACEHOLDER_ASSET_ID = "00000000-0000-4000-8000-000000000000";

/** Contoh soal untuk setiap jenis, dipakai sebagai template editor, seed, dan tes. */
export function questionTemplate(type: QuestionType): { definition: QuestionDefinition; scoring: ScoringInput } {
  const scoring: ScoringInput = { points: 1, mode: QUESTION_TYPE_META[type].defaultScoringMode };
  const definition = TEMPLATES[type]();
  return { definition, scoring };
}

const TEMPLATES: { [T in QuestionType]: () => Extract<QuestionDefinition, { type: T }> } = {
  single_choice: () => ({
    type: "single_choice",
    content: {
      prompt: "<p>Ibu kota Indonesia saat ini adalah …</p>",
      options: [
        { id: "A", content: "Jakarta" },
        { id: "B", content: "Bandung" },
        { id: "C", content: "Surabaya" },
        { id: "D", content: "Nusantara" },
      ],
      shuffleOptions: true,
    },
    answerKey: { correctOptionId: "A" },
  }),
  multiple_choice: () => ({
    type: "multiple_choice",
    content: {
      prompt: "<p>Manakah yang termasuk bilangan prima? (pilih semua yang benar)</p>",
      options: [
        { id: "A", content: "2" },
        { id: "B", content: "4" },
        { id: "C", content: "7" },
        { id: "D", content: "9" },
      ],
      shuffleOptions: true,
    },
    answerKey: { correctOptionIds: ["A", "C"] },
  }),
  true_false: () => ({
    type: "true_false",
    content: { prompt: "<p>Air mendidih pada suhu 100°C di permukaan laut.</p>" },
    answerKey: { value: true },
  }),
  multiple_true_false: () => ({
    type: "multiple_true_false",
    content: {
      prompt: "<p>Tentukan benar atau salah setiap pernyataan berikut.</p>",
      statements: [
        { id: "s1", content: "Matahari terbit dari timur." },
        { id: "s2", content: "Bulan adalah planet." },
        { id: "s3", content: "Bumi berbentuk bulat." },
      ],
      shuffleStatements: false,
    },
    answerKey: { values: { s1: true, s2: false, s3: true } },
  }),
  short_answer: () => ({
    type: "short_answer",
    content: { prompt: "<p>Siapa presiden pertama Republik Indonesia?</p>", maxLength: 100 },
    answerKey: {
      accepted: [
        { value: "Soekarno", mode: "exact", caseSensitive: false, normalizeWhitespace: true, ignorePunctuation: true, ignoreAccents: false, score: 1 },
        { value: "Sukarno", mode: "exact", caseSensitive: false, normalizeWhitespace: true, ignorePunctuation: true, ignoreAccents: false, score: 1 },
        { value: "Ir. Soekarno", mode: "exact", caseSensitive: false, normalizeWhitespace: true, ignorePunctuation: true, ignoreAccents: false, score: 1 },
      ],
    },
  }),
  numeric: () => ({
    type: "numeric",
    content: { prompt: "<p>Berapakah nilai \\(\\pi\\) sampai dua desimal?</p>", decimalPlaces: 2 },
    answerKey: { accepted: [{ value: 3.14, tolerance: 0.005, toleranceMode: "absolute", score: 1 }] },
  }),
  essay: () => ({
    type: "essay",
    content: { prompt: "<p>Jelaskan proses fotosintesis secara singkat.</p>", minWords: 30, maxWords: 300, richText: false },
    answerKey: {
      modelAnswer: "<p>Tumbuhan mengubah air dan CO₂ menjadi glukosa dan O₂ dengan bantuan cahaya matahari dan klorofil.</p>",
      rubric: [
        { id: "r1", criterion: "Menyebut bahan (air, CO₂)", maxPoints: 1 },
        { id: "r2", criterion: "Menyebut hasil (glukosa, O₂)", maxPoints: 1 },
        { id: "r3", criterion: "Menyebut peran cahaya & klorofil", maxPoints: 1 },
      ],
    },
  }),
  matching: () => ({
    type: "matching",
    content: {
      prompt: "<p>Jodohkan negara dengan ibu kotanya.</p>",
      left: [
        { id: "L1", content: "Jepang" },
        { id: "L2", content: "Prancis" },
        { id: "L3", content: "Mesir" },
      ],
      right: [
        { id: "R1", content: "Tokyo" },
        { id: "R2", content: "Paris" },
        { id: "R3", content: "Kairo" },
        { id: "R4", content: "Roma" },
      ],
      allowReuse: false,
      shuffle: true,
    },
    answerKey: {
      pairs: [
        { leftId: "L1", rightId: "R1" },
        { leftId: "L2", rightId: "R2" },
        { leftId: "L3", rightId: "R3" },
      ],
    },
  }),
  ordering: () => ({
    type: "ordering",
    content: {
      prompt: "<p>Urutkan dari yang terkecil.</p>",
      items: [
        { id: "a", content: "10" },
        { id: "b", content: "3" },
        { id: "c", content: "7" },
      ],
    },
    answerKey: { order: ["b", "c", "a"] },
  }),
  fill_blanks: () => ({
    type: "fill_blanks",
    content: {
      prompt: "<p>Lengkapi kalimat berikut.</p>",
      text: "<p>Indonesia merdeka pada tahun [[b1]]. Ibu kotanya adalah [[b2]], dan lagu kebangsaannya [[b3]].</p>",
      blanks: [
        { id: "b1", kind: "numeric", width: 6 },
        {
          id: "b2",
          kind: "dropdown",
          options: [
            { id: "o1", content: "Jakarta" },
            { id: "o2", content: "Medan" },
          ],
        },
        { id: "b3", kind: "word_bank" },
      ],
      wordBank: [
        { id: "w1", content: "Indonesia Raya" },
        { id: "w2", content: "Garuda Pancasila" },
      ],
      reuseWordBank: false,
    },
    answerKey: {
      blanks: {
        b1: { numeric: [{ value: 1945, tolerance: 0, toleranceMode: "absolute", score: 1 }] },
        b2: { optionIds: ["o1"] },
        b3: { optionIds: ["w1"] },
      },
    },
  }),
  categorization: () => ({
    type: "categorization",
    content: {
      prompt: "<p>Kelompokkan hewan berikut.</p>",
      items: [
        { id: "i1", content: "Kucing" },
        { id: "i2", content: "Elang" },
        { id: "i3", content: "Paus" },
        { id: "i4", content: "Merpati" },
      ],
      categories: [
        { id: "mamalia", content: "Mamalia" },
        { id: "unggas", content: "Burung" },
      ],
      shuffleItems: true,
    },
    answerKey: { mapping: { i1: "mamalia", i2: "unggas", i3: "mamalia", i4: "unggas" } },
  }),
  hotspot: () => ({
    type: "hotspot",
    content: {
      prompt: "<p>Klik letak Pulau Kalimantan pada peta.</p>",
      image: { src: `asset://${PLACEHOLDER_ASSET_ID}`, width: 800, height: 400, alt: "Peta Indonesia" },
      maxSelections: 1,
    },
    answerKey: { regions: [{ id: "kalimantan", shape: "rect", x: 0.3, y: 0.2, width: 0.2, height: 0.35 }] },
  }),
  hot_text: () => ({
    type: "hot_text",
    content: {
      prompt: "<p>Pilih semua kata kerja dalam kalimat berikut.</p>",
      segments: [
        { id: "t1", content: "Adik", selectable: true },
        { id: "t2", content: "makan", selectable: true },
        { id: "t3", content: "lalu", selectable: true },
        { id: "t4", content: "tidur", selectable: true },
        { id: "t5", content: ".", selectable: false },
      ],
    },
    answerKey: { correctSegmentIds: ["t2", "t4"] },
  }),
  matrix: () => ({
    type: "matrix",
    content: {
      prompt: "<p>Tentukan wujud zat berikut pada suhu ruang.</p>",
      rows: [
        { id: "r1", content: "Air" },
        { id: "r2", content: "Besi" },
        { id: "r3", content: "Oksigen" },
      ],
      columns: [
        { id: "padat", content: "Padat" },
        { id: "cair", content: "Cair" },
        { id: "gas", content: "Gas" },
      ],
      multiplePerRow: false,
    },
    answerKey: { correct: { r1: ["cair"], r2: ["padat"], r3: ["gas"] } },
  }),
  file_upload: () => ({
    type: "file_upload",
    content: {
      prompt: "<p>Foto hasil gambar kerja Anda lalu unggah di sini.</p>",
      accept: ["image/*", ".pdf"],
      maxFiles: 2,
      maxSizeMb: 10,
    },
    answerKey: { rubric: [{ id: "r1", criterion: "Kerapian & kelengkapan", maxPoints: 1 }] },
  }),
};
