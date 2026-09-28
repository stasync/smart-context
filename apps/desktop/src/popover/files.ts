export type FileReference = { path: string; line: number | null };

/** Extensions a bare `path.ext` in code must have to count as a file, so
 * `app.listen` or `express.json` stay plain code. */
const CODE_FILE =
  /\.(tsx?|jsx?|mjs|cjs|json|md|mdx|rs|toml|ya?ml|py|go|java|kt|swift|rb|php|cs|c|h|cpp|hpp|css|scss|html|vue|svelte|sh|sql|lock|txt|gradle|xml)$/i;

/**
 * A project file from an answer, like `src/server.ts:12`: relative, no
 * scheme, no `..`. `strict` (for inline code) also wants a folder or a
 * known extension.
 */
export function fileReference(
  text: string,
  strict: boolean,
): FileReference | null {
  const match = /^([\w@.\-/]+?)(?::(\d+))?$/.exec(text.trim());
  if (!match) return null;
  const [, path, line] = match;
  if (
    path.startsWith("/") ||
    path.split("/").includes("..") ||
    !/\w/.test(path)
  ) {
    return null;
  }
  if (strict && !path.includes("/") && !CODE_FILE.test(path)) return null;
  if (!strict && !path.includes("/") && !path.includes(".")) return null;
  return { path, line: line ? Number(line) : null };
}
