import { invoke } from "@tauri-apps/api/core";
import type { QuestionType } from "@cbt/shared";

/** Pesan error dari backend Rust dikirim sebagai string. */
export function errorText(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

export type AppMode = "server" | "participant";

export interface ConfigView {
  mode: AppMode | null;
  deviceId: string;
  deviceName: string | null;
  configured: boolean;
  serverUrl: string | null;
  siteCode: string | null;
  hasSecret: boolean;
  autoSync: boolean;
  lanPort: number;
  lanUrl: string | null;
}

export interface ServerConfigInput {
  serverUrl: string;
  siteCode: string;
  secret?: string | null;
  deviceName?: string | null;
  autoSync?: boolean;
  lanPort?: number | null;
}

export interface ParticipantConfigInput {
  lanUrl: string;
  deviceName?: string | null;
}

export interface Proctor {
  id: string;
  username: string;
  name: string;
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
  accessToken: string | null;
  attempts: AttemptCounts;
}

/** Jadwal yang ditawarkan server lokal ke PC peserta. */
export interface ExamSchedule {
  scheduleId: string;
  name: string;
  examCode: string;
  examTitle: string;
  durationMinutes: number;
  startAt: string;
  endAt: string;
  requiresToken: boolean;
  ready: boolean;
}

export interface RemoteSchedule {
  id: string;
  name: string;
  startAt: string;
  endAt: string;
  status: string;
  accessToken: string | null;
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
  deadline: string;
  /** false bila server lokal sedang tidak terjangkau (jawaban diantrekan di PC). */
  connected: boolean;
  /** Perubahan yang menunggu dikirim ke server lokal. */
  pending: number;
}

export interface SyncStatus {
  counts: AttemptCounts;
  lastSyncAt: string | null;
  lastError: string | null;
  autoSync: boolean;
  proctorLogPending: number;
  batches: { id: string; createdAt: string; status: string; attemptCount: number; response: { attempts?: { attemptId: string; accepted: boolean; reason: string | null }[] } | null }[];
}

export interface MonitorRow {
  participantId: string;
  number: string;
  name: string;
  groupName: string | null;
  attemptId: string | null;
  status: "not_started" | ExamSession["status"];
  deviceId: string | null;
  deviceName: string | null;
  deviceLastSeen: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  deadline: string | null;
  remainingSeconds: number;
  answered: number;
  questionCount: number;
  violationCount: number;
  synced: boolean;
  syncError: string | null;
}

export interface Device {
  id: string;
  name: string;
  status: "pending" | "approved" | "revoked";
  pairingCode: string;
  appVersion: string | null;
  ip: string | null;
  createdAt: string;
  approvedAt: string | null;
  approvedBy: string | null;
  lastSeenAt: string | null;
}

export interface ProctorLogRow {
  id: string;
  at: string;
  username: string;
  action: string;
  attemptId: string | null;
  participantId: string | null;
  data: Record<string, unknown> | null;
  synced: boolean;
}

export interface LanInfo {
  port: number;
  running: boolean;
  error: string | null;
  addresses: string[];
  devicesOnline: number;
}

export interface LinkStatus {
  connected: boolean;
  pending: number;
  error: string | null;
}

export const ipc = {
  // umum
  getConfig: () => invoke<ConfigView>("get_config"),
  saveServerConfig: (input: ServerConfigInput) => invoke<ConfigView>("save_server_config", { input }),
  saveParticipantConfig: (input: ParticipantConfigInput) => invoke<ConfigView>("save_participant_config", { input }),
  proctorLogin: (username: string, password: string) => invoke<Proctor>("proctor_login", { username, password }),
  proctorLogout: () => invoke<void>("proctor_logout"),
  currentProctor: () => invoke<Proctor | null>("current_proctor"),
  setExamMode: (active: boolean, lockdown: boolean) => invoke<void>("set_exam_mode", { active, lockdown }),
  quitApp: () => invoke<void>("quit_app"),
  appInfo: () => invoke<{ version: string; dataDir: string }>("app_info"),

  // server lokal
  syncProctors: () => invoke<{ count: number }>("sync_proctors"),
  proctorCount: () => invoke<number>("proctor_count"),
  testConnection: () => invoke<{ site: { code: string; name: string }; serverTime: string }>("test_connection"),
  remoteSchedules: () => invoke<RemoteSchedule[]>("remote_schedules"),
  downloadSchedule: (scheduleId: string) =>
    invoke<{ scheduleId: string; packageVersion: number; updated: boolean; assetsDownloaded: number; assetsTotal: number }>(
      "download_schedule",
      { scheduleId },
    ),
  localSchedules: () => invoke<LocalSchedule[]>("local_schedules"),
  syncStatus: () => invoke<SyncStatus>("sync_status"),
  syncNow: () => invoke<{ attachmentsUploaded: number; attemptsSent: number; batches: string[]; proctorLogSent: number }>("sync_now"),
  monitor: (scheduleId: string) => invoke<MonitorRow[]>("monitor", { scheduleId }),
  releaseDevice: (attemptId: string) => invoke<void>("release_device", { attemptId }),
  extendTime: (attemptId: string, minutes: number) => invoke<AttemptState>("extend_time", { attemptId, minutes }),
  terminateAttempt: (attemptId: string, reason?: string) => invoke<void>("terminate_attempt", { attemptId, reason: reason ?? null }),
  unlockAttempt: (attemptId: string, extraMinutes?: number) =>
    invoke<AttemptState>("unlock_attempt", { attemptId, extraMinutes: extraMinutes ?? null }),
  deleteAttempt: (attemptId: string) => invoke<void>("delete_attempt", { attemptId }),
  listDevices: () => invoke<Device[]>("list_devices"),
  setDeviceStatus: (deviceId: string, approved: boolean) => invoke<void>("set_device_status", { deviceId, approved }),
  deleteDevice: (deviceId: string) => invoke<void>("delete_device", { deviceId }),
  proctorLog: () => invoke<ProctorLogRow[]>("proctor_log"),
  lanInfo: () => invoke<LanInfo>("lan_info"),

  // PC peserta
  pairDevice: () => invoke<{ status: "pending" | "approved" | "revoked"; pairingCode: string }>("pair_device"),
  linkStatus: () => invoke<LinkStatus>("link_status"),
  examSchedules: () => invoke<ExamSchedule[]>("exam_schedules"),
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
  attemptState: (attemptId: string) => invoke<AttemptState>("attempt_state", { attemptId }),
  saveAttachment: (attemptId: string, questionId: string, name: string, mime: string, dataBase64: string) =>
    invoke<{ attachmentId: string; name: string; size: number; mime: string }>("save_attachment", {
      attemptId,
      questionId,
      name,
      mime,
      dataBase64,
    }),
};
