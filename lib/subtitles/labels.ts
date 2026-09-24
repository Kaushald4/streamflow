const LANGUAGE_LABELS: Record<string, string> = {
  en: "English",
  eng: "English",
  es: "Spanish",
  spa: "Spanish",
  fr: "French",
  fre: "French",
  fra: "French",
  de: "German",
  ger: "German",
  deu: "German",
  it: "Italian",
  ita: "Italian",
  pt: "Portuguese",
  por: "Portuguese",
  ru: "Russian",
  rus: "Russian",
  ar: "Arabic",
  ara: "Arabic",
  hi: "Hindi",
  hin: "Hindi",
  ja: "Japanese",
  jpn: "Japanese",
  ko: "Korean",
  kor: "Korean",
  zh: "Chinese",
  zho: "Chinese",
  chi: "Chinese",
}

export function subtitleDisplayLabel(track: {
  language?: string
  label?: string
}): string {
  const raw = (track.label ?? track.language ?? "").trim()
  if (!raw) return "Subtitles"
  const key = raw.toLowerCase()
  return LANGUAGE_LABELS[key] ?? raw
}
