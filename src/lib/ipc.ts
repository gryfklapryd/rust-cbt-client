import { invoke } from "@tauri-apps/api/core";
import type { QuestionType } from "@cbt/shared";

/** Pesan error dari backend Rust dikirim sebagai string. */
export function errorText(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

export interface ConfigView {
  serverUrl: string | null;
  siteCode: string | null;
  hasSecret: boolean;
  hasPin: boolean;
  deviceId: string;
  deviceName: string | null;
  autoSync: boolean;
  configured: boolean;
}

export interface ConfigInput {
  serverUrl: string;
  siteCode: string;
  secret?: string | null;
  operatorPin?: string | null;
  deviceName?: string | null;
  autoSync?: boolean;
}

export interface AttemptCounts {
  total: number;
  inProgress: number;
  finished: number;
  unsynced: number;
  syncErrors: number;
}

export interface LocalSchedule {
  scheduleId: string;
  name: string;
  examCode: string;
  examTitle: string;
  durationMinutes: number;
  startAt: string;
  endAt: string;
  packageId: string;
  packageVersion: number;
  checksum: string;
  downloadedAt: string;
  participantCount: number;
  questionCount: number;
  assetCount: number;
  assetsMissing: number;
  requiresToken: boolean;
  attempts: AttemptCounts;
}

export interface RemoteSchedule {
  id: string;
  name: string;
  startAt: string;
  endAt: string;
  status: string;
  exam: { id: string; code: string; title: string; durationMinutes: number };
  package: { id: string; version: number; checksum: string | null; size: number | null; builtAt: string | null; assetCount: number | null } | null;
}

export interface ExamSettings {
  shuffleQuestions: boolean;
  shuffleOptions: boolean;
  allowBackNavigation: boolean;
  allowFlagging: boolean;
  autoSubmitOnTimeout: boolean;
  minTimeBeforeSubmitMinutes: number;
  showScoreToParticipant: boolean;
  lockdown: boolean;
  maxViolations: number;
  passingScore: number | null;
}

export interface SessionQuestion {
  id: string;
  type: QuestionType;
  content: unknown;
  points: number;
  stimulusId: string | null;
  version: number;
}

export interface ExamSession {
  attemptId: string;
  status: "in_progress" | "submitted" | "timed_out" | "terminated";
  participant: { id: string; number: string; name: string; groupName: string | null; photoAssetId: string | null };
  exam: { code: string; title: string; instructions: string | null; durationMinutes: number; settings: ExamSettings };
  scheduleName: string;
  sections: { id: string; title: string; instructions: string | null; questionIds: string[] }[];
  questions: Record<string, SessionQuestion>;
  stimuli: Record<string, { id: string; title: string; content: string; settings: { mediaPlayLimit?: number } }>;
  optionOrders: Record<string, Record<string, string[]>>;
  answers: Record<string, { response: unknown; flagged: boolean; timeSpent: number }>;
  startedAt: string;
  finishedAt: string | null;
  deadline: string;
  now: string;
  remainingSeconds: number;
  canSubmitAt: string;
  violationCount: number;
  currentIndex: number;
}

export interface AttemptState {
  status: ExamSession["status"];
  remainingSeconds: number;
  violationCount: number;
}

export interface SyncStatus {
  counts: AttemptCounts;
  lastSyncAt: string | null;
  lastError: string | null;
  autoSync: boolean;
  batches: { id: string; createdAt: string; status: string; attemptCount: number; response: { attempts?: { attemptId: string; accepted: boolean; reason: string | null }[] } | null }[];
}

export interface AttemptRow {
  id: string;
  participantNumber: string;
  participantName: string;
  status: string;
  startedAt: string;
  finishedAt: string | null;
  answered: number;
  violationCount: number;
  synced: boolean;
  syncError: string | null;
}

export const ipc = {
  getConfig: () => invoke<ConfigView>("get_config"),
  saveConfig: (input: ConfigInput) => invoke<ConfigView>("save_config", { input }),
  unlockOperator: (pin: string) => invoke<boolean>("unlock_operator", { pin }),
  lockOperator: () => invoke<void>("lock_operator"),
  testConnection: () => invoke<{ site: { code: string; name: string }; serverTime: string }>("test_connection"),
  remoteSchedules: () => invoke<RemoteSchedule[]>("remote_schedules"),
  downloadSchedule: (scheduleId: string) =>
    invoke<{ scheduleId: string; packageVersion: number; updated: boolean; assetsDownloaded: number; assetsTotal: number }>(
      "download_schedule",
      { scheduleId },
    ),
  localSchedules: () => invoke<LocalSchedule[]>("local_schedules"),
  syncStatus: () => invoke<SyncStatus>("sync_status"),
  syncNow: () => invoke<{ attachmentsUploaded: number; attemptsSent: number; batches: string[] }>("sync_now"),
  listAttempts: (scheduleId: string) => invoke<AttemptRow[]>("list_attempts", { scheduleId }),
  resetAttempt: (attemptId: string) => invoke<void>("reset_attempt", { attemptId }),
  login: (request: { scheduleId: string; number: string; password: string; token?: string }) =>
    invoke<ExamSession>("participant_login", { request }),
  getSession: (attemptId: string) => invoke<ExamSession>("get_session", { attemptId }),
  saveAnswer: (request: {
    attemptId: string;
    questionId: string;
    response: unknown;
    flagged: boolean;
    timeSpentDelta: number;
    currentIndex?: number;
  }) => invoke<AttemptState>("save_answer", { request }),
  logEvent: (attemptId: string, kind: string, data?: Record<string, unknown>) =>
    invoke<AttemptState>("log_event", { attemptId, kind, data: data ?? null }),
  submit: (attemptId: string, manual: boolean) => invoke<AttemptState>("submit_exam", { attemptId, manual }),
  saveAttachment: (attemptId: string, questionId: string, name: string, mime: string, dataBase64: string) =>
    invoke<{ attachmentId: string; name: string; size: number; mime: string }>("save_attachment", {
      attemptId,
      questionId,
      name,
      mime,
      dataBase64,
    }),
  setExamMode: (active: boolean, lockdown: boolean) => invoke<void>("set_exam_mode", { active, lockdown }),
  quitApp: () => invoke<void>("quit_app"),
  appInfo: () => invoke<{ version: string; dataDir: string }>("app_info"),
};
