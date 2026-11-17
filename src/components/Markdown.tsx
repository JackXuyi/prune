/**
 * Minimal markdown renderer for the update dialog (docs/updater-plan.md).
 *
 * Why hand-rolled instead of react-markdown: the project stays deliberately
 * free of markdown deps; CHANGELOG / release notes only use a small syntax
 * subset, so a compact renderer is enough.
 *
 * Safety: everything is composed as React elements, never via
 * dangerouslySetInnerHTML, so there is no XSS surface. Only http(s) links are
 * allowed; other schemes (javascript:/data:) degrade to plain text.
 *
 * Supported: # / ## / ### headings, - / * and 1. lists, **bold**, `code`,
 * [text](url) links, blank-line paragraphs, and fenced code blocks.
 */
import type { ReactNode } from "react";

// Split inline markdown into React nodes (bold / inline code / links).
// NOTE: backticks in the regex use \x60 (not literal `) so the repo's i18n
// scanner does not mistake them for a template-literal delimiter.
const INLINE = /(\*\*[^*]+\*\*|\x60[^\x60]+\x60|\[[^\]]+\]\([^)]+\))/g;

function renderInline(text: string, keyPrefix: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  let m: RegExpExecArray | null;
  INLINE.lastIndex = 0;
  while ((m = INLINE.exec(text)) !== null) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const token = m[0];
    const k = `${keyPrefix}-${m.index}`;
    if (token.startsWith("**") && token.endsWith("**")) {
      out.push(
        <strong key={k} className="font-semibold text-ink">
          {token.slice(2, -2)}
        </strong>,
      );
    } else if (token.startsWith("`") && token.endsWith("`")) {
      out.push(
        <code
          key={k}
          className="rounded bg-surface px-1 py-px font-mono text-[0.92em] text-accent"
        >
          {token.slice(1, -1)}
        </code>,
      );
    } else {
      // Link [text](url): only http(s) is allowed.
      const link = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token);
      if (link) {
        const [, label, url] = link;
        const safe = /^https?:\/\//i.test(url.trim());
        out.push(
          safe ? (
            <a
              key={k}
              href={url}
              target="_blank"
              rel="noreferrer"
              className="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-accent"
            >
              {label}
            </a>
          ) : (
            <span key={k}>{label}</span>
          ),
        );
      } else {
        out.push(token);
      }
    }
    last = m.index + token.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

/**
 * Render markdown text into a div. `className` is applied to the container;
 * body typography is applied inline via Tailwind classes on each element.
 */
export function Markdown({ source, className }: { source: string; className?: string }) {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  let key = 0;

  while (i < lines.length) {
    const raw = lines[i];
    const line = raw.trimEnd();

    // Fenced code block
    if (line.trim().startsWith("```")) {
      const buf: string[] = [];
      i += 1;
      while (i < lines.length && !lines[i].trim().startsWith("```")) {
        buf.push(lines[i]);
        i += 1;
      }
      i += 1; // skip closing fence
      blocks.push(
        <pre
          key={key++}
          className="my-2 overflow-x-auto rounded-ctl bg-surface px-2.5 py-2 font-mono text-[11px] leading-relaxed text-ink-dim"
        >
          {buf.join("\n")}
        </pre>,
      );
      continue;
    }

    // Blank line: skip (paragraph spacing comes from block margins)
    if (!line.trim()) {
      i += 1;
      continue;
    }

    // Heading
    const heading = /^(#{1,3})\s+(.*)$/.exec(line);
    if (heading) {
      const level = heading[1].length;
      const content = renderInline(heading[2], `h${key}`);
      const cls =
        level === 1
          ? "text-[14px] font-semibold text-ink"
          : level === 2
            ? "text-[13px] font-semibold text-ink"
            : "text-[12px] font-medium text-ink-dim";
      blocks.push(
        <div key={key++} className={`${cls} mt-3 first:mt-0`}>
          {content}
        </div>,
      );
      i += 1;
      continue;
    }

    // Unordered list (merge consecutive - / * lines into one ul)
    if (/^\s*[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*[-*]\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*[-*]\s+/, ""));
        i += 1;
      }
      blocks.push(
        <ul key={key++} className="my-1.5 space-y-1.5 pl-1">
          {items.map((item, idx) => (
            <li key={idx} className="flex gap-2 text-[12px] leading-relaxed text-ink-dim">
              <span className="mt-[7px] h-1 w-1 shrink-0 rounded-full bg-accent/70" />
              <span className="min-w-0">{renderInline(item, `li${key}-${idx}`)}</span>
            </li>
          ))}
        </ul>,
      );
      continue;
    }

    // Ordered list (merge consecutive 1. 2. lines into one ol)
    if (/^\s*\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*\d+\.\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*\d+\.\s+/, ""));
        i += 1;
      }
      blocks.push(
        <ol key={key++} className="my-1.5 space-y-1.5 pl-1">
          {items.map((item, idx) => (
            <li key={idx} className="flex gap-2 text-[12px] leading-relaxed text-ink-dim">
              <span className="shrink-0 font-mono text-[11px] text-accent">{idx + 1}.</span>
              <span className="min-w-0">{renderInline(item, `ol${key}-${idx}`)}</span>
            </li>
          ))}
        </ol>,
      );
      continue;
    }

    // Plain paragraph
    blocks.push(
      <p key={key++} className="my-1 text-[12px] leading-relaxed text-ink-dim">
        {renderInline(line, `p${key}`)}
      </p>,
    );
    i += 1;
  }

  return <div className={className}>{blocks}</div>;
}
