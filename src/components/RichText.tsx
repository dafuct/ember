import type { ReactNode, MouseEvent } from "react";
import { openExternal } from "../lib/api";

const ALLOWED_SCHEMES = /^https?:/i;
const URL_RE = /(https?:\/\/[^\s<]+)/g;

function openLink(e: MouseEvent, url: string) {
  e.preventDefault();
  void openExternal(url);
}

function linkifyText(text: string, keyBase: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = new RegExp(URL_RE);
  let last = 0;
  let i = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) out.push(<span key={`${keyBase}-t${i}`}>{text.slice(last, m.index)}</span>);
    const url = m[0];
    out.push(
      <a key={`${keyBase}-u${i}`} href={url} onClick={(e) => openLink(e, url)}>
        {url}
      </a>,
    );
    i++;
    last = m.index + url.length;
  }
  if (last < text.length) out.push(<span key={`${keyBase}-t${i}`}>{text.slice(last)}</span>);
  return out;
}

function renderNode(node: Node, key: string): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) {
    return <span key={key}>{linkifyText(node.textContent ?? "", key)}</span>;
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return null;
  const el = node as Element;
  const tag = el.tagName.toLowerCase();
  const kids = Array.from(el.childNodes).map((c, i) => renderNode(c, `${key}-${i}`));

  switch (tag) {
    case "br":
      return <br key={key} />;
    case "a": {
      const href = el.getAttribute("href") ?? "";
      const text = el.textContent ?? href;
      if (!ALLOWED_SCHEMES.test(href)) return <span key={key}>{text}</span>;
      return (
        <a key={key} href={href} onClick={(e) => openLink(e, href)}>
          {text}
        </a>
      );
    }
    case "p":
    case "div":
      return <div key={key}>{kids}</div>;
    case "b":
    case "strong":
      return <strong key={key}>{kids}</strong>;
    case "i":
    case "em":
      return <em key={key}>{kids}</em>;
    case "ul":
      return <ul key={key}>{kids}</ul>;
    case "ol":
      return <ol key={key}>{kids}</ol>;
    case "li":
      return <li key={key}>{kids}</li>;
    case "script":
    case "style":
    case "img":
      return null;
    default:
      return <span key={key}>{kids}</span>;
  }
}

export function RichText({ html }: { html: string }) {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const nodes = Array.from(doc.body.childNodes).map((n, i) => renderNode(n, `n-${i}`));
  return <>{nodes}</>;
}
