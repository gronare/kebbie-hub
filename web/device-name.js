const BROWSERS = [
  [/EdgiOS|EdgA|Edg\//, "Edge"],
  [/FxiOS|Firefox\//, "Firefox"],
  [/SamsungBrowser/, "Samsung Internet"],
  [/OPR\/|OPiOS/, "Opera"],
  [/CriOS|Chrome\//, "Chrome"],
  [/Safari\//, "Safari"],
];

const SYSTEMS = [
  [/iPhone/, "iPhone"],
  [/iPad/, "iPad"],
  [/Android/, "Android"],
  [/CrOS/, "ChromeOS"],
  [/Macintosh|Mac OS X/, "Mac"],
  [/Windows/, "Windows"],
  [/Linux/, "Linux"],
];

const found = (table, text) => table.find(([pattern]) => pattern.test(text))?.[1] ?? null;

export function proposedName(userAgent = "") {
  const system = found(SYSTEMS, userAgent);
  const browser = found(BROWSERS, userAgent);
  if ((system === "iPhone" || system === "iPad") && (browser === "Safari" || !browser)) return system;
  if (system && browser) return `${browser} on ${system}`;
  return system ?? browser ?? "Browser";
}
