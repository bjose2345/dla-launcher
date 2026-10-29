import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ImagePlus, Images, LoaderCircle, X } from "lucide-react";
import { useEffect, useId, useState } from "react";

import type { MessageKey } from "../../i18n/catalogs";
import { usePresentation } from "../../preferences/PresentationProvider";
import { OWN_WORK_DETAILS_KEY } from "./ownWorkDetails";
import type { LibraryGateway, OwnWorkDetails, OwnWorkKind, SaveOwnWorkDetailsRequest } from "./types";

export type OwnWorkEditorGateway = Pick<
  LibraryGateway,
  | "readOwnWorkDetails"
  | "saveOwnWorkDetails"
  | "listInstallationImages"
  | "importOwnWorkArtwork"
  | "importOwnWorkArtworkFile"
  | "ownWorkArtworkUrl"
  | "installationImageUrl"
>;

export type OwnWorkSaveState = "saving" | "saved" | "failed";

export const MAX_SAMPLE_IMAGES = 12;
const IMAGES_SHOWN = 24;
const KINDS: ReadonlyArray<readonly [OwnWorkKind, MessageKey]> = [
  ["images", "library.own.kind.images"],
  ["manga", "library.own.kind.manga"],
  ["game", "library.own.kind.game"],
  ["voice", "library.own.kind.voice"],
  ["video", "library.own.kind.video"],
  ["document", "library.own.kind.document"],
];

export function ownWorkDetailsKey(installationId: string) {
  return [...OWN_WORK_DETAILS_KEY, installationId] as const;
}

/**
 * The details a work outside the catalog shows where a catalog work shows
 * the catalog's: a cover and sample images from its own files or a picture,
 * its circle, code, date, kind, tags and description. Each change is saved
 * as it is made, like the rest of Edit details.
 */
