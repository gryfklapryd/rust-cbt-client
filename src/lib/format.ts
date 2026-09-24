const dtf = new Intl.DateTimeFormat("id-ID", { dateStyle: "medium", timeStyle: "short" });
const tf = new Intl.DateTimeFormat("id-ID", { timeStyle: "short" });

export const fmtDateTime = (v: string | null | undefined) => (v ? dtf.format(new Date(v)) : "-");
export const fmtTime = (v: string | null | undefined) => (v ? tf.format(new Date(v)) : "-");

export function fmtDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(sec)}` : `${pad(m)}:${pad(sec)}`;
}

export function readFileBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ""));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}
