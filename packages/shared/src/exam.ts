import { z } from "zod";

/** Pengaturan ujian yang dijalankan oleh client desktop. */
export const ExamSettings = z.object({
  /** Acak urutan soal di setiap bagian (per peserta). */
  shuffleQuestions: z.boolean().default(false),
  /** Acak urutan opsi (bila jenis soal mengizinkan). */
  shuffleOptions: z.boolean().default(true),
  /** Peserta boleh kembali ke soal sebelumnya. */
  allowBackNavigation: z.boolean().default(true),
  /** Peserta boleh menandai soal "ragu-ragu". */
  allowFlagging: z.boolean().default(true),
  /** Otomatis kumpulkan ketika waktu habis. */
  autoSubmitOnTimeout: z.boolean().default(true),
  /** Tombol selesai baru aktif setelah X menit berjalan. */
  minTimeBeforeSubmitMinutes: z.number().int().min(0).default(0),
  /** Tampilkan skor (soal otomatis) ke peserta setelah selesai. */
  showScoreToParticipant: z.boolean().default(false),
  /** Kunci layar (fullscreen/kiosk) selama ujian. */
  lockdown: z.boolean().default(true),
  /** Jumlah pelanggaran (mis. keluar aplikasi) sebelum ujian dihentikan otomatis. 0 = tanpa batas. */
  maxViolations: z.number().int().min(0).default(0),
  /** Nilai kelulusan (skala 0-100). */
  passingScore: z.number().min(0).max(100).optional(),
});
export type ExamSettings = z.infer<typeof ExamSettings>;
export type ExamSettingsInput = z.input<typeof ExamSettings>;

export const StimulusSettings = z.object({
  /** Batas putar audio/video di stimulus (0 = tanpa batas). */
  mediaPlayLimit: z.number().int().min(0).default(0),
});
export type StimulusSettings = z.infer<typeof StimulusSettings>;