export function OwnWorkDetailsEditor({
  installationId,
  gateway,
  onSaveState,
}: {
  installationId: string;
  gateway: OwnWorkEditorGateway;
  onSaveState: (state: OwnWorkSaveState, error?: unknown) => void;
}) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const ids = { circle: useId(), code: useId(), date: useId(), tags: useId(), description: useId() };
  const saved = useQuery({
    queryKey: ownWorkDetailsKey(installationId),
    queryFn: () => gateway.readOwnWorkDetails(installationId),
  });
  const [draft, setDraft] = useState<SaveOwnWorkDetailsRequest>(() => toRequest(installationId, null));
  const [tag, setTag] = useState("");
  const [picker, setPicker] = useState<"cover" | "samples" | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [importing, setImporting] = useState<string | null>(null);
  const images = useQuery({
    queryKey: ["library", "installation-images", installationId],
    queryFn: () => gateway.listInstallationImages(installationId),
    enabled: picker !== null,
  });

  useEffect(() => {
    if (saved.data !== undefined) setDraft(toRequest(installationId, saved.data));
  }, [installationId, saved.data]);

  const commit = async (next: SaveOwnWorkDetailsRequest) => {
    setDraft(next);
    onSaveState("saving");
    try {
      const details = await gateway.saveOwnWorkDetails(next);
      queryClient.setQueryData(ownWorkDetailsKey(installationId), details);
      void queryClient.invalidateQueries({ queryKey: OWN_WORK_DETAILS_KEY, exact: true });
      onSaveState("saved");
    } catch (error) {
      onSaveState("failed", error);
    }
  };
  const change = (patch: Partial<SaveOwnWorkDetailsRequest>) => void commit({ ...draft, ...patch });
  const commitText = (field: "circle" | "code" | "releaseDate" | "description") => {
    const value = draft[field]?.trim() || null;
    if (value === (saved.data?.[field] ?? null)) return;
    void commit({ ...draft, [field]: value });
  };
  const setText = (field: "circle" | "code" | "releaseDate" | "description", value: string) => (
    setDraft((current) => ({ ...current, [field]: value }))
  );

  const pickImage = async (relativePath: string) => {
    if (importing) return;
    setImporting(relativePath);
    try {
      const artwork = await gateway.importOwnWorkArtwork(installationId, relativePath);
      if (picker === "cover") {
        setPicker(null);
        await commit({ ...draft, cover: artwork });
      } else if (!draft.samples.includes(artwork) && draft.samples.length < MAX_SAMPLE_IMAGES) {
        await commit({ ...draft, samples: [...draft.samples, artwork] });
      }
    } catch (error) {
      onSaveState("failed", error);
    } finally {
      setImporting(null);
    }
  };
  const pickFile = async () => {
    try {
      const artwork = await gateway.importOwnWorkArtworkFile(installationId);
      if (artwork) await commit({ ...draft, cover: artwork });
    } catch (error) {
      onSaveState("failed", error);
    }
  };
  const addTag = () => {
    const value = tag.trim();
    setTag("");
    if (!value || draft.tags.some((existing) => existing.toLocaleLowerCase() === value.toLocaleLowerCase())) return;
    change({ tags: [...draft.tags, value] });
  };

  const shownImages = showAll ? images.data ?? [] : (images.data ?? []).slice(0, IMAGES_SHOWN);
  const coverUrl = draft.cover ? gateway.ownWorkArtworkUrl(installationId, draft.cover) : null;

  return (
    <div className="library-own">
      <section className="library-manage-field">
        <h3>{t("library.own.title")}</h3>
        <p className="library-manage-help">{t("library.own.help")}</p>
      </section>

      <section className="library-manage-field">
        <h3>{t("library.own.cover")}</h3>
        <div className="library-own-cover">
          <span className="library-own-cover-art">
            {coverUrl ? <img src={coverUrl} alt={t("library.own.cover")} /> : <Images aria-hidden="true" />}
          </span>
          <div className="library-own-cover-actions">
            <p className="library-manage-help">{t("library.own.coverHelp")}</p>
            <div className="library-manage-row">
              <button
                className="library-manage-button"
                type="button"
                aria-expanded={picker === "cover"}
                onClick={() => setPicker((current) => (current === "cover" ? null : "cover"))}
              >
                {t(picker === "cover" ? "library.own.hideImages" : "library.own.fromWork")}
              </button>
              <button className="library-manage-button" type="button" onClick={() => void pickFile()}>
                <ImagePlus aria-hidden="true" />{t("library.own.fromFile")}
              </button>
              {draft.cover ? (
                <button className="library-manage-link" type="button" onClick={() => change({ cover: null })}>
                  {t("library.own.removeCover")}
                </button>
              ) : null}
            </div>
          </div>
        </div>
        {picker === "cover" ? (
          <ImageChoices
            images={shownImages}
            total={images.data?.length ?? 0}
            loading={images.isPending}
            importing={importing}
            label={(name) => t("library.own.useAsCover", { name })}
            imageUrl={(path) => gateway.installationImageUrl(installationId, path)}
            onPick={(path) => void pickImage(path)}
            onShowAll={showAll ? undefined : () => setShowAll(true)}
          />
        ) : null}
      </section>

      <div className="library-own-fields">
        <section className="library-manage-field">
          <label htmlFor={ids.circle}><h3>{t("library.manage.circle")}</h3></label>
          <input
            id={ids.circle}
            className="library-manage-input"
            type="text"
            value={draft.circle ?? ""}
            placeholder={t("library.own.circlePlaceholder")}
            onChange={(event) => setText("circle", event.target.value)}
            onBlur={() => commitText("circle")}
          />
        </section>
        <section className="library-manage-field">
          <label htmlFor={ids.code}><h3>{t("library.manage.code")}</h3></label>
          <input
            id={ids.code}
            className="library-manage-input library-own-code"
            type="text"
            value={draft.code ?? ""}
            placeholder={t("library.own.codePlaceholder")}
            onChange={(event) => setText("code", event.target.value)}
            onBlur={() => commitText("code")}
          />
        </section>
        <section className="library-manage-field">
          <label htmlFor={ids.date}><h3>{t("library.own.releaseDate")}</h3></label>
          <input
            id={ids.date}
            className="library-manage-input"
            type="date"
            value={draft.releaseDate ?? ""}
            onChange={(event) => setText("releaseDate", event.target.value)}
            onBlur={() => commitText("releaseDate")}
          />
        </section>
        <section className="library-manage-field">
          <label htmlFor={ids.tags}><h3>{t("library.own.tags")}</h3></label>
          <div className="library-own-tags">
            {draft.tags.map((value) => (
              <span className="library-own-tag" key={value}>
                {value}
                <button
                  type="button"
                  aria-label={t("library.own.removeTag", { tag: value })}
                  onClick={() => change({ tags: draft.tags.filter((existing) => existing !== value) })}
                >
                  <X aria-hidden="true" />
                </button>
              </span>
            ))}
            <input
              id={ids.tags}
              className="library-manage-input"
              type="text"
              value={tag}
              placeholder={t("library.own.addTag")}
              onChange={(event) => setTag(event.target.value)}
              onBlur={addTag}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === ",") {
                  event.preventDefault();
                  addTag();
                }
              }}
            />
          </div>
        </section>
      </div>

      <section className="library-manage-field">
        <h3>{t("library.own.kind")}</h3>
        <div className="library-own-kinds" role="group" aria-label={t("library.own.kind")}>
          {KINDS.map(([value, label]) => (
            <button
              className={`library-own-kind${draft.kind === value ? " is-active" : ""}`}
              type="button"
              key={value}
              aria-pressed={draft.kind === value}
              onClick={() => change({ kind: draft.kind === value ? null : value })}
            >
              {t(label)}
            </button>
          ))}
        </div>
      </section>

      <section className="library-manage-field">
        <h3>{t("library.own.samples")}</h3>
        <p className="library-manage-help">{t("library.own.samplesHelp", { count: MAX_SAMPLE_IMAGES })}</p>
        <div className="library-own-samples">
          {draft.samples.map((artwork, index) => (
            <span className="library-own-sample" key={artwork}>
              <img src={gateway.ownWorkArtworkUrl(installationId, artwork)} alt="" />
              <button
                type="button"
                aria-label={t("library.own.removeSample", { number: index + 1 })}
                onClick={() => change({ samples: draft.samples.filter((existing) => existing !== artwork) })}
              >
                <X aria-hidden="true" />
              </button>
            </span>
          ))}
          {draft.samples.length < MAX_SAMPLE_IMAGES ? (
            <button
              className="library-own-sample-add"
              type="button"
              aria-expanded={picker === "samples"}
              onClick={() => setPicker((current) => (current === "samples" ? null : "samples"))}
            >
              {t(picker === "samples" ? "library.own.hideImages" : "library.own.pickSamples")}
            </button>
          ) : null}
        </div>
        {picker === "samples" ? (
          <ImageChoices
            images={shownImages}
            total={images.data?.length ?? 0}
            loading={images.isPending}
            importing={importing}
            label={(name) => t("library.own.addSample", { name })}
            imageUrl={(path) => gateway.installationImageUrl(installationId, path)}
            onPick={(path) => void pickImage(path)}
            onShowAll={showAll ? undefined : () => setShowAll(true)}
          />
        ) : null}
      </section>

      <section className="library-manage-field">
        <label htmlFor={ids.description}><h3>{t("library.own.description")}</h3></label>
        <textarea
          id={ids.description}
          className="library-manage-input library-own-description"
          rows={4}
          value={draft.description ?? ""}
          placeholder={t("library.own.optional")}
          onChange={(event) => setText("description", event.target.value)}
          onBlur={() => commitText("description")}
        />
      </section>
    </div>
  );
}

