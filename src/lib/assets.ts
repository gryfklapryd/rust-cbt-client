/**
 * URL media lokal yang disajikan backend lewat protokol `cbtasset`.
 * Windows (WebView2) memakai bentuk http://<scheme>.localhost/, platform lain <scheme>://localhost/.
 */
const isWindows = typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent);

export function assetUrl(assetId: string): string {
  return isWindows ? `http://cbtasset.localhost/${assetId}` : `cbtasset://localhost/${assetId}`;
}

export const resolveAsset = (assetId: string) => assetUrl(assetId);
