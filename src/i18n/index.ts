import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zhCN from "./zh-CN";
import enUS from "./en-US";

export const languages = [
  { code: "zh-CN", label: "简体中文" },
  { code: "en-US", label: "English" },
] as const;

const storageKey = "transitlink.language";
const saved = localStorage.getItem(storageKey);
const initial = saved ?? (navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US");

void i18n.use(initReactI18next).init({
  resources: { "zh-CN": { translation: zhCN }, "en-US": { translation: enUS } },
  lng: initial,
  fallbackLng: "zh-CN",
  interpolation: { escapeValue: false },
});

i18n.on("languageChanged", (language) => {
  localStorage.setItem(storageKey, language);
  document.documentElement.lang = language;
});
document.documentElement.lang = initial;

export default i18n;
