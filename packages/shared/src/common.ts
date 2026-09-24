import { z } from "zod";

/**
 * Konten kaya (rich text) dalam bentuk HTML.
 *
 * Media (gambar, audio, video) TIDAK di-embed langsung, melainkan dirujuk
 * dengan URI `asset://<uuid>` (mis. `<img src="asset://…">`). Server
 * mengumpulkan semua rujukan ini saat membangun paket ujian sehingga client
 * desktop bisa mengunduh dan menyajikannya secara offline.
 *
 * Rumus matematika ditulis LaTeX: `\( … \)` inline dan `\[ … \]` blok.
 */
export const RichText = z.string().max(200_000);
export type RichText = z.infer<typeof RichText>;

/** ID lokal di dalam satu soal (opsi, pernyataan, item, blank, region, …). */
export const LocalId = z
  .string()
  .min(1)
  .max(64)
  .regex(/^[A-Za-z0-9_-]+$/, "Hanya huruf, angka, '_' dan '-'");
export type LocalId = z.infer<typeof LocalId>;

/** Sebuah pilihan/opsi yang punya id dan konten. */
export const Choice = z.object({
  id: LocalId,
  content: RichText,
});
export type Choice = z.infer<typeof Choice>;

export const ASSET_URI_PREFIX = "asset://";
export const ASSET_URI_REGEX = /asset:\/\/([0-9a-fA-F-]{36})/g;

/** Kumpulkan semua id aset yang dirujuk (`asset://<uuid>`) di dalam sebuah nilai JSON. */
export function collectAssetIds(value: unknown, into: Set<string> = new Set()): Set<string> {
  if (typeof value === "string") {
    for (const m of value.matchAll(ASSET_URI_REGEX)) into.add(m[1]!.toLowerCase());
  } else if (Array.isArray(value)) {
    for (const v of value) collectAssetIds(v, into);
  } else if (value && typeof value === "object") {
    for (const v of Object.values(value)) collectAssetIds(v, into);
  }
  return into;
}

export const IsoDateTime = z.iso.datetime({ offset: true });

export function findDuplicates(values: readonly string[]): string[] {
  const seen = new Set<string>();
  const dups = new Set<string>();
  for (const v of values) {
    if (seen.has(v)) dups.add(v);
    seen.add(v);
  }
  return [...dups];
}
