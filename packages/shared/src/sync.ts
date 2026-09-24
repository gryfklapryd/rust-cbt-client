import { z } from "zod";
import { IsoDateTime, LocalId } from "./common.js";
import { ExamSettings, StimulusSettings } from "./exam.js";
import { QuestionType } from "./questions/types.js";

/**
 * Kontrak sinkronisasi server pusat <-> client desktop (titik ujian).
 *
 * Alur:
 *  1. POST /api/sync/auth                     -> token lokasi (JWT)
 *  2. GET  /api/sync/schedules                -> daftar jadwal + versi paket
 *  3. GET  /api/sync/schedules/:id/package    -> ExamPackage (tanpa kunci jawaban)
 *  4. GET  /api/sync/assets/:assetId          -> unduh media (gambar/audio/video)
 *  5. (ujian berlangsung offline)
 *  6. POST /api/sync/attachments              -> unggah berkas jawaban (soal unggah berkas)
 *  7. POST /api/sync/results                  -> ResultsBatch (idempoten per batchId)
 *  8. GET  /api/sync/results/:batchId         -> status pemrosesan batch
 */
export const PACKAGE_FORMAT_VERSION = 1;

// ---------------------------------------------------------------------------
// Paket ujian (server -> desktop)
// ---------------------------------------------------------------------------

export const PackageQuestion = z.object({
  id: z.uuid(),
  type: QuestionType,
  /** Bagian publik soal (lihat `QUESTION_SCHEMAS[type].content`). */
  content: z.unknown(),
  /** Skor maksimum, untuk ditampilkan. */
  points: z.number(),
  stimulusId: z.uuid().nullable(),
  version: z.number().int(),
});
export type PackageQuestion = z.infer<typeof PackageQuestion>;

export const PackageStimulus = z.object({
  id: z.uuid(),
  title: z.string(),
  content: z.string(),
  settings: StimulusSettings,
});
export type PackageStimulus = z.infer<typeof PackageStimulus>;

export const PackageSection = z.object({
  id: z.uuid(),
  title: z.string(),
  instructions: z.string().nullable(),
  order: z.number().int(),
  /** Batas waktu bagian (menit), null = ikut durasi ujian. */
  durationMinutes: z.number().int().nullable(),
  /** Ambil acak N soal dari bagian ini per peserta. null = semua. */
  pickCount: z.number().int().nullable(),
  shuffleQuestions: z.boolean(),
  questionIds: z.array(z.uuid()),
});
export type PackageSection = z.infer<typeof PackageSection>;

export const PackageParticipant = z.object({
  id: z.uuid(),
  number: z.string(),
  name: z.string(),
  groupName: z.string().nullable(),
  gender: z.enum(["L", "P"]).nullable(),
  birthDate: z.string().nullable(),
  /** Hash argon2id (format PHC) untuk verifikasi login offline di desktop. */
  passwordHash: z.string().nullable(),
  photoAssetId: z.uuid().nullable(),
});
export type PackageParticipant = z.infer<typeof PackageParticipant>;

export const PackageAsset = z.object({
  id: z.uuid(),
  filename: z.string(),
  mime: z.string(),
  size: z.number().int(),
  sha256: z.string(),
});
export type PackageAsset = z.infer<typeof PackageAsset>;

export const ExamPackage = z.object({
  formatVersion: z.literal(PACKAGE_FORMAT_VERSION),
  packageId: z.uuid(),
  version: z.number().int(),
  builtAt: IsoDateTime,
  site: z.object({ id: z.uuid(), code: z.string(), name: z.string() }),
  schedule: z.object({
    id: z.uuid(),
    name: z.string(),
    startAt: IsoDateTime,
    endAt: IsoDateTime,
    /** Batas terlambat masuk (menit setelah startAt). null = sampai endAt. */
    lateEntryMinutes: z.number().int().nullable(),
    /** Hash argon2id token sesi yang dibagikan pengawas saat ujian dimulai. */
    accessTokenHash: z.string().nullable(),
  }),
  exam: z.object({
    id: z.uuid(),
    code: z.string(),
    title: z.string(),
    description: z.string().nullable(),
    instructions: z.string().nullable(),
    durationMinutes: z.number().int(),
    settings: ExamSettings,
    sections: z.array(PackageSection),
  }),
  questions: z.array(PackageQuestion),
  stimuli: z.array(PackageStimulus),
  participants: z.array(PackageParticipant),
  assets: z.array(PackageAsset),
});
export type ExamPackage = z.infer<typeof ExamPackage>;

