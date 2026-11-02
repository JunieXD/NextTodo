// Writes `.br` and `.gz` next to every text file in `dist`, for the server to send as they are.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

const TEXT = /\.(html|js|css|svg|json|webmanifest|txt)$/;

for (const entry of readdirSync("dist", { recursive: true, withFileTypes: true })) {
  if (!entry.isFile() || !TEXT.test(entry.name)) continue;
  const path = join(entry.parentPath, entry.name);
  const data = readFileSync(path);
  writeFileSync(`${path}.br`, brotliCompressSync(data, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }));
  writeFileSync(`${path}.gz`, gzipSync(data, { level: 9 }));
}
