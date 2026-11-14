import { createContext, useContext, type ReactNode } from "react";
import { EN } from "./i18n.en";

// Deutsch ist die Grundsprache: Der deutsche Text ist gleichzeitig der Schlüssel.
// Platzhalter: {0}, {1} …
type Lang = "de" | "en";
const Ctx = createContext<Lang>("de");

export function LangProvider({ lang, children }: { lang: Lang; children: ReactNode }) {
  return <Ctx.Provider value={lang}>{children}</Ctx.Provider>;
}

let current: Lang = "de";
export function setLang(l: Lang) {
  current = l;
}

export function tr(s: string, ...args: (string | number)[]) {
  let out = current === "en" ? (EN[s] ?? s) : s;
  args.forEach((a, i) => (out = out.replaceAll(`{${i}}`, String(a))));
  return out;
}

export function useT() {
  useContext(Ctx); // neu rendern bei Sprachwechsel
  return tr;
}
