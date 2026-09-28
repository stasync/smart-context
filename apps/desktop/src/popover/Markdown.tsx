import { openUrl } from "@tauri-apps/plugin-opener";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openFile } from "../shared/ipc";
import { fileReference, type FileReference } from "./files";

const OPENABLE = /^(https?:|mailto:)/i;

function open(file: FileReference) {
  void openFile(file.path, file.line);
}

/** An answer's Markdown. Web links open in the browser; project files open
 * in the editor, whether linked or written as code. */
export function Markdown({ text }: { text: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        a: ({ href, children }) => {
          const file = href ? fileReference(decodeURI(href), false) : null;
          return (
            <a
              href={href}
              title={file ? "Open in the editor" : href}
              onClick={(event) => {
                event.preventDefault();
                if (file) open(file);
                else if (href && OPENABLE.test(href)) void openUrl(href);
              }}
            >
              {children}
            </a>
          );
        },
        code: ({ className, children }) => {
          const text = String(children);
          const file =
            !className && !text.includes("\n")
              ? fileReference(text, true)
              : null;
          if (!file) return <code className={className}>{children}</code>;
          return (
            <code>
              <a
                href="#"
                title="Open in the editor"
                onClick={(e) => (e.preventDefault(), open(file))}
              >
                {children}
              </a>
            </code>
          );
        },
      }}
    >
      {text}
    </ReactMarkdown>
  );
}
