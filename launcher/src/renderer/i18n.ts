import i18n from "i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import { initReactI18next } from "react-i18next";

export const fallbackLng = "en";

function requireNamespace(
  lang: string,
  nsKey: string
): { [key: string]: string } {
  try {
    return require(`./locales/${lang}/${nsKey}.json`);
  } catch (_e) {
    console.warn("could not load locale namespace", lang, nsKey);
    return {};
  }
}

function requireLocale(lang: string): {
  [nsKey: string]: { [key: string]: string };
} {
  return {
    common: requireNamespace(lang, "common"),
    navbar: requireNamespace(lang, "navbar"),
    patches: requireNamespace(lang, "patches"),
    play: requireNamespace(lang, "play"),
    replays: requireNamespace(lang, "replays"),
    settings: requireNamespace(lang, "settings"),
    setup: requireNamespace(lang, "setup"),
    supervisor: requireNamespace(lang, "supervisor"),
    "input-keys": requireNamespace(lang, "input-keys"),
    "input-buttons": requireNamespace(lang, "input-buttons"),
    "input-axes": requireNamespace(lang, "input-axes"),
  };
}

i18n
  .use({
    type: "backend",
    init: () => {
      void null;
    },
    read: function (
      language: string,
      namespace: string,
      callback: (err: any, data?: { [key: string]: string }) => void
    ) {
      callback(null, requireLocale(language)[namespace]);
    },
  })
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    fallbackLng,
    ns: Object.keys(requireLocale("en")),
    defaultNS: "common",

    react: {
      transSupportBasicHtmlNodes: false,
    },

    interpolation: {
      escapeValue: false, // not needed for react as it escapes by default
    },
  });

export default i18n;

export const LANGUAGES = [
  { code: "en", name: "English" },
  { code: "ja", name: "日本語" },
  { code: "zh-Hans", name: "简体中文" },
  { code: "es", name: "Español" },
  { code: "pt-BR", name: "Português (Brasil)" },
  { code: "fr", name: "Français" },
  { code: "de", name: "Deutsch" },
];
