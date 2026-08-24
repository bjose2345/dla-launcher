import { useId, type ReactNode } from "react";

import type { LocaleId } from "./preferences";

const flags: Record<LocaleId, ReactNode> = {
  "en-US": (
    <>
      <path fill="#eee" d="M0 0h512v512H0z" />
      <g fill="#d80027">
        <path d="M0 64h512v56H0zM0 176h512v56H0zM0 288h512v56H0zM0 400h512v56H0z" />
      </g>
      <path fill="#0052b4" d="M0 0h256v232H0z" />
    </>
  ),
  "ja-JP": (
    <>
      <path fill="#eee" d="M0 0h512v512H0z" />
      <circle cx="256" cy="256" r="111" fill="#d80027" />
    </>
  ),
  "es-ES": (
    <>
      <path fill="#ffda44" d="M0 128h512v256H0z" />
      <path fill="#d80027" d="M0 0h512v128H0zM0 384h512v128H0z" />
    </>
  ),
  "de-DE": (
    <>
      <path fill="#333" d="M0 0h512v171H0z" />
      <path fill="#d80027" d="M0 171h512v170H0z" />
      <path fill="#ffda44" d="M0 341h512v171H0z" />
    </>
  ),
  "fr-FR": (
    <>
      <path fill="#eee" d="M171 0h170v512H171z" />
      <path fill="#0052b4" d="M0 0h171v512H0z" />
      <path fill="#d80027" d="M341 0h171v512H341z" />
    </>
  ),
  "it-IT": (
    <>
      <path fill="#eee" d="M171 0h170v512H171z" />
      <path fill="#6da544" d="M0 0h171v512H0z" />
      <path fill="#d80027" d="M341 0h171v512H341z" />
    </>
  ),
  "pt-PT": (
    <>
      <path fill="#d80027" d="M196 0h316v512H196z" />
      <path fill="#6da544" d="M0 0h196v512H0z" />
      <circle cx="196" cy="256" r="64" fill="#ffda44" />
      <circle cx="196" cy="256" r="42" fill="#d80027" />
    </>
  ),
  "ru-RU": (
    <>
      <path fill="#eee" d="M0 0h512v171H0z" />
      <path fill="#0052b4" d="M0 171h512v170H0z" />
      <path fill="#d80027" d="M0 341h512v171H0z" />
    </>
  ),
};

export function LocaleFlag({ locale }: { locale: LocaleId }) {
  const clipId = `locale-flag-${useId().replaceAll(":", "")}`;
  return (
    <svg className="locale-flag" viewBox="0 0 512 512" aria-hidden="true">
      <mask id={clipId}>
        <circle cx="256" cy="256" r="256" fill="#fff" />
      </mask>
      <g mask={`url(#${clipId})`}>{flags[locale]}</g>
    </svg>
  );
}
