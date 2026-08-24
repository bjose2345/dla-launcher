import DOMPurify from "dompurify";
import { useMemo, useState } from "react";

import { DescriptionVersionPicker } from "./DescriptionVersionPicker";
import type { CatalogDescriptions } from "./types";
import { WorkDetailSection } from "./WorkDetailSection";
import { latestDescriptionVersion, orderDescriptionVersions } from "./descriptionVersions";
import { usePresentation } from "../../preferences/PresentationProvider";

const DESCRIPTION_TAGS = [
  "div", "p", "h2", "h3", "h4", "span", "a", "img", "iframe", "br", "ul", "ol", "li",
  "table", "thead", "tbody", "tr", "td", "th", "strong", "em", "b", "i", "u", "hr",
];

const DESCRIPTION_ATTRIBUTES = [
  "class", "itemprop", "href", "target", "rel", "src", "alt", "title", "width", "height",
  "loading", "frameborder", "allowfullscreen", "referrerpolicy", "sandbox",
];

interface WorkDescriptionProps {
  descriptions: CatalogDescriptions;
  onOpenExternal: (url: string) => Promise<void>;
}

export function WorkDescription({ descriptions, onOpenExternal }: WorkDescriptionProps) {
  const { t } = usePresentation();
  const versions = useMemo(
    () => orderDescriptionVersions(descriptions.versions),
    [descriptions.versions],
  );
  const latest = useMemo(() => latestDescriptionVersion(versions), [versions]);
  const [selectedVersion, setSelectedVersion] = useState(latest?.version ?? 0);
  const selected = versions.find((entry) => entry.version === selectedVersion) ?? latest;
  const html = useMemo(() => sanitizeDescriptionHtml(selected?.html ?? ""), [selected?.html]);
  const emptyLabel = descriptions.included
    ? t("detail.descriptionUnavailable")
    : t("detail.descriptionNotIncluded");

  return (
    <WorkDetailSection
      title={t("detail.description")}
      headingAccessory={versions.length > 1 && latest ? (
        <DescriptionVersionPicker
          versions={versions}
          selectedVersion={selected?.version ?? latest.version}
          latestVersion={latest.version}
          latestLabel={t("detail.descriptionLatest")}
          onSelect={setSelectedVersion}
        />
      ) : null}
    >
      {html ? (
        <div
          className="work-description-content"
          onClick={(event) => {
            const anchor = (event.target as Element).closest("a[href]");
            if (!anchor) return;
            const href = anchor.getAttribute("href");
            const url = href ? remoteUrl(href) : null;
            if (!url) return;
            event.preventDefault();
            void onOpenExternal(url).catch(() => undefined);
          }}
          dangerouslySetInnerHTML={{ __html: html }}
        />
      ) : (
        <p className="work-detail-empty">{emptyLabel}</p>
      )}
    </WorkDetailSection>
  );
}

function sanitizeDescriptionHtml(html: string) {
  if (!html.trim()) return "";
  const sanitized = DOMPurify.sanitize(html, {
    ALLOWED_TAGS: DESCRIPTION_TAGS,
    ALLOWED_ATTR: DESCRIPTION_ATTRIBUTES,
    ALLOW_DATA_ATTR: false,
    FORBID_ATTR: ["style"],
  });
  const template = document.createElement("template");
  template.innerHTML = sanitized;
  template.content.querySelectorAll<HTMLAnchorElement>("a[href]").forEach((anchor) => {
    const url = remoteUrl(anchor.getAttribute("href") ?? "");
    if (!url) {
      anchor.removeAttribute("href");
      return;
    }
    anchor.href = url;
    anchor.target = "_blank";
    anchor.rel = "noopener noreferrer nofollow";
  });
  template.content.querySelectorAll<HTMLImageElement>("img[src]").forEach((image) => {
    const url = remoteUrl(image.getAttribute("src") ?? "");
    if (!url) image.remove();
    else image.src = url;
  });
  template.content.querySelectorAll<HTMLIFrameElement>("iframe[src]").forEach((frame) => {
    const url = remoteUrl(frame.getAttribute("src") ?? "");
    if (!url || !isAllowedFrame(url)) {
      frame.remove();
      return;
    }
    frame.src = url;
    frame.loading = "lazy";
    frame.referrerPolicy = "no-referrer";
    frame.setAttribute("sandbox", "allow-scripts allow-same-origin allow-presentation");
  });
  return template.innerHTML;
}

function remoteUrl(value: string) {
  try {
    const normalized = value.startsWith("//") ? `https:${value}` : value;
    const url = new URL(normalized);
    return url.protocol === "http:" || url.protocol === "https:" ? url.toString() : null;
  } catch {
    return null;
  }
}

function isAllowedFrame(value: string) {
  const hostname = new URL(value).hostname.toLowerCase();
  return hostname === "chobit.cc" || hostname.endsWith(".chobit.cc");
}