function ImageChoices({
  images,
  total,
  loading,
  importing,
  label,
  imageUrl,
  onPick,
  onShowAll,
}: {
  images: { relativePath: string }[];
  total: number;
  loading: boolean;
  importing: string | null;
  label: (name: string) => string;
  imageUrl: (relativePath: string) => string;
  onPick: (relativePath: string) => void;
  onShowAll?: () => void;
}) {
  const { t } = usePresentation();
  if (loading) {
    return <p className="library-manage-note"><LoaderCircle className="library-spin" aria-hidden="true" />{t("library.own.imagesLoading")}</p>;
  }
  if (!total) return <p className="library-manage-help">{t("library.own.noImages")}</p>;
  return (
    <div className="library-own-choices">
      <div className="library-own-grid">
        {images.map((image) => {
          const name = image.relativePath.split("/").at(-1) ?? image.relativePath;
          return (
            <button
              className="library-own-tile"
              type="button"
              key={image.relativePath}
              aria-label={label(name)}
              title={name}
              disabled={importing !== null}
              onClick={() => onPick(image.relativePath)}
            >
              <img src={imageUrl(image.relativePath)} alt="" loading="lazy" decoding="async" />
              {importing === image.relativePath ? <LoaderCircle className="library-spin" aria-hidden="true" /> : null}
            </button>
          );
        })}
      </div>
      {onShowAll && total > images.length ? (
        <button className="library-manage-link" type="button" onClick={onShowAll}>
          {t("library.own.showAll", { count: total })}
        </button>
      ) : null}
    </div>
  );
}

function toRequest(installationId: string, details: OwnWorkDetails | null): SaveOwnWorkDetailsRequest {
  return {
    installationId,
    circle: details?.circle ?? null,
    code: details?.code ?? null,
    releaseDate: details?.releaseDate ?? null,
    kind: details?.kind ?? null,
    tags: details?.tags ?? [],
    description: details?.description ?? null,
    cover: details?.cover ?? null,
    samples: details?.samples ?? [],
  };
}
