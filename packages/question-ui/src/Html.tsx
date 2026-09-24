import DOMPurify from "dompurify";
import { useMemo, type ElementType } from "react";
import { ASSET_URI_REGEX } from "@cbt/shared";

/** Ubah `asset://<uuid>` menjadi URL yang bisa dimuat (signed URL di admin, file lokal di desktop). */
export type AssetResolver = (assetId: string) => string | undefined;

/** Gambar transparen 1x1 selama URL aset belum tersedia (hindari request ke skema asset://). */
const PENDING_ASSET = "data:image/gif;base64,R0lGODlhAQABAAAAACw=";

export function resolveAssetUris(html: string, resolve?: AssetResolver): string {
  if (!resolve) return html;
  return html.replace(ASSET_URI_REGEX, (_match, id: string) => resolve(id.toLowerCase()) ?? PENDING_ASSET);
}

// Izinkan skema asset:// dan URL relatif/https/blob/data gambar; blokir javascript: dsb.
const ALLOWED_URI = /^(?:(?:https?|blob|asset):|data:image\/|\/|\.|#)/i;

export function sanitizeHtml(html: string): string {
  return DOMPurify.sanitize(html, {
    ALLOWED_URI_REGEXP: ALLOWED_URI,
    ADD_TAGS: ["audio", "video", "source", "track"],
    ADD_ATTR: ["controls", "controlslist", "preload", "target"],
    FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "iframe", "object", "embed"],
  });
}

interface HtmlProps {
  html: string;
  resolveAsset?: AssetResolver;
  as?: ElementType;
  className?: string;
}

/** Tampilkan konten HTML soal yang sudah disanitasi, dengan rujukan aset sudah di-resolve. */
export function Html({ html, resolveAsset, as: Tag = "div", className }: HtmlProps) {
  const safe = useMemo(() => resolveAssetUris(sanitizeHtml(html), resolveAsset), [html, resolveAsset]);
  return <Tag className={className} dangerouslySetInnerHTML={{ __html: safe }} />;
}
