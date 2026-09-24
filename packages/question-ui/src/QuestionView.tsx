import { useLayoutEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import {
  FILL_BLANK_MARKER,
  type Choice,
  type QuestionContent,
  type QuestionResponse,
  type QuestionType,
} from "@cbt/shared";
import { Html, resolveAssetUris, sanitizeHtml, type AssetResolver } from "./Html.js";

export interface UploadedFile {
  attachmentId: string;
  name: string;
  size: number;
  mime: string;
}

export interface QuestionViewProps {
  type: QuestionType;
  /** Bagian publik soal (`content`). */
  content: unknown;
  /** Jawaban saat ini (bentuk sesuai jenis soal) atau null. */
  value: unknown;
  onChange?: (value: unknown) => void;
  readOnly?: boolean;
  resolveAsset?: AssetResolver;
  /**
   * Urutan opsi per daftar (setelah diacak oleh client ujian). Kunci: "options",
   * "statements", "left", "right", "items". Tanpa ini, urutan asli yang dipakai.
   */
  order?: Partial<Record<"options" | "statements" | "left" | "right" | "items", string[]>>;
  /** Unggah berkas untuk soal `file_upload`. Tanpa ini, berkas hanya dicatat (mode pratinjau). */
  onUploadFile?: (file: File) => Promise<UploadedFile>;
}

type Props<T extends QuestionType> = Omit<QuestionViewProps, "type" | "content" | "value" | "onChange"> & {
  content: QuestionContent<T>;
  value: QuestionResponse<T> | null;
  onChange: (value: QuestionResponse<T>) => void;
};

const LETTERS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

function ordered<T extends { id: string }>(items: T[], order?: string[]): T[] {
  if (!order?.length) return items;
  const byId = new Map(items.map((i) => [i.id, i]));
  const out = order.map((id) => byId.get(id)).filter((x): x is T => !!x);
  // Tambahkan item yang tidak ada di `order` (jaga-jaga).
  for (const i of items) if (!order.includes(i.id)) out.push(i);
  return out;
}

/** Tampilkan & jawab satu soal. Komponen terkontrol: simpan `value` di state pemanggil. */
export function QuestionView(props: QuestionViewProps) {
  const noop = () => {};
  // Konten diasumsikan sudah valid terhadap skema jenis soalnya (divalidasi di server / editor).
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const common: any = { ...props, onChange: props.readOnly ? noop : (props.onChange ?? noop) };
  const content = props.content as { prompt?: string };
  let body: ReactNode;
  switch (props.type) {
    case "single_choice":
      body = <SingleChoice {...common} />;
      break;
    case "multiple_choice":
      body = <MultipleChoice {...common} />;
      break;
    case "true_false":
      body = <TrueFalse {...common} />;
      break;
    case "multiple_true_false":
      body = <MultipleTrueFalse {...common} />;
      break;
    case "short_answer":
      body = <ShortAnswer {...common} />;
      break;
    case "numeric":
      body = <Numeric {...common} />;
      break;
    case "essay":
      body = <Essay {...common} />;
      break;
    case "matching":
      body = <Matching {...common} />;
      break;
    case "ordering":
      body = <Ordering {...common} />;
      break;
    case "fill_blanks":
      body = <FillBlanks {...common} />;
      break;
    case "categorization":
      body = <Categorization {...common} />;
      break;
    case "hotspot":
      body = <Hotspot {...common} />;
      break;
    case "hot_text":
      body = <HotText {...common} />;
      break;
    case "matrix":
      body = <Matrix {...common} />;
      break;
    case "file_upload":
      body = <FileUpload {...common} />;
      break;
    default:
      body = <p className="cbtq-error">Jenis soal tidak dikenal: {String(props.type)}</p>;
  }
  return (
    <div className={`cbtq cbtq-${props.type}${props.readOnly ? " cbtq-readonly" : ""}`}>
      {content.prompt && props.type !== "fill_blanks" ? (
        <Html className="cbtq-prompt" html={content.prompt} resolveAsset={props.resolveAsset} />
      ) : null}
      {body}
    </div>
  );
}

// ---------------------------------------------------------------------------

function ChoiceLabel({ choice, index, resolveAsset }: { choice: Choice; index: number; resolveAsset?: AssetResolver }) {
  return (
    <>
      <span className="cbtq-letter">{LETTERS[index]}</span>
      <Html as="span" className="cbtq-choice-content" html={choice.content} resolveAsset={resolveAsset} />
    </>
  );
}

function SingleChoice({ content, value, onChange, readOnly, resolveAsset, order }: Props<"single_choice">) {
  const options = ordered(content.options, order?.options);
  return (
    <div className="cbtq-options" role="radiogroup">
      {options.map((o, i) => {
        const checked = value?.optionId === o.id;
        return (
          <button
            type="button"
            key={o.id}
            role="radio"
            aria-checked={checked}
            disabled={readOnly}
            className={`cbtq-option${checked ? " is-selected" : ""}`}
            onClick={() => onChange({ optionId: checked ? null : o.id })}
          >
            <ChoiceLabel choice={o} index={i} resolveAsset={resolveAsset} />
          </button>
        );
      })}
    </div>
  );
}

function MultipleChoice({ content, value, onChange, readOnly, resolveAsset, order }: Props<"multiple_choice">) {
  const options = ordered(content.options, order?.options);
  const selected = new Set(value?.optionIds ?? []);
  const max = content.maxSelections;
  return (
    <>
      {max ? <p className="cbtq-hint">Pilih paling banyak {max} jawaban.</p> : <p className="cbtq-hint">Pilih semua jawaban yang benar.</p>}
      <div className="cbtq-options">
        {options.map((o, i) => {
          const checked = selected.has(o.id);
          return (
            <button
              type="button"
              key={o.id}
              role="checkbox"
              aria-checked={checked}
              disabled={readOnly || (!checked && !!max && selected.size >= max)}
              className={`cbtq-option cbtq-check${checked ? " is-selected" : ""}`}
              onClick={() => {
                const next = new Set(selected);
                if (checked) next.delete(o.id);
                else next.add(o.id);
                onChange({ optionIds: content.options.map((x) => x.id).filter((id) => next.has(id)) });
              }}
            >
              <ChoiceLabel choice={o} index={i} resolveAsset={resolveAsset} />
            </button>
          );
        })}
      </div>
    </>
  );
}

function TrueFalse({ content, value, onChange, readOnly }: Props<"true_false">) {
  const labels = { true: content.labels?.true ?? "Benar", false: content.labels?.false ?? "Salah" };
  return (
    <div className="cbtq-options cbtq-inline">
      {([true, false] as const).map((v) => (
        <button
          type="button"
          key={String(v)}
          disabled={readOnly}
          className={`cbtq-option${value?.value === v ? " is-selected" : ""}`}
          onClick={() => onChange({ value: value?.value === v ? null : v })}
        >
          {v ? labels.true : labels.false}
        </button>
      ))}
    </div>
  );
}

function MultipleTrueFalse({ content, value, onChange, readOnly, resolveAsset, order }: Props<"multiple_true_false">) {
  const labels = { true: content.labels?.true ?? "Benar", false: content.labels?.false ?? "Salah" };
  const statements = ordered(content.statements, order?.statements);
  const values = value?.values ?? {};
  return (
    <table className="cbtq-table">
      <thead>
        <tr>
          <th>Pernyataan</th>
          <th>{labels.true}</th>
          <th>{labels.false}</th>
        </tr>
      </thead>
      <tbody>
        {statements.map((s) => (
          <tr key={s.id}>
            <td>
              <Html html={s.content} resolveAsset={resolveAsset} />
            </td>
            {([true, false] as const).map((v) => (
              <td key={String(v)} className="cbtq-center">
                <input
                  type="radio"
                  name={`mtf-${s.id}`}
                  aria-label={`${v ? labels.true : labels.false}`}
                  disabled={readOnly}
                  checked={values[s.id] === v}
                  onChange={() => onChange({ values: { ...values, [s.id]: v } })}
                />
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function ShortAnswer({ content, value, onChange, readOnly }: Props<"short_answer">) {
  return (
    <input
      className="cbtq-input"
      type="text"
      maxLength={content.maxLength}
      placeholder={content.placeholder ?? "Tulis jawaban…"}
      readOnly={readOnly}
      value={value?.text ?? ""}
      onChange={(e) => onChange({ text: e.target.value })}
    />
  );
}

function Numeric({ content, value, onChange, readOnly }: Props<"numeric">) {
  return (
    <div className="cbtq-numeric">
      <input
        className="cbtq-input"
        type="text"
        inputMode="decimal"
        placeholder="0"
        readOnly={readOnly}
        value={value?.value === null || value?.value === undefined ? "" : String(value.value)}
        onChange={(e) => onChange({ value: e.target.value === "" ? null : e.target.value })}
      />
      {content.unit ? <span className="cbtq-unit">{content.unit}</span> : null}
    </div>
  );
}

function countWords(s: string) {
  const t = s.trim();
  return t ? t.split(/\s+/).length : 0;
}

function Essay({ content, value, onChange, readOnly }: Props<"essay">) {
  const text = value?.text ?? "";
  const words = countWords(text);
  const tooFew = content.minWords !== undefined && words > 0 && words < content.minWords;
  const tooMany = content.maxWords !== undefined && words > content.maxWords;
  return (
    <div>
      <textarea
        className="cbtq-textarea"
        rows={10}
        readOnly={readOnly}
        value={text}
        placeholder="Tulis jawaban uraian…"
        onChange={(e) => onChange({ text: e.target.value })}
      />
      <div className={`cbtq-hint${tooFew || tooMany ? " cbtq-warn" : ""}`}>
        {words} kata
        {content.minWords !== undefined ? ` · min ${content.minWords}` : ""}
        {content.maxWords !== undefined ? ` · maks ${content.maxWords}` : ""}
      </div>
    </div>
  );
}

function Matching({ content, value, onChange, readOnly, resolveAsset, order }: Props<"matching">) {
  const left = ordered(content.left, order?.left);
  const right = ordered(content.right, order?.right);
  const pairs = value?.pairs ?? [];
  const current = (leftId: string) => pairs.find((p) => p.leftId === leftId)?.rightId ?? "";
  const used = new Set(pairs.map((p) => p.rightId));
  const set = (leftId: string, rightId: string) => {
    const next = pairs.filter((p) => p.leftId !== leftId);
    if (rightId) next.push({ leftId, rightId });
    onChange({ pairs: next });
  };
  return (
    <div className="cbtq-matching">
      <ol className="cbtq-right-list">
        {right.map((r, i) => (
          <li key={r.id}>
            <span className="cbtq-letter">{LETTERS[i]}</span>
            <Html as="span" html={r.content} resolveAsset={resolveAsset} />
          </li>
        ))}
      </ol>
      <table className="cbtq-table">
        <tbody>
          {left.map((l) => (
            <tr key={l.id}>
              <td>
                <Html html={l.content} resolveAsset={resolveAsset} />
              </td>
              <td className="cbtq-select-cell">
                <select className="cbtq-select" disabled={readOnly} value={current(l.id)} onChange={(e) => set(l.id, e.target.value)}>
                  <option value="">Pilih...</option>
                  {right.map((r, i) => (
                    <option key={r.id} value={r.id} disabled={!content.allowReuse && used.has(r.id) && current(l.id) !== r.id}>
                      {LETTERS[i]}
                    </option>
                  ))}
                </select>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Ordering({ content, value, onChange, readOnly, resolveAsset, order }: Props<"ordering">) {
  const initial = ordered(content.items, order?.items).map((i) => i.id);
  const current = value?.order?.length ? value.order : initial;
  const byId = new Map(content.items.map((i) => [i.id, i]));
  const move = (idx: number, delta: number) => {
    const next = [...current];
    const j = idx + delta;
    if (j < 0 || j >= next.length) return;
    [next[idx], next[j]] = [next[j]!, next[idx]!];
    onChange({ order: next });
  };
  return (
    <ol className="cbtq-ordering">
      {current.map((id, idx) => (
        <li key={id} className="cbtq-order-item">
          <span className="cbtq-order-num">{idx + 1}</span>
          <Html as="span" className="cbtq-grow" html={byId.get(id)?.content ?? id} resolveAsset={resolveAsset} />
          {!readOnly ? (
            <span className="cbtq-order-actions">
              <button type="button" aria-label="Naik" disabled={idx === 0} onClick={() => move(idx, -1)}>
                ▲
              </button>
              <button type="button" aria-label="Turun" disabled={idx === current.length - 1} onClick={() => move(idx, 1)}>
                ▼
              </button>
            </span>
          ) : null}
        </li>
      ))}
      {!value?.order?.length && !readOnly ? <p className="cbtq-hint">Ubah urutan dengan tombol ▲ ▼.</p> : null}
    </ol>
  );
}

function FillBlanks({ content, value, onChange, readOnly, resolveAsset }: Props<"fill_blanks">) {
  const values = value?.values ?? {};
  const set = (id: string, v: string) => onChange({ values: { ...values, [id]: v === "" ? null : v } });
  const usedWords = new Set(Object.values(values).filter(Boolean) as string[]);

  // Penanda [[id]] diganti elemen slot (id blank dijamin [A-Za-z0-9_-]), lalu input
  // dirender ke slot tersebut lewat portal sehingga struktur HTML teks tetap utuh.
  const html = useMemo(
    () =>
      resolveAssetUris(sanitizeHtml(content.text), resolveAsset).replace(
        FILL_BLANK_MARKER,
        (_m, id: string) => `<span class="cbtq-blank-slot" data-blank="${id}"></span>`,
      ),
    [content.text, resolveAsset],
  );
  const ref = useRef<HTMLDivElement>(null);
  const [slots, setSlots] = useState<[string, HTMLElement][]>([]);
  useLayoutEffect(() => {
    const found: [string, HTMLElement][] = [];
    ref.current?.querySelectorAll<HTMLElement>("[data-blank]").forEach((el) => found.push([el.dataset.blank!, el]));
    setSlots(found);
  }, [html]);

  const renderBlank = (id: string) => {
    const blank = content.blanks.find((b) => b.id === id);
    if (!blank) return <span>[[{id}]]</span>;
    const v = values[blank.id] ?? "";
    if (blank.kind === "dropdown" || blank.kind === "word_bank") {
      const opts = blank.kind === "dropdown" ? (blank.options ?? []) : (content.wordBank ?? []);
      return (
        <select className="cbtq-select cbtq-blank" disabled={readOnly} value={v} aria-label={`Rumpang ${blank.id}`} onChange={(e) => set(blank.id, e.target.value)}>
          <option value="">…</option>
          {opts.map((o) => (
            <option key={o.id} value={o.id} disabled={blank.kind === "word_bank" && !content.reuseWordBank && usedWords.has(o.id) && v !== o.id}>
              {stripHtml(o.content)}
            </option>
          ))}
        </select>
      );
    }
    return (
      <input
        className="cbtq-input cbtq-blank"
        style={{ width: `${Math.max(4, blank.width ?? 10) + 2}ch` }}
        inputMode={blank.kind === "numeric" ? "decimal" : "text"}
        readOnly={readOnly}
        value={v}
        aria-label={`Rumpang ${blank.id}`}
        onChange={(e) => set(blank.id, e.target.value)}
      />
    );
  };

  return (
    <div>
      <Html className="cbtq-prompt" html={content.prompt} resolveAsset={resolveAsset} />
      {content.wordBank?.length ? (
        <div className="cbtq-wordbank">
          {content.wordBank.map((w) => (
            <Html
              key={w.id}
              as="span"
              className={`cbtq-chip${usedWords.has(w.id) && !content.reuseWordBank ? " is-used" : ""}`}
              html={w.content}
              resolveAsset={resolveAsset}
            />
          ))}
        </div>
      ) : null}
      <div ref={ref} className="cbtq-cloze" dangerouslySetInnerHTML={{ __html: html }} />
      {slots.map(([id, el]) => createPortal(renderBlank(id), el, id))}
    </div>
  );
}

function stripHtml(html: string) {
  if (typeof document === "undefined") return html.replace(/<[^>]+>/g, "");
  const div = document.createElement("div");
  div.innerHTML = sanitizeHtml(html);
  return div.textContent ?? "";
}

function Categorization({ content, value, onChange, readOnly, resolveAsset, order }: Props<"categorization">) {
  const items = ordered(content.items, order?.items);
  const mapping = value?.mapping ?? {};
  return (
    <div className="cbtq-categorize">
      {[{ id: "", content: "Belum dikelompokkan" }, ...content.categories].map((cat) => {
        const inCat = items.filter((i) => (mapping[i.id] ?? "") === cat.id);
        return (
          <div key={cat.id || "_none"} className={`cbtq-bucket${cat.id ? "" : " is-pool"}`}>
            <Html className="cbtq-bucket-title" html={cat.content} resolveAsset={resolveAsset} />
            {inCat.map((item) => (
              <div key={item.id} className="cbtq-bucket-item">
                <Html as="span" className="cbtq-grow" html={item.content} resolveAsset={resolveAsset} />
                <select
                  className="cbtq-select"
                  disabled={readOnly}
                  value={mapping[item.id] ?? ""}
                  aria-label="Pindahkan ke kategori"
                  onChange={(e) => onChange({ mapping: { ...mapping, [item.id]: e.target.value || null } })}
                >
                  <option value="">-</option>
                  {content.categories.map((c) => (
                    <option key={c.id} value={c.id}>
                      {stripHtml(c.content)}
                    </option>
                  ))}
                </select>
              </div>
            ))}
            {!inCat.length ? <div className="cbtq-hint">kosong</div> : null}
          </div>
        );
      })}
    </div>
  );
}

function Hotspot({ content, value, onChange, readOnly, resolveAsset }: Props<"hotspot">) {
  const ref = useRef<HTMLDivElement>(null);
  const points = value?.points ?? [];
  const src = resolveAssetUris(content.image.src, resolveAsset);
  const add = (e: MouseEvent<HTMLDivElement>) => {
    if (readOnly || !ref.current) return;
    const rect = ref.current.getBoundingClientRect();
    const x = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    const y = Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height));
    const next = [...points, { x: Math.round(x * 10_000) / 10_000, y: Math.round(y * 10_000) / 10_000 }];
    onChange({ points: next.slice(-content.maxSelections) });
  };
  return (
    <div>
      <p className="cbtq-hint">
        Klik pada gambar ({content.maxSelections > 1 ? `maks ${content.maxSelections} titik` : "1 titik"}). Klik titik untuk menghapus.
      </p>
      <div
        ref={ref}
        className="cbtq-hotspot"
        style={{ aspectRatio: `${content.image.width} / ${content.image.height}` }}
        onClick={add}
      >
        <img src={src} alt={content.image.alt ?? ""} draggable={false} />
        {points.map((p, i) => (
          <button
            type="button"
            key={i}
            className="cbtq-marker"
            style={{ left: `${p.x * 100}%`, top: `${p.y * 100}%` }}
            aria-label={`Titik ${i + 1}`}
            onClick={(e) => {
              e.stopPropagation();
              if (!readOnly) onChange({ points: points.filter((_, j) => j !== i) });
            }}
          >
            {i + 1}
          </button>
        ))}
      </div>
    </div>
  );
}

function HotText({ content, value, onChange, readOnly, resolveAsset }: Props<"hot_text">) {
  const selected = new Set(value?.segmentIds ?? []);
  const max = content.maxSelections;
  return (
    <div>
      <p className="cbtq-hint">Klik bagian teks untuk memilih{max ? ` (maks ${max})` : ""}.</p>
      <p className="cbtq-hottext">
        {content.segments.map((s) =>
          s.selectable ? (
            <button
              type="button"
              key={s.id}
              disabled={readOnly}
              className={`cbtq-segment${selected.has(s.id) ? " is-selected" : ""}`}
              onClick={() => {
                const next = new Set(selected);
                if (next.has(s.id)) next.delete(s.id);
                else if (!max || next.size < max) next.add(s.id);
                onChange({ segmentIds: content.segments.map((x) => x.id).filter((id) => next.has(id)) });
              }}
            >
              <Html as="span" html={s.content} resolveAsset={resolveAsset} />
            </button>
          ) : (
            <Html key={s.id} as="span" html={s.content} resolveAsset={resolveAsset} />
          ),
        ).flatMap((node, i) => (i === 0 ? [node] : [" ", node]))}
      </p>
    </div>
  );
}

function Matrix({ content, value, onChange, readOnly, resolveAsset }: Props<"matrix">) {
  const selections = value?.selections ?? {};
  const toggle = (rowId: string, colId: string) => {
    const cur = selections[rowId] ?? [];
    let next: string[];
    if (content.multiplePerRow) next = cur.includes(colId) ? cur.filter((c) => c !== colId) : [...cur, colId];
    else next = cur[0] === colId ? [] : [colId];
    onChange({ selections: { ...selections, [rowId]: next } });
  };
  return (
    <table className="cbtq-table">
      <thead>
        <tr>
          <th />
          {content.columns.map((c) => (
            <th key={c.id}>
              <Html as="span" html={c.content} resolveAsset={resolveAsset} />
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {content.rows.map((r) => (
          <tr key={r.id}>
            <td>
              <Html html={r.content} resolveAsset={resolveAsset} />
            </td>
            {content.columns.map((c) => (
              <td key={c.id} className="cbtq-center">
                <input
                  type={content.multiplePerRow ? "checkbox" : "radio"}
                  name={`mx-${r.id}`}
                  disabled={readOnly}
                  checked={(selections[r.id] ?? []).includes(c.id)}
                  onChange={() => toggle(r.id, c.id)}
                  onClick={() => {
                    // Radio yang sudah terpilih: klik lagi untuk mengosongkan.
                    if (!content.multiplePerRow && (selections[r.id] ?? [])[0] === c.id) toggle(r.id, c.id);
                  }}
                />
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function FileUpload({ content, value, onChange, readOnly, onUploadFile }: Props<"file_upload">) {
  const files = value?.files ?? [];
  const accept = content.accept.join(",");
  const onPick = async (list: FileList | null) => {
    if (!list) return;
    const next = [...files];
    for (const f of Array.from(list)) {
      if (next.length >= content.maxFiles) break;
      if (f.size > content.maxSizeMb * 1024 * 1024) {
        alert(`${f.name} melebihi ${content.maxSizeMb} MB`);
        continue;
      }
      const uploaded = onUploadFile
        ? await onUploadFile(f)
        : { attachmentId: crypto.randomUUID(), name: f.name, size: f.size, mime: f.type || "application/octet-stream" };
      next.push(uploaded);
    }
    onChange({ files: next });
  };
  return (
    <div className="cbtq-upload">
      <p className="cbtq-hint">
        Maks {content.maxFiles} berkas, masing-masing ≤ {content.maxSizeMb} MB{accept ? ` (${accept})` : ""}.
      </p>
      <ul className="cbtq-files">
        {files.map((f) => (
          <li key={f.attachmentId}>
            📎 {f.name} <span className="cbtq-hint">({Math.ceil(f.size / 1024)} KB)</span>
            {!readOnly ? (
              <button type="button" className="cbtq-link" onClick={() => onChange({ files: files.filter((x) => x.attachmentId !== f.attachmentId) })}>
                hapus
              </button>
            ) : null}
          </li>
        ))}
      </ul>
      {!readOnly && files.length < content.maxFiles ? (
        <input type="file" accept={accept || undefined} multiple={content.maxFiles > 1} onChange={(e) => void onPick(e.target.files)} />
      ) : null}
    </div>
  );
}
