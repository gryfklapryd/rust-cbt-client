import { useEffect, useRef, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";

export function Button({
  variant = "secondary",
  loading,
  className = "",
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "primary" | "secondary" | "danger" | "ghost"; loading?: boolean }) {
  return (
    <button type="button" {...rest} disabled={rest.disabled || loading} className={`btn btn-${variant} ${className}`}>
      {loading ? <span className="spinner" aria-hidden /> : null}
      {children}
    </button>
  );
}

export function Field({ label, hint, children }: { label: ReactNode; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      {children}
      {hint ? <span className="field-hint">{hint}</span> : null}
    </label>
  );
}

export const Input = (p: InputHTMLAttributes<HTMLInputElement>) => <input {...p} className={`input ${p.className ?? ""}`} />;

export function Alert({ tone = "danger", children }: { tone?: "danger" | "info" | "success" | "warning"; children: ReactNode }) {
  if (!children) return null;
  return <div className={`alert alert-${tone}`}>{children}</div>;
}

export function Modal({ open, title, children, footer, onClose }: { open: boolean; title: ReactNode; children: ReactNode; footer?: ReactNode; onClose?: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);
  return (
    <dialog
      ref={ref}
      className="modal"
      onCancel={(e) => {
        e.preventDefault();
        onClose?.();
      }}
    >
      {open ? (
        <>
          <h2>{title}</h2>
          <div className="modal-body">{children}</div>
          {footer ? <div className="modal-footer">{footer}</div> : null}
        </>
      ) : null}
    </dialog>
  );
}

export function Badge({ tone = "neutral", children }: { tone?: "neutral" | "success" | "warning" | "danger" | "info"; children: ReactNode }) {
  return <span className={`badge badge-${tone}`}>{children}</span>;
}

const STATUS: Record<string, [string, "neutral" | "success" | "warning" | "danger" | "info"]> = {
  in_progress: ["berlangsung", "info"],
  submitted: ["selesai", "success"],
  timed_out: ["waktu habis", "warning"],
  terminated: ["dihentikan", "danger"],
  received: ["diterima", "info"],
  processing: ["diproses", "info"],
  processed: ["selesai", "success"],
  failed: ["gagal", "danger"],
  sending: ["mengirim", "warning"],
  lost: ["tidak sampai", "danger"],
};
export function StatusBadge({ status }: { status: string }) {
  const [label, tone] = STATUS[status] ?? [status, "neutral"];
  return <Badge tone={tone}>{label}</Badge>;
}
