/**
 * 发送前压缩图片：最长边超过 MAX_SIDE 的先缩小，再转成 WebP（WebView 不支持时用 JPEG）。
 * 截图一般从几 MB 变成几百 KB，发送和客服查看都快很多。GIF（可能是动图）和本来就小的图片原样发送；
 * 压缩后反而变大时也用原图。
 */

/** 最长边超过这么多像素的图片先缩小（报错文字仍然看得清）。 */
export const MAX_SIDE = 2048;
const QUALITY = 0.85;
/** 尺寸不超过上限、又不到这个大小的图片原样发送。 */
const KEEP_BELOW_BYTES = 300 * 1024;

let webpEncoding: boolean | undefined;

/** WebView 能不能把画布存成 WebP（Windows 的 WebView2 可以；部分 macOS 版本不行）。 */
function canEncodeWebp(): boolean {
  if (webpEncoding === undefined) {
    const canvas = document.createElement("canvas");
    canvas.width = 1;
    canvas.height = 1;
    webpEncoding = canvas.toDataURL("image/webp").startsWith("data:image/webp");
  }
  return webpEncoding;
}

/** `webp`：指定输出格式（测试用）；不指定时按 WebView 是否支持 WebP 选。 */
export async function compressImage(file: File, options: { webp?: boolean } = {}): Promise<File> {
  if (file.type === "image/gif") return file;
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(file);
  } catch {
    // 解不开的图片原样交给桌面端和服务器检查。
    return file;
  }
  const scale = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
  if (scale === 1 && file.size <= KEEP_BELOW_BYTES) {
    bitmap.close();
    return file;
  }
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));
  const context = canvas.getContext("2d");
  if (!context) {
    bitmap.close();
    return file;
  }
  const webp = options.webp ?? canEncodeWebp();
  if (!webp) {
    // JPEG 没有透明：先铺白色。
    context.fillStyle = "#ffffff";
    context.fillRect(0, 0, canvas.width, canvas.height);
  }
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  const type = webp ? "image/webp" : "image/jpeg";
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, type, QUALITY));
  if (!blob || blob.type !== type || blob.size >= file.size) return file;
  const base = file.name.replace(/\.[^./\\]*$/, "") || "image";
  return new File([blob], `${base}.${webp ? "webp" : "jpg"}`, { type });
}