// ---------------------------------------------------------------------------
// Hasil ujian (desktop -> server)
// ---------------------------------------------------------------------------

export const ATTEMPT_STATUSES = ["in_progress", "submitted", "timed_out", "terminated"] as const;
export const AttemptStatus = z.enum(ATTEMPT_STATUSES);
export type AttemptStatus = z.infer<typeof AttemptStatus>;

export const AnswerUpload = z.object({
  questionId: z.uuid(),
  /** Bentuk sesuai `QUESTION_SCHEMAS[type].response`. Divalidasi saat penilaian. */
  response: z.unknown(),
  answeredAt: IsoDateTime.nullable().optional(),
  timeSpentSeconds: z.number().int().min(0).optional(),
  flagged: z.boolean().optional(),
  /** Berapa kali jawaban diubah. */
  changeCount: z.number().int().min(0).optional(),
});
export type AnswerUpload = z.infer<typeof AnswerUpload>;

export const AttemptEvent = z.object({
  /** mis. start, resume, focus_lost, focus_gained, violation, network_change, submit, proctor_note */
  type: z.string().min(1).max(64),
  at: IsoDateTime,
  data: z.record(z.string(), z.unknown()).optional(),
});
export type AttemptEvent = z.infer<typeof AttemptEvent>;

export const AttemptUpload = z.object({
  /** UUID dibuat client saat peserta memulai ujian; kunci idempoten. */
  attemptId: z.uuid(),
  packageId: z.uuid(),
  scheduleId: z.uuid(),
  participantId: z.uuid(),
  status: AttemptStatus,
  /**
   * Nomor urut sinkronisasi attempt ini (naik setiap kali dikirim ulang).
   * Server hanya menerima data dengan sequence >= yang tersimpan.
   */
  sequence: z.number().int().min(0),
  startedAt: IsoDateTime,
  finishedAt: IsoDateTime.nullable().optional(),
  /** Urutan soal yang benar-benar ditampilkan ke peserta (setelah acak/pick). */
  questionOrder: z.array(z.uuid()).optional(),
  /** Urutan opsi per soal setelah diacak, untuk audit. */
  optionOrders: z.record(z.uuid(), z.array(LocalId)).optional(),
  answers: z.array(AnswerUpload).max(2_000),
  events: z.array(AttemptEvent).max(10_000).default([]),
  client: z
    .object({
      deviceId: z.string().max(200).optional(),
      appVersion: z.string().max(50).optional(),
      hostname: z.string().max(200).optional(),
    })
    .optional(),
});
export type AttemptUpload = z.infer<typeof AttemptUpload>;
export type AttemptUploadInput = z.input<typeof AttemptUpload>;

export const ResultsBatch = z.object({
  /** UUID dibuat client; mengirim ulang batch yang sama aman (idempoten). */
  batchId: z.uuid(),
  attempts: z.array(AttemptUpload).min(1).max(500),
});
export type ResultsBatch = z.infer<typeof ResultsBatch>;
export type ResultsBatchInput = z.input<typeof ResultsBatch>;

export const BATCH_STATUSES = ["received", "processing", "processed", "failed"] as const;
export const BatchStatus = z.enum(BATCH_STATUSES);
export type BatchStatus = z.infer<typeof BatchStatus>;

export const ResultsBatchAck = z.object({
  batchId: z.uuid(),
  status: BatchStatus,
  attemptCount: z.number().int(),
  receivedAt: IsoDateTime,
  processedAt: IsoDateTime.nullable(),
  error: z.string().nullable(),
  /** Hasil per attempt setelah diproses. */
  attempts: z
    .array(
      z.object({
        attemptId: z.uuid(),
        accepted: z.boolean(),
        reason: z.string().nullable(),
      }),
    )
    .optional(),
});
export type ResultsBatchAck = z.infer<typeof ResultsBatchAck>;

export const SyncAuthRequest = z.object({
  siteCode: z.string().min(1).max(64),
  secret: z.string().min(1).max(200),
  deviceId: z.string().max(200).optional(),
  appVersion: z.string().max(50).optional(),
});
export type SyncAuthRequest = z.infer<typeof SyncAuthRequest>;

export const HeartbeatRequest = z.object({
  deviceId: z.string().max(200).optional(),
  appVersion: z.string().max(50).optional(),
  /** Ringkasan status di lokasi, bebas (jumlah peserta aktif, jadwal berjalan, dll). */
  status: z.record(z.string(), z.unknown()).optional(),
});
export type HeartbeatRequest = z.infer<typeof HeartbeatRequest>;
